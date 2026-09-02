// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::fs;
use std::path::PathBuf;

/// Environment variable that overrides the cache directory. Its value is used
/// verbatim as the cache directory (no `socorro-cli` component is appended);
/// an unset or blank value falls back to the OS-standard location below.
///
/// This exists for test isolation. Cache keys carry no base-URL component --
/// `commands::crash_pings` uses `crash-pings-<date>.json` -- and the cache is
/// consulted before the request URL is built, so a test that points a command
/// at a local server would otherwise get a cache hit and silently read
/// production data, or on a miss write its fixture into the user's real cache
/// under a key the real CLI would later read as genuine. Any test that touches
/// the cache must set this to a temporary directory.
pub(crate) const CACHE_DIR_ENV_VAR: &str = "SOCORRO_CACHE_DIR";

/// Resolves the cache directory path without touching the filesystem. Split
/// out of `cache_dir` so that a test can assert on the default, un-overridden
/// path without `create_dir_all`-ing the user's real cache as a side effect.
fn resolve_cache_dir() -> Option<PathBuf> {
    match std::env::var(CACHE_DIR_ENV_VAR) {
        Ok(path) if !path.trim().is_empty() => Some(PathBuf::from(path)),
        _ => Some(dirs::cache_dir()?.join("socorro-cli")),
    }
}

/// Returns the cache directory for socorro-cli, creating it if necessary.
/// Honors the `SOCORRO_CACHE_DIR` override; otherwise uses the OS-standard
/// cache directory:
/// - Linux: ~/.cache/socorro-cli/
/// - macOS: ~/Library/Caches/socorro-cli/
/// - Windows: %LOCALAPPDATA%/socorro-cli/cache/
pub fn cache_dir() -> Option<PathBuf> {
    let dir = resolve_cache_dir()?;
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Read cached data for a given key (filename).
/// Returns None if the cache file doesn't exist or is empty.
pub fn read_cached(key: &str) -> Option<Vec<u8>> {
    let path = cache_dir()?.join(key);
    let data = fs::read(&path).ok()?;
    if data.is_empty() {
        return None;
    }
    Some(data)
}

/// Write data to cache with the given key (filename).
/// Returns true if writing succeeded.
pub fn write_cache(key: &str, data: &[u8]) -> bool {
    let Some(dir) = cache_dir() else {
        return false;
    };
    fs::write(dir.join(key), data).is_ok()
}

/// Points the cache at a fresh temporary directory for as long as the guard
/// lives, and unsets [`CACHE_DIR_ENV_VAR`] on drop -- including when a test
/// panics, so a failing assertion cannot leak the variable into another test.
///
/// Every test that touches the cache needs this, in this module and in
/// `commands::crash_pings`, because the alternative is a test suite that
/// mutates the user's real cache. `write_cache` used to drop
/// `test-cache-roundtrip.txt` into it, `cache_dir` still `create_dir_all`s
/// whatever path it resolves, and crash-ping cache keys are
/// `crash-pings-<date>.json` with no base-URL component -- so a test pointed
/// at a local server would otherwise get a cache hit and silently assert
/// against production data, or on a miss write its fixture into the real cache
/// under a key the CLI would later read as genuine.
///
/// It lives at module scope rather than inside `mod tests` so that both test
/// modules can share one copy; a `#[cfg(test)] mod tests` is not reachable
/// from a sibling module. `#[cfg(test)]` keeps it out of every shipping build,
/// exactly as `crate::test_server` is kept out.
#[cfg(test)]
pub(crate) struct RedirectedCache {
    tmp: tempfile::TempDir,
}

#[cfg(test)]
impl RedirectedCache {
    pub(crate) fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::set_var(CACHE_DIR_ENV_VAR, tmp.path()) };
        Self { tmp }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.tmp.path()
    }
}

