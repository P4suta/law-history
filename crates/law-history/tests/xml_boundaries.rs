use law_history::{model::*, xml};
use std::collections::BTreeMap;

#[test]
fn title_attributes_check_schema_positions_names_and_values_independently() {
    use std::collections::BTreeSet;
    let schema =
        roxmltree::Document::parse(include_str!("../../../fixtures/egov-law-schema.xsd")).unwrap();
    let styles = BTreeSet::from(["dotted", "double", "none", "solid"]);
    let seed = include_str!("../../../fuzz/corpus/law_xml/inline-attributes.xml").trim_end();
    assert_eq!(xml::title_text(seed).unwrap(), "甲乙");
    for tag in ["Line", "Ruby", "Rt", "Sup", "Sub"] {
        let declaration = schema
            .root_element()
            .children()
            .find(|node| {
                node.is_element()
                    && node.tag_name().name() == "element"
                    && node.attribute("name") == Some(tag)
            })
            .unwrap();
        let attributes: BTreeMap<_, BTreeSet<_>> = declaration
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "attribute")
            .map(|node| {
                (
                    node.attribute("name").unwrap(),
                    node.descendants()
                        .filter(|node| node.is_element() && node.tag_name().name() == "enumeration")
                        .map(|node| node.attribute("value").unwrap())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            attributes,
            if tag == "Line" {
                BTreeMap::from([("Style", styles.clone())])
            } else {
                BTreeMap::new()
            },
            "Unclassified attribute declaration for {tag}"
        );
        for name in ["Style", "Other", "style"] {
            for value in styles.iter().copied().chain(["unknown", "", "　solid　"]) {
                for (prefix, suffix) in [("", ""), (" \t", "\r\n ")] {
                    let value = format!("{prefix}{value}{suffix}");
                    let normalized = value
                        .split([' ', '\t', '\r', '\n'])
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let expected = attributes
                        .get(name)
                        .is_some_and(|values| values.contains(normalized.as_str()));
                    let input = match tag {
                        "Ruby" => format!("<Ruby {name}=\"{value}\">甲<Rt>第二条</Rt></Ruby>"),
                        "Rt" => format!("<Ruby>甲<Rt {name}=\"{value}\">第二条</Rt></Ruby>"),
                        _ => format!("<{tag} {name}=\"{value}\">甲</{tag}>"),
                    };
                    let result = xml::title_text(&input);
                    assert_eq!(result.is_ok(), expected, "{input}: {result:?}");
                    if expected {
                        assert_eq!(result.unwrap(), "甲");
                    }
                }
            }
        }
    }
}

#[test]
fn every_pinned_title_schema_child_has_a_distinguishing_projection_probe() {
    let bytes = include_bytes!("../../../fixtures/egov-law-schema.xsd");
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../fixtures/egov-ruby-title.json")).unwrap();
    assert_eq!(law_history::store::sha256(bytes), fixture["schema_sha256"]);
    let schema = roxmltree::Document::parse(std::str::from_utf8(bytes).unwrap()).unwrap();
    let mut checked = std::collections::BTreeSet::new();
    for parent in ["LawTitle", "Line", "Ruby"] {
        let declaration = schema
            .root_element()
            .children()
            .find(|node| {
                node.is_element()
                    && node.tag_name().name() == "element"
                    && node.attribute("name") == Some(parent)
            })
            .unwrap();
        let children: std::collections::BTreeSet<_> = declaration
            .descendants()
            .filter_map(|node| node.attribute("ref"))
            .collect();
        assert!(!children.is_empty());
        for child in children {
            let sample = match child {
                "Line" => "<Line>甲</Line>",
                "Ruby" => "<Ruby>甲<Rt>第二条</Rt></Ruby>",
                "Sup" => "<Sup>甲</Sup>",
                "Sub" => "<Sub>甲</Sub>",
                "QuoteStruct" => "<QuoteStruct><Sentence>甲</Sentence></QuoteStruct>",
                "ArithFormula" => "<ArithFormula><Sentence>甲</Sentence></ArithFormula>",
                "Rt" => "<Rt>第二条</Rt>",
                _ => panic!("Unclassified title schema child: {parent}/{child}"),
            };
            let input = match parent {
                "LawTitle" => sample.to_owned(),
                "Line" => format!("<Line>{sample}</Line>"),
                "Ruby" => format!("<Ruby>甲{sample}</Ruby>"),
                _ => unreachable!(),
            };
            assert_eq!(xml::title_text(&input).unwrap(), "甲", "{parent}/{child}");
            checked.insert((parent.to_owned(), child.to_owned()));
        }
    }
    let expected: std::collections::BTreeSet<_> = [
        ("LawTitle", "Line"),
        ("LawTitle", "Ruby"),
        ("LawTitle", "Sup"),
        ("LawTitle", "Sub"),
        ("Line", "QuoteStruct"),
        ("Line", "ArithFormula"),
        ("Line", "Ruby"),
        ("Line", "Sup"),
        ("Line", "Sub"),
        ("Ruby", "Rt"),
    ]
    .into_iter()
    .map(|(parent, child)| (parent.to_owned(), child.to_owned()))
    .collect();
    assert_eq!(checked, expected);
}

#[test]
fn base_text_matches_an_independent_reading_ancestor_path_oracle() {
    let names = [
        "LawTitle", "Ruby", "Rt", "Line", "Sub", "Sup", "Sentence", "Other",
    ];
    for length in 0..=4u32 {
        for mut encoded in 0..names.len().pow(length) {
            let path: Vec<_> = (0..length)
                .map(|_| {
                    let name = names[encoded % names.len()];
                    encoded /= names.len();
                    name
                })
                .collect();
            let excluded = path.windows(2).any(|pair| pair == ["Ruby", "Rt"]);
            let mut root = element("Leaf");
            root.content.push(Content::Text("正文 甲&乙".into()));
            for name in path.iter().rev() {
                let mut parent = element(name);
                parent.content.push(Content::Element(root));
                root = parent;
            }
            let original = root.clone();
            assert_eq!(
                xml::base_text(&root),
                if excluded { "" } else { "正文 甲&乙" },
                "{path:?}"
            );
            assert_eq!(root, original);
            assert_eq!(root.text(), "正文 甲&乙");
        }
    }
}

#[test]
fn inline_titles_preserve_base_spacing_entities_and_supported_roles_and_refuse_unknown_syntax() {
    let seed = include_str!("../../../fuzz/corpus/law_xml/ruby.xml").trim_end();
    assert_eq!(xml::title_text(seed).unwrap(), "甚");
    assert_eq!(xml::base_text(&xml::parse_xml(seed).unwrap()), "甚");
    for (source, expected) in [
        ("甲 & 乙", "甲 & 乙"),
        ("甲<Line> </Line>乙", "甲 乙"),
        ("<Ruby>甲<Rt>第二条</Rt>乙<Rt>第三条</Rt></Ruby>", "甲乙"),
        (
            "<Line Style=\"dotted\">甲<Ruby>乙<Rt>第二条</Rt></Ruby></Line>",
            "甲乙",
        ),
        ("甲<Sup> </Sup><Sub>　</Sub>乙", "甲 　乙"),
        ("<Ruby>&lt;&amp;&gt;<Rt>第二条</Rt></Ruby>", "<&>"),
        ("<Ruby><Rt>第二条</Rt></Ruby>", ""),
        (
            "<Line><QuoteStruct><Sentence>甲<Ruby>乙<Rt>第二条</Rt></Ruby></Sentence></QuoteStruct></Line>",
            "甲乙",
        ),
        (
            "<Line><ArithFormula><Formula>甲</Formula></ArithFormula></Line>",
            "甲",
        ),
    ] {
        assert_eq!(xml::title_text(source).unwrap(), expected, "{source}");
    }
    for source in [
        "<Ruby>甲<Rt>こう</Ruby>",
        "<Unknown>甲</Unknown>",
        "<Rt>甲</Rt>",
        "<Ruby><Sub>甲</Sub></Ruby>",
        "<Ruby>甲<Rt><Ruby>乙</Ruby></Rt></Ruby>",
        "<Sup><Ruby>甲</Ruby></Sup>",
        "<Ruby Other=\"value\">甲</Ruby>",
        "<Line Other=\"value\">甲</Line>",
        "<Line Style=\"unknown\">甲</Line>",
        "<LawTitle>甲</LawTitle>",
        "<Ruby xmlns=\"unknown\">甲</Ruby>",
        "<!DOCTYPE x><Ruby>甲</Ruby>",
        "<Ruby>&unknown;</Ruby>",
    ] {
        assert!(xml::title_text(source).is_err(), "{source}");
    }
}

fn element(tag: &str) -> Element {
    Element {
        tag: tag.into(),
        attributes: BTreeMap::new(),
        content: Vec::new(),
    }
}

#[test]
fn tree_and_parser_limits_include_the_exact_boundary() {
    let mut tree = element("Law");
    tree.content = vec![Content::Element(element("Node")); 499_999];
    assert!(xml::validate_tree(&tree).is_ok());
    tree.content.push(Content::Element(element("Node")));
    assert!(xml::validate_tree(&tree).is_err());
    drop(tree);
    for depth in [128, 129, 130] {
        let input = format!(
            "<Law>{}text{}</Law>",
            "<Node>".repeat(depth),
            "</Node>".repeat(depth)
        );
        let parsed = xml::parse_xml(&input);
        if depth == 128 {
            assert!(parsed.is_ok());
        } else {
            assert_eq!(parsed.unwrap_err(), "XML nesting exceeds 128 elements");
        }
        let mut tree = element("Leaf");
        for _ in 0..depth {
            let mut parent = element("Parent");
            parent.content.push(Content::Element(tree));
            tree = parent;
        }
        assert_eq!(xml::validate_tree(&tree).is_ok(), depth == 128);
    }
    let mut input = format!(
        "<Law>{}</Law>",
        "x".repeat(33_554_432 - "<Law></Law>".len())
    );
    assert_eq!(input.len(), 33_554_432);
    assert!(xml::parse_xml(&input).is_ok());
    input.push(' ');
    assert!(xml::parse_xml(&input).is_err());
    input.push(' ');
    assert!(xml::parse_xml(&input).is_err());
}

#[test]
fn parser_node_budget_counts_document_and_text_nodes_and_refuses_dtds() {
    let boundary = format!("<Law>{}</Law>", "<Node/>".repeat(499_998));
    assert!(xml::parse_xml(&boundary).is_ok());
    let excess = format!("<Law>{}</Law>", "<Node/>".repeat(499_999));
    assert!(xml::parse_xml(&excess).is_err());
    for input in [
        "<!DOCTYPE Law><Law/>",
        "<!DOCTYPE Law [<!ENTITY term 'expanded'>]><Law>&term;</Law>",
        "<!DOCTYPE Law SYSTEM 'file:///unavailable'><Law/>",
    ] {
        assert!(xml::parse_xml(input).is_err());
    }
}

#[test]
fn provision_selection_and_ordinal_insertion_preserve_supported_structure() {
    let tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>A</Sentence></Item></Paragraph></Article></MainProvision><SupplProvision/><AppdxTable Num=\"0\"><TableStruct/></AppdxTable></LawBody></Law>").unwrap();
    let provisions = xml::provisions(&tree);
    for tag in [
        "Article",
        "Paragraph",
        "Item",
        "Sentence",
        "SupplProvision",
        "AppdxTable",
        "TableStruct",
    ] {
        assert!(provisions.iter().any(|node| node.element.tag == tag));
    }
    assert!(!provisions.iter().any(|node| matches!(
        node.element.tag.as_str(),
        "Law" | "LawBody" | "MainProvision"
    )));
    let tree = xml::parse_xml("<Law><Sentence>A</Sentence><Sentence>B</Sentence></Law>").unwrap();
    let inserted = element("Added");
    let changed = xml::append(&tree, "/Law", &inserted, Some("/Law/Sentence[index=2]")).unwrap();
    let tags: Vec<_> = changed.children().map(|child| child.tag.as_str()).collect();
    assert_eq!(tags, ["Sentence", "Added", "Sentence"]);
    assert!(xml::append(&tree, "/Law", &inserted, Some("/Law/Sentence[index=3]")).is_err());
    for tag in ["", "0Tag", "bad-name", "bad space", "é"] {
        assert!(xml::validate_tree(&element(tag)).is_err());
    }
    for tag in ["Law", "_Node", "Node2"] {
        assert!(xml::validate_tree(&element(tag)).is_ok());
    }
}

