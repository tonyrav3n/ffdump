mod fuckingfast;
mod privatebin;

use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;

const DEFAULT_CONCURRENCY: usize = 15;

fn usage(prog: &str) {
    eprintln!(
        "Usage:  {} <paste-url> [--concurrency N] [--password P] [--output FILE]",
        prog
    );
    eprintln!();
    eprintln!("  <paste-url>         PrivateBin URL — MUST be in double-quotes so the");
    eprintln!("                      shell doesn't strip the '#' key fragment.");
    eprintln!(
        "  --concurrency N     Parallel fuckingfast requests (default: {})",
        DEFAULT_CONCURRENCY
    );
    eprintln!("  --password P        Paste password, if the paste is password-protected.");
    eprintln!("  --output FILE       Output file path (overrides the auto-generated name).");
    eprintln!();
    eprintln!("Example:");
    eprintln!(
        r#"  {} "https://paste.fitgirl-repacks.site/?abc123#SomeBase58Key""#,
        prog
    );
}

/// Try to derive the game name from the fuckingfast URL fragments.
///
/// FitGirl filenames look like:
///   `https://fuckingfast.co/<id>#GameName_--_fitgirl-repacks.site_--_.part001.rar`
///
/// We take the part before the first `_--_` as the game name.
/// Falls back to parsing the first non-URL line of the paste, then "download".
fn extract_game_name(ff_links: &[String], paste_text: &str) -> String {
    // Method 1: FF URL fragment
    for link in ff_links {
        if let Some(hash_pos) = link.find('#') {
            let fragment = &link[hash_pos + 1..];
            let name = fragment.split("_--_").next().unwrap_or("").trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }

    // Method 2: first non-empty, non-URL line of the paste text
    for line in paste_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("http") {
            continue;
        }
        // Sanitize: keep alphanumeric, hyphens, underscores, dots — replace the rest
        let sanitized: String = line
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let sanitized = sanitized.trim_matches('_').to_string();
        if sanitized.len() >= 2 {
            return sanitized;
        }
    }

    "download".to_string()
}

/// Build the output path: `<Documents>/<game_name>_direct_fuckingfast_links.txt`
/// Uses the `dirs` crate so it works correctly on Windows, macOS, and Linux.
/// Falls back to the current directory if the Documents folder can't be resolved.
fn build_output_path(game_name: &str) -> PathBuf {
    let filename = format!("{}_direct_fuckingfast_links.txt", game_name);
    match dirs::document_dir() {
        Some(dir) => {
            if let Err(e) = fs::create_dir_all(&dir) {
                eprintln!(
                    "WARN: could not create Documents dir: {} -- saving here instead.",
                    e
                );
                return PathBuf::from(filename);
            }
            dir.join(filename)
        }
        None => PathBuf::from(filename),
    }
}

