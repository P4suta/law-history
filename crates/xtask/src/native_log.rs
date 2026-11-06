pub const MAXIMUM_BYTES: usize = 8_388_608;

pub const fn allows(byte_count: usize) -> bool {
    byte_count <= MAXIMUM_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_log_lengths_include_eight_mebibytes_and_refuse_excess() {
        assert_eq!(MAXIMUM_BYTES, 8_388_608);
        for (length, expected) in [
            (0, true),
            (MAXIMUM_BYTES - 1, true),
            (MAXIMUM_BYTES, true),
            (MAXIMUM_BYTES + 1, false),
            (usize::MAX, false),
        ] {
            assert_eq!(allows(length), expected);
        }
    }
}