#[test]
fn repeated_appendix_items_and_path_delimiters_preserve_distinct_original_positions() {
    let tree = xml::parse_xml(include_str!(
        "../../../fixtures/repeated-appendix-items.xml"
    ))
    .unwrap();
    let parent = "/Law/LawBody[index=1]/AppdxTable[num=1]";
    let first = format!("{parent}/Item[index=1]");
    let second = format!("{parent}/Item[index=2]");
    let indexed = xml::index(&tree);
    assert_eq!(indexed[&first].attributes["Num"], "1");
    assert_eq!(indexed[&second].attributes["Num"], "1");
    assert!(indexed[&first].text().contains("First appendix item"));
    assert!(indexed[&second].text().contains("Second appendix item"));
    let changed = xml::replace(&tree, &second, None).unwrap();
    assert!(changed.text().contains("First appendix item"));
    assert!(!changed.text().contains("Second appendix item"));
    let changed = xml::append(&tree, parent, &element("Added"), Some(&second)).unwrap();
    assert_eq!(
        xml::index(&changed)[parent]
            .children()
            .map(|child| child.tag.as_str())
            .collect::<Vec<_>>(),
        ["AppdxTableTitle", "Item", "Added", "Item"]
    );
    for number in ["1/2", "[1]", "]", "0", ""] {
        let source = format!(
            "<Law><Article Num=\"{number}\"><Sentence>First</Sentence></Article><Article Num=\"2\"/></Law>"
        );
        let tree = xml::parse_xml(&source).unwrap();
        assert_eq!(
            xml::index(&tree)["/Law/Article[index=1]"].attributes["Num"],
            number
        );
        assert!(xml::index(&tree).contains_key("/Law/Article[num=2]"));
        assert!(
            xml::replace(&tree, "/Law/Article[index=1]", None)
                .unwrap()
                .text()
                .is_empty()
        );
    }
}