#[tokio::main]
async fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    let prog = raw_args.first().map(String::as_str).unwrap_or("ffdump");

    // ── Argument parsing ───────────────────────────────────────────────────
    let mut url: Option<String> = None;
    let mut concurrency = DEFAULT_CONCURRENCY;
    let mut password: Option<String> = None;
    let mut output_override: Option<PathBuf> = None;
    let mut verbose = false;

    let mut i = 1usize;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--concurrency" | "-c" => {
                i += 1;
                match raw_args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    Some(n) if n > 0 => concurrency = n,
                    _ => {
                        eprintln!("Error: --concurrency requires a positive integer.");
                        process::exit(1);
                    }
                }
            }
            "--password" | "-p" => {
                i += 1;
                match raw_args.get(i) {
                    Some(p) => password = Some(p.clone()),
                    None => {
                        eprintln!("Error: --password requires an argument.");
                        process::exit(1);
                    }
                }
            }
            "--output" | "-o" => {
                i += 1;
                match raw_args.get(i) {
                    Some(f) => output_override = Some(PathBuf::from(f)),
                    None => {
                        eprintln!("Error: --output requires an argument.");
                        process::exit(1);
                    }
                }
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--help" | "-h" => {
                usage(prog);
                process::exit(0);
            }
            arg if !arg.starts_with('-') => {
                url = Some(arg.to_string());
            }
            unknown => {
                eprintln!("Unknown argument: {}", unknown);
                usage(prog);
                process::exit(1);
            }
        }
        i += 1;
    }

    let url = match url {
        Some(u) => u,
        None => {
            usage(prog);
            process::exit(1);
        }
    };

    // ── Phase 1: Decrypt the PrivateBin paste ─────────────────────────────
    let paste_text = match privatebin::fetch_and_decrypt(&url, password.as_deref()).await {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Error: {:#}", e);
            process::exit(1);
        }
    };

    // ── Extract fuckingfast links from the paste text ─────────────────────
    let ff_re = Regex::new(r#"https?://(?:www\.)?fuckingfast\.co/[^\s"'<>\r\n]+"#)
        .expect("Invalid fuckingfast regex");

    let mut ff_links: Vec<String> = ff_re
        .find_iter(&paste_text)
        .map(|m| m.as_str().to_string())
        .collect();

    // Sort by the URL fragment (encodes the filename / part number) so that
    // when we use these as sort keys later, parts come out in order.
    ff_links.sort_unstable();
    ff_links.dedup();

    if ff_links.is_empty() {
        eprintln!("No fuckingfast.co links were found in the paste.");
        eprintln!(
            "The paste may be empty, use a different mirror, or the format may have changed."
        );
        process::exit(1);
    }

    let total = ff_links.len();

    // Derive game name before we move ff_links into the extractor
    let game_name = extract_game_name(&ff_links, &paste_text);
    let output_path = output_override.unwrap_or_else(|| build_output_path(&game_name));

    println!(
        "Found {} fuckingfast.co link{}.",
        total,
        if total == 1 { "" } else { "s" }
    );
    println!(
        "Extracting direct download links ({} concurrent requests)...",
        concurrency
    );

    // ── Phase 2: Concurrently fetch direct dl links ───────────────────────
    // Returns (source_ff_url, dl_link) pairs in arbitrary completion order.
    let mut pairs = fuckingfast::extract_direct_links(ff_links, concurrency).await;

    // Sort by the URL fragment (everything after '#'), which encodes the original
    // filename e.g. "RDR2_--_fitgirl-repacks.site_--_.part001.rar".
    // The opaque path segment before '#' is random and must NOT be used for ordering.
    pairs.sort_unstable_by(|a, b| {
        let fa = a.0.split_once('#').map(|(_, f)| f).unwrap_or(&a.0);
        let fb = b.0.split_once('#').map(|(_, f)| f).unwrap_or(&b.0);
        fa.cmp(fb)
    });
    // Dedup by dl link in case any two source URLs resolved to the same file
    pairs.dedup_by(|a, b| a.1 == b.1);

    let extracted = pairs.len();

    // ── Phase 3: Write results ────────────────────────────────────────────
    if verbose {
        println!();
        println!("{:<12}  {}", "SOURCE FILE", "DIRECT LINK (first 60 chars)");
        println!("{}", "-".repeat(80));
        for (src, dl) in &pairs {
            let frag = src.split_once('#').map(|(_, f)| f).unwrap_or(src);
            let dl_short = &dl[..dl.len().min(60)];
            println!("{:<50}  {}...", frag, dl_short);
        }
        println!();
    }

    let write_result = fs::File::create(&output_path).and_then(|mut file| {
        for (_, dl_link) in &pairs {
            writeln!(file, "{}", dl_link)?;
        }
        Ok(())
    });

    match write_result {
        Ok(()) => {
            println!();
            println!(
                "Done: extracted {}/{} direct links -> {}",
                extracted,
                total,
                output_path.display()
            );
        }
        Err(e) => {
            eprintln!(
                "Failed to write '{}': {} — printing to stdout instead.",
                output_path.display(),
                e
            );
            for (_, dl_link) in &pairs {
                println!("{}", dl_link);
            }
        }
    }

    if extracted < total {
        eprintln!(
            "WARN: {} link{} could not be resolved (see errors above).",
            total - extracted,
            if total - extracted == 1 { "" } else { "s" }
        );
        process::exit(1);
    }
}
