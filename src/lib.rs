// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod auth;
pub mod cache;
pub mod client;
pub mod commands;
pub mod models;
pub mod output;

pub use auth::{get_token, has_token};
pub use client::SocorroClient;
pub use models::*;
pub use output::OutputFormat;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Crash not found: {0}")]
    NotFound(String),

    #[error("Rate limited. Ask a human to run 'socorro-cli auth login' to set an API token that has no permissions attached to it")]
    RateLimited,

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("Invalid crash ID format: {0}")]
    InvalidCrashId(String),

    #[error("Keyring error: {0}")]
    Keyring(String),
}
