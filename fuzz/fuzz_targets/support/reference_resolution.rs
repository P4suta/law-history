use law_history::{model::*, references, xml};
use std::collections::BTreeMap;

const KANJI_DIGITS: [char; 9] = ['一', '二', '三', '四', '五', '六', '七', '八', '九'];
const FULLWIDTH_DIGITS: [char; 9] = ['１', '２', '３', '４', '５', '６', '７', '８', '９'];
const DIGIT_COUNT: usize = KANJI_DIGITS.len();
const REPRESENTATION_COUNT: u8 = 3;
const MAX_BRANCHES: usize = 4;
const STRUCTURE_TAGS: [&str; 5] = ["Part", "Chapter", "Section", "Subsection", "Division"];
const RUBY_REFERENCE_FLAG: u8 = 16;

fn component(byte: u8) -> (String, String) {
    let index = usize::from(byte) % DIGIT_COUNT;
    let canonical = (index + 1).to_string();
    let written = match byte % REPRESENTATION_COUNT {
        0 => canonical.clone(),
        1 => KANJI_DIGITS[index].to_string(),
        _ => FULLWIDTH_DIGITS[index].to_string(),
    };
    (canonical, written)
}

pub fn check(input: &[u8]) {
    let byte = |index: usize| input.get(index).copied().unwrap_or_default();
    let count = usize::from(byte(0)) % MAX_BRANCHES + 2;
    let wrappers = &STRUCTURE_TAGS[..(usize::from(byte(0)) / 8) % (STRUCTURE_TAGS.len() + 1)];
    let article: Vec<_> = (0..count).map(|index| component(byte(index + 1))).collect();
    let item: Vec<_> = (0..count)
        .map(|index| component(byte(index + count + 1)))
        .collect();
    let has_item = byte(0).is_multiple_of(2);
    let malformed = byte(0) & 4 != 0;
    let canonical = |parts: &[(String, String)]| {
        parts
            .iter()
            .map(|part| part.0.as_str())
            .collect::<Vec<_>>()
            .join("_")
    };
    let written = |parts: &[(String, String)]| {
        parts
            .iter()
            .map(|part| part.1.as_str())
            .collect::<Vec<_>>()
            .join("の")
    };
    let article_num = canonical(&article);
    let mut reference = format!("第{}条の{}", article[0].1, written(&article[1..]));
    if malformed {
        reference.push_str("の十百");
    }
    reference.push_str("第一項");
    let item_num = canonical(&item);
    if has_item {
        reference.push_str(&format!("第{}号の{}", item[0].1, written(&item[1..])));
    }
    let mut document = String::from("<Law><LawBody><MainProvision>");
    for tag in wrappers {
        document.push_str(&format!("<{tag}>"));
    }
    for prefix in 1..article.len() {
        document.push_str(&format!("<Article Num=\"{}\"><Paragraph Num=\"1\"><Sentence>Prefix distractor</Sentence></Paragraph></Article>", canonical(&article[..prefix])));
    }
    document.push_str(&format!(
        "<Article Num=\"{article_num}\"><Paragraph Num=\"1\"><Sentence>Target paragraph</Sentence>"
    ));
    if has_item {
        for prefix in 1..item.len() {
            document.push_str(&format!(
                "<Item Num=\"{}\"><Sentence>Prefix item</Sentence></Item>",
                canonical(&item[..prefix])
            ));
        }
        document.push_str(&format!(
            "<Item Num=\"{item_num}\"><Sentence>Target item</Sentence></Item>"
        ));
    }
    document.push_str("</Paragraph></Article>");
    let written_reference = if byte(0) & RUBY_REFERENCE_FLAG == 0 {
        reference.clone()
    } else {
        reference
            .chars()
            .map(|ch| format!("<Ruby>{ch}<Rt>第二条</Rt></Ruby>"))
            .collect()
    };
    document.push_str(&format!("<Article Num=\"999999\"><Sentence>{written_reference}の規定</Sentence><Paragraph Num=\"2\"><Sentence><QuoteStruct><Article Num=\"{article_num}\"><Sentence>Quoted article</Sentence></Article><LawBody><MainProvision><Article Num=\"{article_num}\"><Sentence>Quoted law body</Sentence></Article></MainProvision></LawBody></QuoteStruct></Sentence></Paragraph><AmendProvision><NewProvision><Article Num=\"{article_num}\"><Sentence>Unapplied article</Sentence></Article></NewProvision></AmendProvision></Article>"));
    for tag in wrappers.iter().rev() {
        document.push_str(&format!("</{tag}>"));
    }
    document.push_str("</MainProvision></LawBody></Law>");
    let day = "1970-01-01".parse().unwrap();
    let revision = Revision {
        schema_version: SCHEMA_VERSION,
        law_id: "REFERENCE_FIXTURE".into(),
        revision_id: "REFERENCE_REVISION".into(),
        title: "Synthetic reference fixture".into(),
        promulgated: LegalDate::known(day, "Synthetic date"),
        amendment_promulgated: None,
        effective: LegalDate::known(day, "Synthetic date"),
        repeal: None,
        termination: None,
        sequence: None,
        kind: RevisionKind::Enactment,
        amendment_law_id: None,
        original_enactment: true,
        tree: xml::parse_xml(&document).unwrap(),
        source: Provenance {
            url: "urn:law-history:synthetic:reference".into(),
            raw_sha256: "0".repeat(64),
            media_type: "application/xml".into(),
            attribution: "Synthetic reference fixture".into(),
            terms_url: "urn:law-history:synthetic:terms".into(),
            confidence: Confidence::Synthetic,
            locator: "input".into(),
        },
        metadata_source: None,
    };
    let corpus = BTreeMap::from([(
        "REFERENCE_FIXTURE".into(),
        ("fixture-version".into(), revision),
    )]);
    let edges = references::analyze(&corpus);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].text, reference);
    if malformed {
        assert!(edges[0].to.is_none());
        assert!(!edges[0].diagnostics.is_empty());
    } else {
        let target = edges[0].to.as_ref().unwrap();
        assert_eq!(target.law_id, "REFERENCE_FIXTURE");
        assert_eq!(target.version, "fixture-version");
        let ancestry: String = wrappers
            .iter()
            .map(|tag| format!("/{tag}[index=1]"))
            .collect();
        let mut expected = format!(
            "/Law/LawBody[index=1]/MainProvision[index=1]{ancestry}/Article[num={article_num}]/Paragraph[num=1]"
        );
        if has_item {
            expected.push_str(&format!("/Item[num={item_num}]"));
        }
        assert_eq!(target.path, expected);
    }
}
