const SHA256_HEX_BYTES: usize = 64;

pub fn valid_digest(bytes: &[u8]) -> bool {
    bytes.len() == SHA256_HEX_BYTES
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn is_immutable(reference: &str) -> bool {
    match reference.split_once("@sha256:") {
        Some((name, digest)) => {
            !name.is_empty() && !name.contains('@') && valid_digest(digest.as_bytes())
        }
        None => reference
            .strip_prefix("sha256:")
            .is_some_and(|digest| valid_digest(digest.as_bytes())),
    }
}
