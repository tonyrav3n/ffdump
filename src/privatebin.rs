use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use flate2::read::DeflateDecoder;
use std::io::Read;

use aes_gcm::aead::consts::{U12, U16};
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::Aes256;
use aes_gcm::{Aes256Gcm, AesGcm};

use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

/// Fetch and decrypt a PrivateBin paste URL.
///
/// The URL must include the fragment key, e.g.:
///   https://paste.example.com/?abc123#Base58EncodedKey
///
/// Supports v2 (AES-256-GCM + zlib) and v1 (legacy) paste formats.
pub async fn fetch_and_decrypt(url: &str, password: Option<&str>) -> Result<String> {
    // ── 1. Parse URL: separate everything before '#' from the key fragment ──
    let (base_url, fragment) = url
        .split_once('#')
        .ok_or_else(|| anyhow!("URL has no '#' fragment — is this a valid PrivateBin URL?\nWrap the URL in double-quotes so the shell doesn't strip the fragment."))?;

    // Strip leading '-' used by burn-after-reading safe URLs (PrivateBin >= 1.7)
    let fragment = fragment.strip_prefix('-').unwrap_or(fragment);

    // ── 2. Fetch the encrypted JSON payload via the JSON API ──
    println!("Fetching PrivateBin paste...");
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; ffdump/0.1)")
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get(base_url)
        .header("X-Requested-With", "JSONHttpRequest")
        .send()
        .await
        .context("Failed to reach PrivateBin server")?;

    if !response.status().is_success() {
        bail!("PrivateBin server returned HTTP {}", response.status());
    }

    let json: serde_json::Value = response
        .json()
        .await
        .context("Server response was not valid JSON")?;

    // Check PrivateBin API status field
    if json["status"].as_i64() == Some(1) {
        bail!(
            "PrivateBin API error: {}",
            json["message"].as_str().unwrap_or("unknown error")
        );
    }

    // Detect format version
    let version = json["v"].as_u64().unwrap_or(1);

    // ── 3. Base58-decode the key ──
    let key_bytes = bs58::decode(fragment)
        .into_vec()
        .context("Failed to base58-decode the key fragment")?;

    println!("Decrypting paste (v{}, AES-256-GCM)...", version);

    let plaintext = if version == 2 {
        decrypt_v2(&json, &key_bytes, password)?
    } else {
        decrypt_v1(&json, &key_bytes, password)?
    };

    // ── 8. Parse the inner paste JSON ──
    // The decrypted plaintext is JSON: {"paste":"...content..."}
    let inner: serde_json::Value =
        serde_json::from_str(&plaintext).context("Failed to parse decrypted paste as JSON")?;

    let paste_text = inner["paste"]
        .as_str()
        .ok_or_else(|| anyhow!("Decrypted paste has no 'paste' field — unexpected format"))?
        .to_string();

    Ok(paste_text)
}

// ── v2 decryption (PrivateBin >= 1.3, the current format) ──────────────────
fn decrypt_v2(
    json: &serde_json::Value,
    key_bytes: &[u8],
    password: Option<&str>,
) -> Result<String> {
    // adata structure:
    //   adata[0] = [iv_b64, salt_b64, iterations, keysize_bits, tagsize_bits, "aes", "gcm", compression]
    //   adata[1] = format string
    //   adata[2] = open-discussion flag
    //   adata[3] = burn-after-reading flag
    let adata = &json["adata"];
    let crypto_params = adata
        .get(0)
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Invalid adata[0] — expected an array"))?;

    let iv_b64 = crypto_params
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing IV in adata[0][0]"))?;
    let salt_b64 = crypto_params
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing salt in adata[0][1]"))?;
    let iterations = crypto_params
        .get(2)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("Missing iterations in adata[0][2]"))? as u32;
    let key_size_bits = crypto_params.get(3).and_then(|v| v.as_u64()).unwrap_or(256) as usize;
    let compression = crypto_params
        .get(7)
        .and_then(|v| v.as_str())
        .unwrap_or("zlib");

    let iv = BASE64
        .decode(iv_b64)
        .context("Failed to base64-decode IV")?;
    let salt = BASE64
        .decode(salt_b64)
        .context("Failed to base64-decode salt")?;

    // ── 5. PBKDF2-HMAC-SHA256 key derivation ──
    let key_size_bytes = key_size_bits / 8;
    let mut passphrase = key_bytes.to_vec();
    if let Some(pwd) = password {
        passphrase.extend_from_slice(pwd.as_bytes());
    }

    let mut derived_key = vec![0u8; key_size_bytes];
    pbkdf2_hmac::<Sha256>(&passphrase, &salt, iterations, &mut derived_key);

    // ── 6. Base64-decode the ciphertext (includes GCM auth tag) ──
    let ct_b64 = json["ct"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing ciphertext 'ct' field in paste JSON"))?;
    let ciphertext = BASE64
        .decode(ct_b64)
        .context("Failed to base64-decode ciphertext")?;

    // ── 7. AES-256-GCM decrypt ──
    // The AAD is the compact JSON serialization of the entire adata array,
    // matching JavaScript's JSON.stringify(adata).
    let aad = serde_json::to_string(adata).context("Failed to re-serialize adata")?;

    let plaintext_bytes = aes_gcm_decrypt(&derived_key, &iv, ciphertext.as_slice(), aad.as_bytes())
        .map_err(|e| {
            anyhow!(
                "AES-GCM decryption failed: {} — wrong key or corrupted paste",
                e
            )
        })?;

    // ── 8. Decompress ──
    decompress(plaintext_bytes.as_slice(), compression)
}

