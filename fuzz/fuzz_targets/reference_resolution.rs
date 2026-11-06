#![no_main]

#[path = "support/reference_resolution.rs"]
mod reference_resolution;

libfuzzer_sys::fuzz_target!(|input: &[u8]| reference_resolution::check(input));
