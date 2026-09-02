// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! A minimal, test-only HTTP server for exercising the crate's HTTP paths
//! offline.
//!
//! The crate's error handling (404 -> [`crate::Error::NotFound`], 429 ->
//! [`crate::Error::RateLimited`], a malformed body -> [`crate::Error::ParseError`])
//! can only be reached through a real HTTP round trip, because the blocking
//! `reqwest` client is not abstracted behind a trait. This module binds a
//! loopback socket, serves responses a test has queued up front, and hands back
//! a `base_url` that [`crate::SocorroClient::new`] and the command modules can
//! be pointed at.
//!
//! Each answered request is recorded ([`TestServer::requests`]) as a path plus
//! the *names* of the headers it carried, which is what lets a test assert that
//! `Auth-Token` was or was not sent. Header values are never captured; see
//! [`RecordedRequest`].
//!
//! It is deliberately hand-rolled over [`std::net::TcpListener`] rather than
//! pulling in a mock-server crate: no new dependency (so nothing to review for
//! MPL-2.0 compatibility), and full control over the raw status line, which
//! matters for statuses such as `202` that `reqwest`'s own helpers do not treat
//! as errors.
//!
//! The module is declared `#[cfg(test)]` in `src/lib.rs`, so it is never
//! compiled into a shipping build.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// How long the accept loop sleeps between polls of the non-blocking listener.
/// This also bounds how long [`TestServer::drop`] blocks while joining.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Guard against a wedged connection holding the server thread forever: a
/// client that opens a socket and never finishes its request head must not
/// stall the queue for the rest of the test run.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// One queued HTTP response: a status code and the raw body bytes to send.
///
/// The body is `Vec<u8>` rather than `String` so a test can serve invalid
/// UTF-8, or a body whose length splits a multi-byte character at an awkward
/// offset.
struct QueuedResponse {
    status: u16,
    body: Vec<u8>,
}

/// A request the server answered, recorded so a test can assert on what the
/// code under test put on the wire.
///
/// **Header values are deliberately never captured.** [`crate::auth::get_token`]
/// consults the real system keychain before falling back to a file, so a test
/// running on a developer machine can genuinely send a live API token to this
/// server. Storing only header *names* makes leaking one structurally
/// impossible: no assertion message, `Debug` render, or panic backtrace can
/// print a value that was never put in this struct. Do not "improve" this later
/// by recording values, not even redacted ones -- the guarantee is the absence
/// of the data, not the care taken with it.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// The request target from the request line, including any query string
    /// (e.g. `/ProcessedCrash/?crash_id=...`). This crate only ever sends the
    /// API token as a header, never as a query parameter, so the target is safe
    /// to record; keep it that way.
    pub path: String,
    /// Header names, lowercased, in the order the client sent them. Values are
    /// not recorded -- see the note on this struct.
    pub header_names: Vec<String>,
}

impl RecordedRequest {
    /// Whether the request carried a header with this name, compared
    /// case-insensitively.
    ///
    /// This is the whole point of the type: it answers "was `Auth-Token`
    /// sent?" without anyone needing to look at what was sent.
    pub fn has_header(&self, name: &str) -> bool {
        // The recorded names are already lowercased by `from_head`, so
        // normalising the argument is what makes the comparison
        // case-insensitive. It is load-bearing -- `src/client.rs` asserts on
        // `auth-token` while `reqwest` sends `Auth-Token` -- so do not drop it
        // on the grounds that `contains` looks like it needs no setup.
        let name = name.to_ascii_lowercase();
        self.header_names.contains(&name)
    }

