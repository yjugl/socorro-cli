// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::{Error, Result};

const SERVICE_NAME: &str = "socorro-cli";
const TOKEN_KEY: &str = "api-token";

/// Environment variable pointing to a file containing the API token.
/// Used for CI/headless environments where no system keychain is available.
/// The file should be stored in a location that AI agents cannot read
/// (e.g., outside the project directory, with restricted permissions).
const TOKEN_PATH_ENV_VAR: &str = "SOCORRO_API_TOKEN_PATH";

/// Retrieves the API token, checking sources in order:
/// 1. System keychain (preferred for interactive use)
/// 2. File at path specified by SOCORRO_API_TOKEN_PATH (for CI/headless environments)
///
/// Returns None if no token is found (does not print anything).
pub fn get_token() -> Option<String> {
    // Try system keychain first
    if let Some(token) = get_from_keychain() {
        return Some(token);
    }

    // Fallback for CI/headless environments without a keychain
    get_from_token_file()
}

fn get_from_token_file() -> Option<String> {
    let path = std::env::var(TOKEN_PATH_ENV_VAR).ok()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let token = content.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn get_from_keychain() -> Option<String> {
    match keyring::Entry::new(SERVICE_NAME, TOKEN_KEY) {
        Ok(entry) => match entry.get_password() {
            Ok(password) => Some(password),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => None,
        },
        Err(_) => None,
    }
}

/// Returns detailed status for debugging keychain issues.
pub fn get_keychain_status() -> KeychainStatus {
    match keyring::Entry::new(SERVICE_NAME, TOKEN_KEY) {
        Ok(entry) => match entry.get_password() {
            Ok(_) => KeychainStatus::HasToken,
            Err(e) => {
                // Show all errors for debugging
                KeychainStatus::Error(format!("get_password failed: {:?}", e))
            }
        },
        Err(e) => KeychainStatus::Error(format!("Entry::new failed: {:?}", e)),
    }
}

#[derive(Debug)]
pub enum KeychainStatus {
    HasToken,
    NoToken,
    Error(String),
}

/// Stores the API token in the system keychain.
pub fn store_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE_NAME, TOKEN_KEY)
        .map_err(|e| Error::Keyring(format!("Failed to create entry: {}", e)))?;

    entry
        .set_password(token)
        .map_err(|e| Error::Keyring(format!("Failed to store: {}", e)))?;

    // Verify with a fresh entry (same instance may cache)
    let verify_entry = keyring::Entry::new(SERVICE_NAME, TOKEN_KEY)
        .map_err(|e| Error::Keyring(format!("Failed to create verify entry: {}", e)))?;

    match verify_entry.get_password() {
        Ok(stored) if stored == token => Ok(()),
        Ok(_) => Err(Error::Keyring("Token mismatch after storage".to_string())),
        Err(e) => Err(Error::Keyring(format!(
            "Storage appeared to succeed but verification failed: {}. \
             This may be a Windows Credential Manager issue.",
            e
        ))),
    }
}

/// Removes the API token from the system keychain.
pub fn delete_token() -> Result<()> {
    let entry =
        keyring::Entry::new(SERVICE_NAME, TOKEN_KEY).map_err(|e| Error::Keyring(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
        Err(e) => Err(Error::Keyring(e.to_string())),
    }
}

/// Returns true if a token is stored in the keychain.
pub fn has_token() -> bool {
    get_token().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_get_from_token_file_reads_token() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "my_secret_token").unwrap();

        // Set the env var and test
        std::env::set_var(TOKEN_PATH_ENV_VAR, token_path.to_str().unwrap());
        let result = get_from_token_file();
        std::env::remove_var(TOKEN_PATH_ENV_VAR);

        assert_eq!(result, Some("my_secret_token".to_string()));
    }

    #[test]
    #[serial]
    fn test_get_from_token_file_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "  my_token_with_whitespace  \n").unwrap();

        std::env::set_var(TOKEN_PATH_ENV_VAR, token_path.to_str().unwrap());
        let result = get_from_token_file();
        std::env::remove_var(TOKEN_PATH_ENV_VAR);

        assert_eq!(result, Some("my_token_with_whitespace".to_string()));
    }

    #[test]
    #[serial]
    fn test_get_from_token_file_returns_none_for_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "").unwrap();

        std::env::set_var(TOKEN_PATH_ENV_VAR, token_path.to_str().unwrap());
        let result = get_from_token_file();
        std::env::remove_var(TOKEN_PATH_ENV_VAR);

        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn test_get_from_token_file_returns_none_for_whitespace_only() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        std::fs::write(&token_path, "   \n\t  ").unwrap();

        std::env::set_var(TOKEN_PATH_ENV_VAR, token_path.to_str().unwrap());
        let result = get_from_token_file();
        std::env::remove_var(TOKEN_PATH_ENV_VAR);

        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn test_get_from_token_file_returns_none_for_missing_file() {
        std::env::set_var(TOKEN_PATH_ENV_VAR, "/nonexistent/path/to/token");
        let result = get_from_token_file();
        std::env::remove_var(TOKEN_PATH_ENV_VAR);

        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn test_get_from_token_file_returns_none_when_env_not_set() {
        std::env::remove_var(TOKEN_PATH_ENV_VAR);
        let result = get_from_token_file();
        assert_eq!(result, None);
    }
}
