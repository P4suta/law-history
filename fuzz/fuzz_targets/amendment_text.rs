#![no_main]

use law_history::{Confidence, Provenance, amendment, xml};

libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    if let Ok(text) = std::str::from_utf8(input) {
        let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><ArticleTitle>第一条</ArticleTitle><Paragraph Num=\"1\"><Sentence>甲乙丙</Sentence><Sentence><QuoteStruct><Article Num=\"1\"><Sentence>Quoted article</Sentence></Article></QuoteStruct></Sentence></Paragraph></Article><Article Num=\"1_2_3\"><Paragraph Num=\"1\"><Item Num=\"1_2_3\"><Sentence>甲乙丙</Sentence></Item></Paragraph></Article></MainProvision></LawBody></Law>").unwrap();
        let source = Provenance {
            url: "urn:law-history:synthetic:amendment".into(),
            raw_sha256: "0".repeat(64),
            media_type: "text/plain".into(),
            attribution: "Synthetic parser fixture".into(),
            terms_url: "urn:law-history:synthetic:terms".into(),
            confidence: Confidence::Synthetic,
            locator: "input".into(),
        };
        let original = tree.clone();
        let parsed = amendment::parse_document(text, "DEMO_ACT", &tree, &source);
        let encoded = serde_json::to_vec(&parsed).unwrap();
        let decoded: amendment::ParsedAmendment = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        if !parsed.diagnostics.is_empty() {
            assert!(parsed.operations.is_empty());
        }
        for operation in &parsed.operations {
            assert_eq!(operation.operation.law_id(), "DEMO_ACT");
            if let Ok(changed) = amendment::apply_operation(&tree, operation) {
                assert!(xml::validate_tree(&changed).is_ok());
            }
        }
        assert_eq!(tree, original);
    }
});
