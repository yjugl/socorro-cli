// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// Keychain is available on Windows (windows-native) and macOS (apple-native)
// unconditionally, but on Linux it requires the secret-service feature (D-Bus).
#[cfg(any(target_os = "windows", target_os = "macos", feature = "secret-service"))]
mod keychain_available {
    use crate::{Result, auth};
    use std::io::{self, Write};

    pub fn login() -> Result<()> {
        if auth::has_token() {
            print!("A token is already stored. Replace it? [y/N] ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Cancelled.");
                return Ok(());
            }
        }

        print!("Enter your Socorro API token: ");
        io::stdout().flush().unwrap();

        let token = rpassword::read_password().unwrap_or_default();

        if token.is_empty() {
            println!("No token provided. Cancelled.");
            return Ok(());
        }

        auth::store_token(&token)?;
        println!("Token stored in system keychain.");
        Ok(())
    }

    pub fn logout() -> Result<()> {
        if !auth::has_token() {
            println!("No token stored.");
            return Ok(());
        }

        auth::delete_token()?;
        println!("Token removed from system keychain.");
        Ok(())
    }

    pub fn status() -> Result<()> {
        match auth::get_keychain_status() {
            auth::KeychainStatus::HasToken => {
                println!("Token is stored in system keychain.");
            }
            auth::KeychainStatus::NoToken => {
                println!("No token stored in keychain.");
                super::check_token_path_fallback();
            }
            auth::KeychainStatus::Error(e) => {
                println!("Keychain error: {}", e);
                super::check_token_path_fallback();
            }
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", feature = "secret-service")))]
mod keychain_unavailable {
    use crate::Result;

    const NO_KEYCHAIN_MSG: &str = "\
This build of socorro-cli was compiled without system keychain support.
Use the SOCORRO_API_TOKEN_PATH environment variable to point to a file
containing your API token instead.";

    pub fn login() -> Result<()> {
        eprintln!("Error: 'auth login' is not available in this build.");
        eprintln!();
        eprintln!("{}", NO_KEYCHAIN_MSG);
        std::process::exit(1);
    }

    pub fn logout() -> Result<()> {
        eprintln!("Error: 'auth logout' is not available in this build.");
        eprintln!();
        eprintln!("{}", NO_KEYCHAIN_MSG);
        std::process::exit(1);
    }

    pub fn status() -> Result<()> {
        println!("System keychain support is not available in this build.");
        println!("Only SOCORRO_API_TOKEN_PATH is supported for authentication.");
        println!();
        super::check_token_path_fallback();
        Ok(())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", feature = "secret-service"))]
pub use keychain_available::{login, logout, status};

#[cfg(not(any(target_os = "windows", target_os = "macos", feature = "secret-service")))]
pub use keychain_unavailable::{login, logout, status};

fn check_token_path_fallback() {
    if let Ok(path) = std::env::var("SOCORRO_API_TOKEN_PATH") {
        if std::path::Path::new(&path).exists() {
            println!("SOCORRO_API_TOKEN_PATH is set and file exists (CI fallback).");
        } else {
            println!(
                "SOCORRO_API_TOKEN_PATH is set but file does not exist: {}",
                path
            );
        }
    }
}
