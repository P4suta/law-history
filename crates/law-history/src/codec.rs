use std::io::{self, Read};

pub const MAX_OBJECT_BYTES: usize = 256 * 1024 * 1024;
const COMPRESSION_LEVEL: i32 = 3;
const MAX_WINDOW_LOG: u32 = 23;

pub fn encode(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() > MAX_OBJECT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Object exceeds the 256 MiB limit",
        ));
    }
    zstd::bulk::compress(bytes, COMPRESSION_LEVEL)
}

pub fn decode(bytes: &[u8], maximum: usize) -> io::Result<Vec<u8>> {
    let maximum = maximum.min(MAX_OBJECT_BYTES);
    let mut decoder = zstd::stream::read::Decoder::new(bytes)?;
    decoder.window_log_max(MAX_WINDOW_LOG)?;
    let mut output = Vec::new();
    decoder.take(maximum as u64 + 1).read_to_end(&mut output)?;
    if output.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Decoded object exceeds its size limit",
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_is_inclusive_for_encoding_and_decoding() {
        let limit = 268_435_456;
        let mut bytes = vec![0; limit];
        let encoded = encode(&bytes).unwrap();
        let decoded = decode(&encoded, usize::MAX).unwrap();
        assert_eq!(decoded.len(), bytes.len());
        assert!(
            decoded == bytes,
            "The decoded boundary object must preserve every byte"
        );
        drop(decoded);
        assert!(decode(&encoded, limit - 1).is_err());
        bytes.push(0);
        assert!(encode(&bytes).is_err());
        let encoded = zstd::bulk::compress(&bytes, 1).unwrap();
        assert!(decode(&encoded, usize::MAX).is_err());
    }

    #[test]
    fn zero_limit_accepts_only_an_empty_decoded_object() {
        assert_eq!(decode(&encode(b"").unwrap(), 0).unwrap(), b"");
        assert!(decode(&encode(b"x").unwrap(), 0).is_err());
        assert!(decode(b"", 0).is_err());
        assert!(decode(b"not a compressed object", usize::MAX).is_err());
    }
}