#[test]
fn repeated_numbered_forms_keep_original_numbers_and_distinct_editable_positions() {
    let tree = xml::parse_xml(include_str!(
        "../../../fixtures/repeated-numbered-forms.xml"
    ))
    .unwrap();
    let indexed = xml::index(&tree);
    let flattened = xml::flatten(&tree);
    assert_eq!(indexed.len(), flattened.len());
    for (path, node) in &indexed {
        assert_eq!(*node, &flattened[path]);
    }
    let parent = "/Law/LawBody[index=1]";
    let first = format!("{parent}/AppdxStyle[index=1]");
    let second = format!("{parent}/AppdxStyle[index=2]");
    assert_eq!(flattened[&first].attributes["Num"], "1");
    assert_eq!(flattened[&second].attributes["Num"], "1");
    assert_eq!(
        flattened[&format!("{parent}/AppdxStyle[num=2]")].attributes["Num"],
        "2"
    );
    for ordinal in [1, 2] {
        assert_eq!(
            flattened[&format!("{parent}/AppdxFormat[index={ordinal}]")].attributes["Num"],
            "15"
        );
    }
    let changed = xml::replace(&tree, &second, None).unwrap();
    assert!(changed.text().contains("First style"));
    assert!(!changed.text().contains("Second style"));
    let inserted = element("Inserted");
    let changed = xml::append(&tree, parent, &inserted, Some(&second)).unwrap();
    let children: Vec<_> = xml::flatten(&changed)[parent]
        .children()
        .map(|child| child.tag.clone())
        .collect();
    assert_eq!(
        children,
        [
            "MainProvision",
            "AppdxStyle",
            "Inserted",
            "AppdxStyle",
            "AppdxStyle",
            "AppdxFormat",
            "AppdxFormat"
        ]
    );
    let provisions = xml::provisions(&tree);
    assert_eq!(
        provisions
            .iter()
            .filter(|node| node.element.tag == "AppdxStyle")
            .count(),
        3
    );
    assert_eq!(
        provisions
            .iter()
            .filter(|node| node.element.tag == "AppdxFormat")
            .count(),
        2
    );
    assert!(flattened.contains_key(&format!("{parent}/MainProvision[index=1]/Article[num=1]")));
}

