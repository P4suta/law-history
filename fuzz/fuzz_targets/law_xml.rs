#![no_main]

fn expected_base<'a>(node: &'a law_history::Element, ancestors: &mut Vec<&'a str>) -> String {
    ancestors.push(&node.tag);
    let result = if ancestors.windows(2).any(|pair| pair == ["Ruby", "Rt"]) {
        String::new()
    } else {
        node.content
            .iter()
            .map(|part| match part {
                law_history::Content::Text(text) => text.clone(),
                law_history::Content::Element(child) => expected_base(child, ancestors),
            })
            .collect()
    };
    ancestors.pop();
    result
}

libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    if let Ok(text) = std::str::from_utf8(input) {
        if let Ok(title) = law_history::xml::title_text(text) {
            let expected = if text.contains('<') {
                let tree =
                    law_history::xml::parse_xml(&format!("<LawTitle>{text}</LawTitle>")).unwrap();
                expected_base(&tree, &mut Vec::new())
            } else {
                text.to_owned()
            };
            assert_eq!(title, expected);
        }
        let Ok(tree) = law_history::xml::parse_xml(text) else {
            return;
        };
        assert_eq!(
            law_history::xml::base_text(&tree),
            expected_base(&tree, &mut Vec::new())
        );
        let encoded = serde_json::to_vec(&tree).expect("serializable element");
        let decoded: law_history::Element =
            serde_json::from_slice(&encoded).expect("valid element JSON");
        assert_eq!(tree, decoded);
        assert!(law_history::xml::validate_tree(&tree).is_ok());
        fn count_elements(node: &law_history::Element) -> usize {
            1 + node.children().map(count_elements).sum::<usize>()
        }
        let indexed = law_history::xml::index(&tree);
        let flattened = law_history::xml::flatten(&tree);
        assert_eq!(indexed.len(), count_elements(&tree));
        assert_eq!(flattened.len(), indexed.len());
        assert!(
            indexed
                .iter()
                .all(|(path, node)| flattened.get(path) == Some(*node)),
            "Every indexed position must preserve its original element"
        );
    }
});
