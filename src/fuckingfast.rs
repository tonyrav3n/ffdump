use futures::stream::{self, StreamExt};
use regex::Regex;
use reqwest::{Client, StatusCode};
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

/// Given a list of `fuckingfast.co/<id>` links, concurrently fetches each
/// page's HTML and extracts the embedded `dl.fuckingfast.co/dl/...` direct link.
///
/// Returns `(source_ff_url, dl_link)` pairs so the caller can sort by the
/// source URL's fragment (which encodes the original filename / part number).
/// Links that fail are silently dropped after logging to stderr.
pub async fn extract_direct_links(links: Vec<String>, concurrency: usize) -> Vec<(String, String)> {
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build reqwest client");

    let dl_re =
        Regex::new(r#"https?://dl\.fuckingfast\.co/dl/[^\\"'<>\r\n]+"#).expect("Invalid DL regex");

    stream::iter(links.into_iter().map(|link| {
        let client = client.clone();
        let dl_re = dl_re.clone();

        async move {
            match fetch_direct_link(&client, &dl_re, &link).await {
                Some(dl) => Some((link, dl)),
                None => {
                    eprintln!("  FAIL  No direct link found: {}", link);
                    None
                }
            }
        }
    }))
    .buffer_unordered(concurrency)
    .filter_map(|opt| async { opt })
    .collect()
    .await
}

/// Fetch one fuckingfast.co landing page and extract the dl link.
/// Retries up to 3 times with exponential back-off on rate-limit (HTTP 429).
async fn fetch_direct_link(client: &Client, dl_re: &Regex, link: &str) -> Option<String> {
    // Strip the URL fragment before sending — fragments are client-side only
    // and the reqwest client would include it literally in the path otherwise.
    let request_url = link.split('#').next().unwrap_or(link);

    for attempt in 0u32..3 {
        if attempt > 0 {
            let delay = Duration::from_millis(2000 * (1 << (attempt - 1)));
            tokio::time::sleep(delay).await;
        }

        let resp = match client.get(request_url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "  FAIL  Request error for {} (attempt {}): {}",
                    link,
                    attempt + 1,
                    e
                );
                if attempt < 2 {
                    continue;
                }
                return None;
            }
        };

        match resp.status() {
            StatusCode::TOO_MANY_REQUESTS => {
                eprintln!(
                    "  WARN  Rate limited on {} (attempt {}), backing off...",
                    link,
                    attempt + 1
                );
                continue;
            }
            s if !s.is_success() => {
                eprintln!("  FAIL  HTTP {} for: {}", s, link);
                return None;
            }
            _ => {}
        }

        let html = match resp.text().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("  FAIL  Failed to read body for {}: {}", link, e);
                return None;
            }
        };

        if let Some(m) = dl_re.find(&html) {
            return Some(m.as_str().to_string());
        }

        return None; // page loaded but no DL link — no point retrying
    }

    None
}