#[test]
fn position_index_matches_independent_number_multiplicities_for_every_small_sibling_sequence() {
    let tags = ["Article", "AppdxStyle", "AppdxFormat"];
    let numbers = [
        None,
        Some(""),
        Some("0"),
        Some("1"),
        Some("2"),
        Some("1/2"),
        Some("[1]"),
        Some("]"),
    ];
    let alphabet = tags.len() * numbers.len();
    for length in 0..=3 {
        for mut encoded in 0..alphabet.pow(length) {
            let mut root = element("Law");
            let mut children = Vec::new();
            for index in 0..length {
                let symbol = encoded % alphabet;
                encoded /= alphabet;
                let mut child = element(tags[symbol / numbers.len()]);
                if let Some(number) = numbers[symbol % numbers.len()] {
                    child.attributes.insert("Num".into(), number.into());
                }
                child.content.push(Content::Text(index.to_string()));
                root.content.push(Content::Element(child.clone()));
                children.push(child);
            }
            let mut expected = BTreeMap::from([("/Law".into(), root.clone())]);
            let mut paths = Vec::new();
            for (offset, child) in children.iter().enumerate() {
                let number = child.attributes.get("Num").filter(|number| {
                    !number.is_empty()
                        && number.as_str() != "0"
                        && !number
                            .chars()
                            .any(|character| ['/', '[', ']'].contains(&character))
                });
                let multiplicity = children
                    .iter()
                    .filter(|other| other.tag == child.tag && other.attributes.get("Num") == number)
                    .count();
                let ordinal = children[..=offset]
                    .iter()
                    .filter(|other| other.tag == child.tag)
                    .count();
                let selector = match number {
                    Some(number) if multiplicity == 1 => format!("num={number}"),
                    _ => format!("index={ordinal}"),
                };
                let path = format!("/Law/{}[{selector}]", child.tag);
                paths.push(path.clone());
                assert!(expected.insert(path, child.clone()).is_none());
            }
            assert!(xml::validate_tree(&root).is_ok());
            assert_eq!(xml::child_paths(&root, "/Law"), paths);
            assert_eq!(xml::flatten(&root), expected);
            assert_eq!(
                xml::index(&root)
                    .into_iter()
                    .map(|(path, node)| (path, node.clone()))
                    .collect::<BTreeMap<_, _>>(),
                expected
            );
        }
    }
}