// ── v1 decryption (PrivateBin <= 1.2, legacy format) ──────────────────────
fn decrypt_v1(
    json: &serde_json::Value,
    key_bytes: &[u8],
    password: Option<&str>,
) -> Result<String> {
    // In v1, the cipher data is inside json["ct"] or json["data"]
    // The adata approach differs: passphrase = base64(key_bytes) [+ hex(sha256(password))]
    let ct_b64 = json["ct"]
        .as_str()
        .or_else(|| json["data"].as_str())
        .ok_or_else(|| anyhow!("No ciphertext in v1 paste"))?;

    // For v1 pastes without password, passphrase = base64(key_bytes)
    let mut passphrase_str = BASE64.encode(key_bytes);
    if let Some(pwd) = password {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(pwd.as_bytes());
        let hash_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        passphrase_str.push_str(&hash_hex);
    }

    // v1 uses SJCL cipher format: parse the cipher_data object
    let cipher_data = json.get("ct").map(|_| json).unwrap_or(json);

    let iv_b64 = cipher_data["iv"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing IV in v1 paste"))?;
    let salt_b64 = cipher_data["salt"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing salt in v1 paste"))?;
    let iterations = cipher_data["iter"].as_u64().unwrap_or(10000) as u32;
    let key_size_bits = cipher_data["ks"].as_u64().unwrap_or(256) as usize;
    let compression = cipher_data["adata"].as_str().unwrap_or("none");

    let iv = BASE64
        .decode(iv_b64)
        .context("Failed to base64-decode v1 IV")?;
    let salt = BASE64
        .decode(salt_b64)
        .context("Failed to base64-decode v1 salt")?;

    let key_size_bytes = key_size_bits / 8;
    let mut derived_key = vec![0u8; key_size_bytes];
    pbkdf2_hmac::<Sha256>(
        passphrase_str.as_bytes(),
        &salt,
        iterations,
        &mut derived_key,
    );

    let ciphertext = BASE64
        .decode(ct_b64)
        .context("Failed to base64-decode v1 ciphertext")?;

    // v1 uses empty AAD
    let plaintext_bytes = aes_gcm_decrypt(&derived_key, &iv, &ciphertext, b"")
        .map_err(|e| anyhow!("v1 AES-GCM decryption failed: {}", e))?;

    // v1 compression is rawdeflate (not zlib)
    let compression_type = if compression.contains("deflate") || compression.is_empty() {
        "rawdeflate"
    } else {
        compression
    };

    decompress(&plaintext_bytes, compression_type)
}

/// Dispatch AES-256-GCM decryption based on the actual IV length.
/// PrivateBin v2 generates 16-byte IVs; some older instances may use 12-byte.
fn aes_gcm_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    match iv.len() {
        12 => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| anyhow!("Bad key length for AES-256: {:?}", e))?;
            let nonce = GenericArray::<u8, U12>::from_slice(iv);
            cipher
                .decrypt(
                    nonce,
                    Payload {
                        msg: ciphertext,
                        aad,
                    },
                )
                .map_err(|_| anyhow!("GCM authentication tag mismatch"))
        }
        16 => {
            type Aes256GcmU16 = AesGcm<Aes256, U16>;
            let cipher = Aes256GcmU16::new_from_slice(key)
                .map_err(|e| anyhow!("Bad key length for AES-256: {:?}", e))?;
            let nonce = GenericArray::<u8, U16>::from_slice(iv);
            cipher
                .decrypt(
                    nonce,
                    Payload {
                        msg: ciphertext,
                        aad,
                    },
                )
                .map_err(|_| anyhow!("GCM authentication tag mismatch"))
        }
        n => bail!("Unexpected IV length: {} bytes (expected 12 or 16)", n),
    }
}

/// Decompress bytes according to the PrivateBin compression type string.
///
/// Despite being labelled "zlib" in v2 pastes, PrivateBin stores raw DEFLATE
/// data (no zlib wrapper/header). Both "zlib" and "rawdeflate" therefore use
/// `DeflateDecoder`. This matches the reference implementation (pbcli/miniz_oxide).
fn decompress(data: &[u8], compression: &str) -> Result<String> {
    match compression {
        "zlib" | "rawdeflate" | "" => {
            let mut decoder = DeflateDecoder::new(data);
            let mut out = String::new();
            decoder
                .read_to_string(&mut out)
                .context("Failed to deflate-decompress paste")?;
            Ok(out)
        }
        "none" => String::from_utf8(data.to_vec()).context("Plaintext is not valid UTF-8"),
        other => bail!("Unknown compression type: '{}'", other),
    }
}
