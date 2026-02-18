// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::fs;
use std::path::PathBuf;

/// Returns the cache directory for socorro-cli, creating it if necessary.
/// Uses the OS-standard cache directory:
/// - Linux: ~/.cache/socorro-cli/
/// - macOS: ~/Library/Caches/socorro-cli/
/// - Windows: %LOCALAPPDATA%/socorro-cli/cache/
pub fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("socorro-cli");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_dir_exists() {
        let dir = cache_dir();
        assert!(dir.is_some());
        assert!(dir.unwrap().exists());
    }

    #[test]
    fn test_read_nonexistent_cache() {
        let result = read_cached("nonexistent-test-file-12345.json");
        assert!(result.is_none());
    }

    #[test]
    fn test_write_and_read_cache() {
        let key = "test-cache-roundtrip.txt";
        let data = b"hello cache";
        assert!(write_cache(key, data));
        let read_back = read_cached(key);
        assert_eq!(read_back, Some(data.to_vec()));

        // Cleanup
        if let Some(dir) = cache_dir() {
            let _ = fs::remove_file(dir.join(key));
        }
    }

    #[test]
    fn test_empty_cache_returns_none() {
        let key = "test-cache-empty.txt";
        assert!(write_cache(key, b""));
        let result = read_cached(key);
        assert!(result.is_none());

        // Cleanup
        if let Some(dir) = cache_dir() {
            let _ = fs::remove_file(dir.join(key));
        }
    }
}
