#![no_main]

use law_history::codec;

#[path = "../../crates/xtask/src/image_reference.rs"]
mod image_reference;

const MAX_DECODED_BYTES: usize = 1024 * 1024;

libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    if let Ok(reference) = std::str::from_utf8(input) {
        let parts: Vec<_> = reference.split('@').collect();
        let identity = match parts.as_slice() {
            [identity] => Some(*identity),
            [name, identity] if !name.is_empty() => Some(*identity),
            _ => None,
        };
        let expected = identity
            .and_then(|identity| identity.split_once(':'))
            .is_some_and(|(algorithm, digest)| {
                algorithm == "sha256"
                    && digest.chars().count() == 64
                    && digest
                        .chars()
                        .all(|digit| "0123456789abcdef".contains(digit))
            });
        assert_eq!(image_reference::is_immutable(reference), expected);
    }
    let encoded = codec::encode(input).unwrap();
    assert_eq!(codec::decode(&encoded, MAX_DECODED_BYTES).unwrap(), input);
    if let Ok(decoded) = codec::decode(input, MAX_DECODED_BYTES) {
        assert!(decoded.len() <= MAX_DECODED_BYTES);
    }
});