    /// Build a record from the request head lines produced by [`read_head`].
    ///
    /// The first line is the request line; the rest are headers, of which only
    /// the name (the part before the first `:`) is kept.
    fn from_head(head: &[String]) -> Self {
        let path = head
            .first()
            .and_then(|request_line| request_line.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string();

        let header_names = head[1..]
            .iter()
            .filter_map(|line| line.split_once(':'))
            .map(|(name, _value)| name.trim().to_ascii_lowercase())
            .collect();

        Self { path, header_names }
    }
}

/// A loopback HTTP server that replies with responses queued by the test.
///
/// Create one with [`TestServer::start`], queue responses with
/// [`TestServer::push_response`], and point the code under test at
/// [`TestServer::base_url`]. Requests are answered in the order they were
/// queued; a request arriving with an empty queue gets a `500` whose body says
/// so, which makes an under-queued test fail loudly instead of hanging.
///
/// The server listens on `127.0.0.1:0` and reports the OS-assigned port, so
/// concurrent tests never collide, and it shuts its accept loop down when the
/// value is dropped.
pub struct TestServer {
    port: u16,
    queue: Arc<Mutex<VecDeque<QueuedResponse>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    /// Bind `127.0.0.1:0` and start serving in a background thread.
    ///
    /// Panics if the socket cannot be bound, which in a test is the right
    /// response: there is nothing useful to fall back to.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let port = listener
            .local_addr()
            .expect("read local_addr of test server")
            .port();
        listener
            .set_nonblocking(true)
            .expect("set test server listener non-blocking");

        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let handle = {
            let queue = Arc::clone(&queue);
            let requests = Arc::clone(&requests);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || serve(listener, &queue, &requests, &shutdown))
        };

        Self {
            port,
            queue,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    /// Queue `body` to be served with HTTP status `status`.
    ///
    /// Responses are served in the order they were pushed, one per request.
    /// Accepts anything that converts into `Vec<u8>`, so `&str`, `String`,
    /// `&[u8]` and `Vec<u8>` all work.
    pub fn push_response(&self, status: u16, body: impl Into<Vec<u8>>) {
        self.queue
            .lock()
            .expect("test server queue mutex poisoned")
            .push_back(QueuedResponse {
                status,
                body: body.into(),
            });
    }

    /// The requests answered so far, oldest first.
    ///
    /// Each carries the request target and the names of the headers that were
    /// sent -- never their values. See [`RecordedRequest`] for why.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .expect("test server request log mutex poisoned")
            .clone()
    }

    /// The URL to hand to [`crate::SocorroClient::new`], with no trailing
    /// slash: the client builds endpoint URLs as `{base_url}/ProcessedCrash/`.
    ///
    /// The path is ignored by the server, so a test may append one freely.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Accept loop: poll the non-blocking listener until the owning [`TestServer`]
/// is dropped, answering one request per connection.
fn serve(
    listener: TcpListener,
    queue: &Mutex<VecDeque<QueuedResponse>>,
    requests: &Mutex<Vec<RecordedRequest>>,
    shutdown: &AtomicBool,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(err) = handle_connection(stream, queue, requests) {
                    eprintln!("test server: failed to serve a connection: {err}");
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_INTERVAL),
            Err(err) => {
                eprintln!("test server: accept failed, stopping: {err}");
                break;
            }
        }
    }
}

/// Read one request in full, then write back the next queued response.
///
/// The head is consumed up to and including the terminating blank line, and any
/// request body announced by `Content-Length` is drained, before anything is
/// written. Replying while the client is still writing risks the client
/// blocking on a full send buffer while we block on a full one of our own,
/// which a blocking `reqwest` client cannot recover from.
fn handle_connection(
    mut stream: TcpStream,
    queue: &Mutex<VecDeque<QueuedResponse>>,
    requests: &Mutex<Vec<RecordedRequest>>,
) -> io::Result<()> {
    // `accept` on a non-blocking listener can yield a non-blocking socket on
    // some platforms; force blocking reads so the head is read in one pass.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;

    let head = {
        let mut reader = BufReader::new(&mut stream);
        let head = read_head(&mut reader)?;
        if head.is_empty() {
            // A connection that was opened and closed without sending anything
            // must not consume a queued response.
            return Ok(());
        }
        drain_request_body(&mut reader, &head)?;
        head
    };
    debug_assert!(!head.is_empty());

    // Record before answering, so a request that gets the empty-queue 500 still
    // shows up in the log.
    requests
        .lock()
        .expect("test server request log mutex poisoned")
        .push(RecordedRequest::from_head(&head));

    let queued = queue
        .lock()
        .expect("test server queue mutex poisoned")
        .pop_front();
    let (status, body) = match queued {
        Some(response) => (response.status, response.body),
        None => (
            500,
            b"test server: response queue is empty".to_vec() as Vec<u8>,
        ),
    };

    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        reason = reason_phrase(status),
        len = body.len(),
    )
    .into_bytes();
    out.extend_from_slice(&body);

    stream.write_all(&out)?;
    stream.flush()?;
    // Signal end-of-response explicitly, as `Connection: close` promises.
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

