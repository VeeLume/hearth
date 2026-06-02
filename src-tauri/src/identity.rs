//! Identity I/O — HTTP fetch of the RSI public citizen page, feeding
//! the pure parser in `hearth_core::profile`.
//!
//! Living here (not in `hearth-core`) keeps the domain crate I/O-free.
//! `verify_account_command` is the Tauri-facing surface; it scrapes
//! once and writes the immutable anchors (`citizen_record`, `enlisted`)
//! to the matching `accounts` row.

use std::time::Duration;

use hearth_core::{ProfileInfo, profile};

const USER_AGENT: &str = concat!(
    "hearth/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/VeeLume/hearth)"
);

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("HTTP error fetching profile for {handle}: {source}")]
    Http {
        handle: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("profile page returned status {status} for {handle}")]
    Status { handle: String, status: u16 },
    #[error("profile parse error for {handle}: {source}")]
    Parse {
        handle: String,
        #[source]
        source: profile::ProfileError,
    },
}

/// Fetch and parse the profile page for `handle`. Network + parse only;
/// the caller decides what to persist.
pub async fn fetch_profile(handle: &str) -> Result<ProfileInfo, IdentityError> {
    let url = format!(
        "https://robertsspaceindustries.com/en/citizens/{}",
        urlencoded(handle)
    );
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(FETCH_TIMEOUT)
        .build()
        .expect("reqwest client");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|source| IdentityError::Http {
            handle: handle.to_string(),
            source,
        })?;
    if !resp.status().is_success() {
        return Err(IdentityError::Status {
            handle: handle.to_string(),
            status: resp.status().as_u16(),
        });
    }
    let body = resp.text().await.map_err(|source| IdentityError::Http {
        handle: handle.to_string(),
        source,
    })?;
    profile::parse(&body).map_err(|source| IdentityError::Parse {
        handle: handle.to_string(),
        source,
    })
}

/// Minimal URL-path-segment encoder — RSI handles are
/// `[A-Za-z0-9_-]`, but encode anything outside that to be safe.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoded_passes_safe_chars() {
        assert_eq!(urlencoded("VeeLume"), "VeeLume");
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn urlencoded_escapes_others() {
        assert_eq!(urlencoded("a b"), "a%20b");
        assert_eq!(urlencoded("a/b"), "a%2Fb");
    }
}
