#![no_main]

use law_history::{Day, LegalDate};

libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    if let Ok(text) = std::str::from_utf8(input)
        && let Ok(day) = text.parse::<Day>()
    {
        assert_eq!(day.to_string(), text);
        assert_eq!(day.to_string().parse::<Day>(), Ok(day));
    }
    if let Ok(day) = serde_json::from_slice::<Day>(input) {
        let encoded = serde_json::to_vec(&day).expect("serializable calendar day");
        assert_eq!(serde_json::from_slice::<Day>(&encoded).unwrap(), day);
    }
    if let Ok(date) = serde_json::from_slice::<LegalDate>(input) {
        let encoded = serde_json::to_vec(&date).expect("serializable legal date");
        assert_eq!(serde_json::from_slice::<LegalDate>(&encoded).unwrap(), date);
    }
});