/// Read the request head, returning its lines with trailing CRLF stripped.
///
/// Returns an empty vector if the peer closed before sending anything.
fn read_head(reader: &mut BufReader<&mut TcpStream>) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            // Peer closed mid-head; hand back whatever we got.
            break;
        }
        if line == "\r\n" || line == "\n" {
            // The terminating blank line has now been consumed.
            break;
        }
        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    Ok(lines)
}

/// Discard a request body of the length announced by `Content-Length`, if any.
///
/// Leaving an unread body in the socket makes the client's write block, so this
/// is part of "read the request fully before replying" even though the code
/// under test only issues GETs today.
fn drain_request_body(reader: &mut BufReader<&mut TcpStream>, head: &[String]) -> io::Result<()> {
    let length = head.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<u64>().ok()
        } else {
            None
        }
    });

    if let Some(length) = length
        && length > 0
    {
        io::copy(&mut reader.take(length), &mut io::sink())?;
    }
    Ok(())
}

/// Reason phrase for the status line.
///
/// HTTP clients ignore this text, but the status line is malformed without a
/// third field, so every status needs something here.
fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client that ignores any ambient proxy configuration: the server is on
    /// loopback, and an `http_proxy` in the environment would otherwise send
    /// the request somewhere else entirely.
    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build blocking test client")
    }

    #[test]
    fn serves_queued_status_and_body() {
        let server = TestServer::start();
        server.push_response(404, r#"{"error": "not found"}"#);

        let response = client()
            .get(format!("{}/ProcessedCrash/", server.base_url()))
            .send()
            .expect("request to test server");

        assert_eq!(response.status().as_u16(), 404);
        assert_eq!(response.text().unwrap(), r#"{"error": "not found"}"#);
    }

    #[test]
    fn serves_queued_responses_in_order() {
        let server = TestServer::start();
        server.push_response(200, "first");
        server.push_response(429, "second");

        let client = client();

        let first = client
            .get(server.base_url())
            .send()
            .expect("first request to test server");
        assert_eq!(first.status().as_u16(), 200);
        assert_eq!(first.text().unwrap(), "first");

        let second = client
            .get(server.base_url())
            .send()
            .expect("second request to test server");
        assert_eq!(second.status().as_u16(), 429);
        assert_eq!(second.text().unwrap(), "second");
    }

    #[test]
    fn serves_202_with_body() {
        let server = TestServer::start();
        server.push_response(202, "accepted, not ready yet");

        let response = client()
            .get(server.base_url())
            .send()
            .expect("request to test server");

        assert_eq!(response.status().as_u16(), 202);
        // 202 is a success status, so reqwest's own helper does not flag it:
        // the crash-pings code has to recognise it explicitly.
        assert!(response.error_for_status_ref().is_ok());
        assert_eq!(response.text().unwrap(), "accepted, not ready yet");
    }

    #[test]
    fn serves_body_split_mid_utf8_sequence() {
        // 199 ASCII bytes then a 3-byte em dash: byte index 200 lands inside
        // the multi-byte sequence, which is what makes a naive `&body[..200]`
        // slice panic.
        let body = format!("{}\u{2014}", "a".repeat(199));
        assert_eq!(body.len(), 202);
        assert!(!body.is_char_boundary(200));

        let server = TestServer::start();
        server.push_response(200, body.clone());

        let response = client()
            .get(server.base_url())
            .send()
            .expect("request to test server");

        assert_eq!(response.status().as_u16(), 200);
        let received = response.text().unwrap();
        assert_eq!(received.len(), 202);
        assert_eq!(received.chars().last(), Some('\u{2014}'));
        assert_eq!(received, body);
    }

    #[test]
    fn dropping_the_server_stops_listening() {
        let base_url = {
            let server = TestServer::start();
            server.push_response(200, "still up");
            let response = client()
                .get(server.base_url())
                .send()
                .expect("request to test server");
            assert_eq!(response.status().as_u16(), 200);
            server.base_url()
        };

        // `Drop` joins the accept loop, which owns the listener, so the port is
        // closed by the time the scope above ends.
        let address = base_url.trim_start_matches("http://").to_string();
        assert!(
            std::net::TcpStream::connect(&address).is_err(),
            "test server at {address} still accepts connections after being dropped"
        );
    }

    #[test]
    fn records_path_and_names_of_headers_that_were_sent() {
        let server = TestServer::start();
        server.push_response(200, "ok");

        client()
            .get(format!(
                "{}/ProcessedCrash/?crash_id=abc",
                server.base_url()
            ))
            .header("Auth-Token", "irrelevant-and-never-recorded")
            .send()
            .expect("request to test server")
            .text()
            .expect("read response body");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.path, "/ProcessedCrash/?crash_id=abc");
        // Present by the name the client used, and by any casing of it.
        assert!(request.has_header("auth-token"));
        assert!(request.has_header("Auth-Token"));
        // A header reqwest adds itself, to show the whole head is parsed.
        assert!(request.has_header("host"));
        // Names are normalised to lowercase in the record itself.
        assert!(request.header_names.contains(&"auth-token".to_string()));
    }

    #[test]
    fn reports_a_header_that_was_not_sent_as_absent() {
        let server = TestServer::start();
        server.push_response(200, "ok");

        client()
            .get(format!("{}/ProcessedCrash/", server.base_url()))
            .send()
            .expect("request to test server")
            .text()
            .expect("read response body");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(!request.has_header("auth-token"));
        // Guard against passing vacuously: absence must be a real observation
        // about a head that was recorded, not an empty record.
        assert!(request.has_header("host"));
        assert!(!request.header_names.is_empty());
    }

    #[test]
    fn never_records_a_header_value() {
        // Stands in for an API token: if this string can be reached through the
        // recording API or its `Debug` render, the harness could leak a real
        // one into a test failure message.
        const SENTINEL: &str = "sentinel-value-that-must-never-be-recorded";

        let server = TestServer::start();
        server.push_response(200, "ok");

        client()
            .get(server.base_url())
            .header("Auth-Token", SENTINEL)
            .header("X-Other", SENTINEL)
            .send()
            .expect("request to test server")
            .text()
            .expect("read response body");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].has_header("auth-token"));
        assert!(
            !format!("{:?}", requests[0]).contains(SENTINEL),
            "a header value reached the request record"
        );
    }

    #[test]
    fn records_a_request_answered_from_an_empty_queue() {
        let server = TestServer::start();

        client()
            .get(format!("{}/Bugs/", server.base_url()))
            .send()
            .expect("request to test server")
            .text()
            .expect("read response body");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/Bugs/");
    }

    #[test]
    fn empty_queue_yields_a_loud_500() {
        let server = TestServer::start();

        let response = client()
            .get(server.base_url())
            .send()
            .expect("request to test server");

        assert_eq!(response.status().as_u16(), 500);
        assert!(response.text().unwrap().contains("response queue is empty"));
    }
}
