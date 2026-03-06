use anyhow::Result;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose};

type HmacSha256 = Hmac<Sha256>;

/// Build the L2 HMAC headers required by the Polymarket CLOB API.
/// The CLOB uses HMAC-SHA256 over: timestamp + method + path + body
pub fn build_hmac_headers(
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
    method: &str,
    path: &str,
    body: &str,
) -> Result<Vec<(String, String)>> {
    let timestamp = chrono::Utc::now().timestamp().to_string();

    let message = format!("{}{}{}{}", timestamp, method.to_uppercase(), path, body);

    let secret_bytes = general_purpose::STANDARD.decode(api_secret)?;
    let mut mac = HmacSha256::new_from_slice(&secret_bytes)?;
    mac.update(message.as_bytes());
    let signature = general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    Ok(vec![
        ("POLY_HMAC_KEY".to_string(), api_key.to_string()),
        ("POLY_HMAC_SIGNATURE".to_string(), signature),
        ("POLY_HMAC_TIMESTAMP".to_string(), timestamp),
        ("POLY_HMAC_PASSPHRASE".to_string(), api_passphrase.to_string()),
    ])
}