#[test]
fn article_scope_checks_root_body_ancestors_and_missing_index_entries() {
    for body in [false, true] {
        for tags in [
            vec![],
            vec!["Part"],
            vec!["Chapter"],
            vec!["Section"],
            vec!["Subsection"],
            vec!["Division"],
            vec!["Part", "Chapter", "Section", "Subsection", "Division"],
        ] {
            let opening: String = tags.iter().map(|tag| format!("<{tag}>")).collect();
            let closing: String = tags.iter().rev().map(|tag| format!("</{tag}>")).collect();
            let contents = format!(
                "<MainProvision>{opening}<Article Num=\"1\"/><Paragraph Num=\"1\"/>{closing}</MainProvision>"
            );
            let document = if body {
                format!("<Law><LawBody>{contents}</LawBody></Law>")
            } else {
                format!("<Law>{contents}</Law>")
            };
            let tree = xml::parse_xml(&document).unwrap();
            let mut index = xml::index(&tree);
            let article = index
                .iter()
                .find(|(_, node)| node.tag == "Article")
                .unwrap()
                .0
                .clone();
            let paragraph = index
                .iter()
                .find(|(_, node)| node.tag == "Paragraph")
                .unwrap()
                .0
                .clone();
            assert!(xml::is_main_article(&article, &index));
            assert!(!xml::is_main_article(&paragraph, &index));
            assert!(!xml::is_main_article("/Law/Missing", &index));
            let parent = article.rsplit_once('/').unwrap().0.to_string();
            index.remove(&parent);
            assert!(!xml::is_main_article(&article, &index));
        }
    }
}

#[test]
fn generated_article_contexts_match_an_independent_anchored_path_grammar() {
    let grammar = regex::Regex::new(
        r"^Law/(LawBody/)?MainProvision/((Part|Chapter|Section|Subsection|Division)/)*Article$",
    )
    .unwrap();
    let tags = [
        "Law",
        "LawBody",
        "MainProvision",
        "Part",
        "Chapter",
        "Section",
        "Subsection",
        "Division",
        "Article",
        "QuoteStruct",
        "NewProvision",
        "AmendProvision",
        "SupplProvision",
        "AppdxTable",
        "NotMainProvision",
        "ArticleLike",
    ];
    for length in 0..=3 {
        for encoded in 0..tags.len().pow(length) {
            let mut remaining = encoded;
            let mut ancestors = vec!["Law"];
            for _ in 0..length {
                ancestors.push(tags[remaining % tags.len()]);
                remaining /= tags.len();
            }
            let mut tree = element("Article");
            for tag in ancestors.iter().rev() {
                let mut parent = element(tag);
                parent.content.push(Content::Element(tree));
                tree = parent;
            }
            let index = xml::index(&tree);
            let leaf = index.keys().max_by_key(|path| path.len()).unwrap();
            let document_path = format!("{}/Article", ancestors.join("/"));
            assert_eq!(
                xml::is_main_article(leaf, &index),
                grammar.is_match(&document_path),
                "{document_path}"
            );
        }
    }
}

#[test]
fn invalid_move_locations_return_errors_without_mutating_the_source() {
    let root = xml::parse_xml("<Law><Sentence>A</Sentence><Sentence>B</Sentence></Law>").unwrap();
    for (source, parent, before) in [
        ("/Law", "/Law", None),
        ("/Law/absent", "/Law", None),
        ("/Law/Sentence[index=1]", "/Law/absent", None),
        ("/Law/Sentence[index=1]", "/Law", Some("/Law/absent")),
        ("/Law/Sentence[index=1]", "/Law/Sentence[index=1]", None),
    ] {
        assert!(xml::move_element(&root, source, parent, before).is_err());
        assert_eq!(root.text(), "AB");
    }
}
