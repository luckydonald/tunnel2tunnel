use base64::{engine::general_purpose, Engine};
use sha2::{Digest, Sha256};
use crate::error::CoreError;

/// Parse one line in authorized_keys format: `<algo> <b64_key_data> [comment]`
/// Returns `(algorithm, key_data_b64, comment)`.
pub fn parse_authorized_keys_line(line: &str) -> Result<(String, String, Option<String>), CoreError> {
    let line = line.trim();
    let mut parts = line.splitn(3, ' ');
    let algorithm = parts.next()
        .filter(|s| !s.is_empty())
        .ok_or(CoreError::InvalidKeyFormat)?
        .to_string();
    let key_data = parts.next()
        .filter(|s| !s.is_empty())
        .ok_or(CoreError::InvalidKeyFormat)?
        .to_string();
    let comment = parts.next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok((algorithm, key_data, comment))
}

/// Compute the SSH SHA-256 fingerprint from the base64 key blob.
/// Result format: `SHA256:<base64_no_pad_sha256>`
pub fn compute_fingerprint(key_data_b64: &str) -> Result<String, CoreError> {
    let raw = general_purpose::STANDARD
        .decode(key_data_b64.trim())
        .map_err(|_| CoreError::InvalidKeyData)?;
    let hash = Sha256::digest(&raw);
    let b64 = general_purpose::STANDARD_NO_PAD.encode(hash);
    Ok(format!("SHA256:{b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_authorized_keys_line_with_comment() {
        let line = "ssh-ed25519 AAAA... user@host";
        let (algo, key, comment) = parse_authorized_keys_line(line).unwrap();
        assert_eq!(algo, "ssh-ed25519");
        assert_eq!(key, "AAAA...");
        assert_eq!(comment, Some("user@host".into()));
    }

    #[test]
    fn parse_authorized_keys_line_no_comment() {
        let line = "ssh-rsa BBBB==";
        let (algo, key, comment) = parse_authorized_keys_line(line).unwrap();
        assert_eq!(algo, "ssh-rsa");
        assert_eq!(key, "BBBB==");
        assert!(comment.is_none());
    }

    #[test]
    fn fingerprint_stable() {
        let key_b64 = "dGVzdF9zc2hfa2V5X2J5dGVzX2Zvcl90ZXN0";
        // same input → same fingerprint
        let fp1 = compute_fingerprint(key_b64).unwrap();
        let fp2 = compute_fingerprint(key_b64).unwrap();
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("SHA256:"));
    }

    #[test]
    fn fingerprint_rejects_invalid_base64() {
        assert!(compute_fingerprint("not!valid!!base64").is_err());
    }
}
