use crate::{auth, Result};
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

pub fn status() -> Result<()> {
    match auth::get_keychain_status() {
        auth::KeychainStatus::HasToken => {
            println!("Token is stored in system keychain.");
        }
        auth::KeychainStatus::NoToken => {
            println!("No token stored in keychain.");
            check_token_path_fallback();
        }
        auth::KeychainStatus::Error(e) => {
            println!("Keychain error: {}", e);
            check_token_path_fallback();
        }
    }
    Ok(())
}