#[cfg(test)]
impl Drop for RedirectedCache {
    fn drop(&mut self) {
        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::remove_var(CACHE_DIR_ENV_VAR) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// The guard's whole purpose: the override is gone once it drops, *even if
    /// the test that held it panicked*. Without the `Drop` impl a failing
    /// assertion would leave `SOCORRO_CACHE_DIR` pointing at a deleted
    /// `TempDir`, and every later test in the binary would resolve its cache
    /// to a path that no longer exists. Now that both `cache` and
    /// `commands::crash_pings` share this one guard, that leak would reach
    /// across modules.
    #[test]
    #[serial]
    fn redirected_cache_unsets_the_override_even_when_a_test_panics() {
        assert!(std::env::var(CACHE_DIR_ENV_VAR).is_err());

        let result = std::panic::catch_unwind(|| {
            let cache = RedirectedCache::new();
            assert_eq!(
                std::env::var(CACHE_DIR_ENV_VAR).ok().as_deref(),
                cache.path().to_str()
            );
            panic!("a failing assertion inside the guard's scope");
        });

        assert!(result.is_err(), "the closure was supposed to panic");
        assert!(
            std::env::var(CACHE_DIR_ENV_VAR).is_err(),
            "the guard leaked {} past a panic",
            CACHE_DIR_ENV_VAR
        );
    }

    #[test]
    #[serial]
    fn test_cache_dir_exists() {
        let cache = RedirectedCache::new();

        let dir = cache_dir();

        assert_eq!(dir.as_deref(), Some(cache.path()));
        assert!(dir.unwrap().exists());
    }

    #[test]
    #[serial]
    fn test_read_nonexistent_cache() {
        let _cache = RedirectedCache::new();

        assert!(read_cached("nonexistent-test-file-12345.json").is_none());
    }

    #[test]
    #[serial]
    fn test_write_and_read_cache() {
        let cache = RedirectedCache::new();
        let key = "test-cache-roundtrip.txt";
        let data = b"hello cache";

        assert!(write_cache(key, data));

        assert_eq!(read_cached(key), Some(data.to_vec()));
        // No manual cleanup: the TempDir removes itself on drop.
        assert!(cache.path().join(key).is_file());
    }

    #[test]
    #[serial]
    fn test_empty_cache_returns_none() {
        let _cache = RedirectedCache::new();
        let key = "test-cache-empty.txt";

        assert!(write_cache(key, b""));

        assert!(read_cached(key).is_none());
    }

    #[test]
    #[serial]
    fn test_cache_dir_env_var_redirects_the_cache() {
        let cache = RedirectedCache::new();

        let resolved = cache_dir();
        let wrote = write_cache("redirected.json", b"fixture bytes");

        assert_eq!(resolved.as_deref(), Some(cache.path()));
        assert!(cache.path().is_dir());
        assert!(wrote);
        assert_eq!(
            read_cached("redirected.json").as_deref(),
            Some(&b"fixture bytes"[..])
        );
        assert!(cache.path().join("redirected.json").is_file());
    }

    /// The default branch, which is what every real invocation takes. Asserts
    /// on the resolved path only -- calling `cache_dir()` here would
    /// `create_dir_all` the user's real cache directory.
    #[test]
    #[serial]
    fn test_unset_cache_dir_env_var_resolves_to_the_default() {
        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::remove_var(CACHE_DIR_ENV_VAR) };

        assert_eq!(
            resolve_cache_dir(),
            dirs::cache_dir().map(|d| d.join("socorro-cli"))
        );
    }

    #[test]
    #[serial]
    fn test_blank_cache_dir_env_var_falls_back_to_the_default() {
        // SAFETY: tests using env vars are run serially via #[serial]
        unsafe { std::env::set_var(CACHE_DIR_ENV_VAR, "   ") };
        let resolved = resolve_cache_dir();
        unsafe { std::env::remove_var(CACHE_DIR_ENV_VAR) };

        assert_eq!(resolved, dirs::cache_dir().map(|d| d.join("socorro-cli")));
    }
}
