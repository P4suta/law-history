use law_history::{
    Store, ingest,
    model::*,
    references::{self, Position, Reference, ReferenceKind},
    xml,
};
use std::collections::{BTreeMap, BTreeSet};

fn position(index: usize) -> Position {
    Position {
        law_id: "LAW".into(),
        version: "version".into(),
        path: format!("/Law/Article[num={index}]"),
    }
}
fn edge(from: Position, to: Option<Position>) -> Reference {
    Reference {
        from,
        to,
        text: "reference".into(),
        kind: ReferenceKind::Provision,
        confidence: Confidence::Synthetic,
        diagnostics: Vec::new(),
    }
}

#[test]
fn impact_matches_an_independent_closure_for_every_three_node_graph_and_root_set() {
    for graph in 0u16..512 {
        let mut edges = Vec::new();
        for from in 0..3 {
            for to in 0..3 {
                if graph & (1 << (from * 3 + to)) != 0 {
                    edges.push(edge(position(from), Some(position(to))));
                }
            }
        }
        edges.push(edge(position(0), None));
        for roots in 1u8..8 {
            let direct: Vec<_> = (0..3)
                .filter(|index| roots & (1 << index) != 0)
                .map(position)
                .collect();
            let mut reached = roots;
            for _ in 0..3 {
                for from in 0..3 {
                    for to in 0..3 {
                        if graph & (1 << (from * 3 + to)) != 0 && reached & (1 << to) != 0 {
                            reached |= 1 << from;
                        }
                    }
                }
            }
            let result = references::impact(&edges, direct.clone());
            assert_eq!(result.direct, direct);
            let expected: BTreeSet<_> = (0..3)
                .filter(|index| reached & (1 << index) != 0 && roots & (1 << index) == 0)
                .map(position)
                .collect();
            assert_eq!(
                result
                    .review
                    .iter()
                    .map(|path| path.position.clone())
                    .collect::<BTreeSet<_>>(),
                expected
            );
            assert_eq!(result.review.len(), expected.len());
            assert_eq!(result.broken.len(), 1);
            for route in &result.review {
                assert!(direct.contains(route.via[0].to.as_ref().unwrap()));
                for pair in route.via.windows(2) {
                    assert_eq!(pair[1].to.as_ref(), Some(&pair[0].from));
                }
                assert_eq!(route.via.last().unwrap().from, route.position);
            }
        }
    }
}

#[test]
fn impact_intersections_require_the_same_law_version_and_whole_path_segments() {
    let changed = position(1);
    for (target, reaches) in [
        (changed.clone(), true),
        (
            Position {
                path: "/Law".into(),
                ..changed.clone()
            },
            true,
        ),
        (
            Position {
                path: format!("{}/Paragraph[num=1]", changed.path),
                ..changed.clone()
            },
            true,
        ),
        (position(10), false),
        (
            Position {
                law_id: "OTHER".into(),
                ..changed.clone()
            },
            false,
        ),
        (
            Position {
                version: "other".into(),
                ..changed.clone()
            },
            false,
        ),
    ] {
        let result = references::impact(&[edge(position(2), Some(target))], vec![changed.clone()]);
        assert_eq!(result.review.len(), usize::from(reaches));
    }
}

fn revision() -> Revision {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    ingest::import_bundle(&store, include_bytes!("../../../fixtures/demo.json")).unwrap();
    store.history("DEMO_ACT", None).unwrap().remove(0).revision
}

#[test]
fn ruby_annotations_never_supply_reference_tokens_or_break_external_base_titles() {
    for reading in ["じん", "第二条", "甲法第三条", ""] {
        let mut source = revision();
        source.law_id = "ORIGIN".into();
        source.title = "参照元法".into();
        source.tree = xml::parse_xml(&format!(
            "<Law><MainProvision><Article Num=\"1\"><Sentence>激<Ruby>甚<Rt>{reading}</Rt></Ruby>災害法第<Ruby>一<Rt>だいいち</Rt></Ruby>条</Sentence></Article></MainProvision></Law>"
        )).unwrap();
        let mut target = revision();
        target.law_id = "TARGET".into();
        target.title = "激甚災害法".into();
        target.tree = xml::parse_xml(
            "<Law><MainProvision><Article Num=\"1\"/><Article Num=\"2\"/><Article Num=\"3\"/></MainProvision></Law>"
        ).unwrap();
        let edges = references::analyze(&BTreeMap::from([
            ("ORIGIN".into(), ("origin-version".into(), source)),
            ("TARGET".into(), ("target-version".into(), target)),
        ]));
        assert_eq!(edges.len(), 1, "{reading:?}: {edges:?}");
        let to = edges[0].to.as_ref().unwrap();
        assert_eq!(to.law_id, "TARGET");
        assert!(to.path.ends_with("Article[num=1]"));
        assert_eq!(edges[0].text, "第一条");
    }
}

#[test]
fn reference_locators_keep_paragraph_item_branch_and_external_titles_distinct() {
    let mut revision = revision();
    revision.law_id = "LAW".into();
    revision.title = "対象法".into();
    revision.tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>第二条第一項第一号及び第一条の二</Sentence></Item></Paragraph></Article><Article Num=\"1_2\"/><Article Num=\"2\"><Paragraph Num=\"1\"><Item Num=\"1\"><Sentence>対象</Sentence></Item><Item Num=\"2\"/></Paragraph><Paragraph Num=\"2\"><Item Num=\"1\"/></Paragraph></Article></MainProvision><SupplProvision><Article Num=\"2\"/></SupplProvision></LawBody></Law>").unwrap();
    let corpus = BTreeMap::from([("LAW".into(), ("version".into(), revision.clone()))]);
    let edges = references::analyze(&corpus);
    let target = edges
        .iter()
        .find(|edge| edge.text == "第二条第一項第一号")
        .unwrap()
        .to
        .as_ref()
        .unwrap();
    assert_eq!(
        target.path,
        "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=2]/Paragraph[num=1]/Item[num=1]"
    );
    assert!(edges.iter().any(|edge| {
        edge.to
            .as_ref()
            .is_some_and(|target| target.path.ends_with("Article[num=1_2]"))
    }));
    for (text, kind) in [
        ("用語とは、", ReferenceKind::Definition),
        ("用語をいう。", ReferenceKind::Definition),
        ("政令で定める。", ReferenceKind::Delegation),
        ("省令で定める。", ReferenceKind::Delegation),
        ("規則で定める。", ReferenceKind::Delegation),
    ] {
        revision.tree = xml::parse_xml(&format!("<Law><Sentence>{text}</Sentence></Law>")).unwrap();
        assert!(
            references::analyze(&BTreeMap::from([(
                "LAW".into(),
                ("version".into(), revision.clone())
            )]))
            .iter()
            .any(|edge| edge.kind == kind)
        );
    }
    let mut target = revision.clone();
    target.title = "対象法".into();
    target.tree =
        xml::parse_xml("<Law><MainProvision><Article Num=\"1\"/></MainProvision></Law>").unwrap();
    for ambiguous in [false, true] {
        let mut source = target.clone();
        source.title = "出典法".into();
        source.tree = xml::parse_xml(if ambiguous { "<Law><MainProvision><Article Num=\"1\"><Sentence>対象法第一条</Sentence></Article></MainProvision></Law>" } else { "<Law><MainProvision><Article Num=\"1\"><Sentence>未収録法第一条</Sentence></Article></MainProvision></Law>" }).unwrap();
        let mut corpus = BTreeMap::from([
            ("SOURCE".into(), ("source".into(), source)),
            ("TARGET".into(), ("target".into(), target.clone())),
        ]);
        if ambiguous {
            corpus.insert("DUPLICATE".into(), ("duplicate".into(), target.clone()));
        }
        let edges = references::analyze(&corpus);
        let reference = edges
            .iter()
            .find(|edge| edge.from.law_id == "SOURCE")
            .unwrap();
        assert!(reference.to.is_none());
        assert!(!reference.diagnostics.is_empty());
    }
}

#[test]
fn references_require_complete_branch_chains_and_valid_optional_components() {
    let mut revision = revision();
    revision.law_id = "LAW".into();
    for (text, suffix, matched) in [
        (
            "第一条の二の三",
            Some("Article[num=1_2_3]"),
            "第一条の二の三",
        ),
        (
            "第一条の二の三の規定",
            Some("Article[num=1_2_3]"),
            "第一条の二の三",
        ),
        (
            "第一条第一項第二号の三の四",
            Some("Article[num=1]/Paragraph[num=1]/Item[num=2_3_4]"),
            "第一条第一項第二号の三の四",
        ),
        (
            "第１条の２の３",
            Some("Article[num=1_2_3]"),
            "第１条の２の３",
        ),
        ("第一条第十百項", None, "第一条第十百項"),
        ("第一条第一項第十百号", None, "第一条第一項第十百号"),
        ("第十百条", None, "第十百条"),
        ("第一条の十百", None, "第一条の十百"),
        ("第一条第一項第二号の十百", None, "第一条第一項第二号の十百"),
    ] {
        revision.tree = xml::parse_xml(&format!("<Law><MainProvision><Article Num=\"1\"><Sentence>{text}</Sentence><Paragraph Num=\"1\"><Item Num=\"2\"/><Item Num=\"2_3\"/><Item Num=\"2_3_4\"/></Paragraph></Article><Article Num=\"1_2\"/><Article Num=\"1_2_3\"/><Article/></MainProvision></Law>")).unwrap();
        let edges = references::analyze(&BTreeMap::from([(
            "LAW".into(),
            ("version".into(), revision.clone()),
        )]));
        assert_eq!(edges.len(), 1, "{text}");
        let edge = &edges[0];
        assert_eq!(edge.text, matched);
        match suffix {
            Some(suffix) => assert!(
                edge.to
                    .as_ref()
                    .is_some_and(|target| target.path.ends_with(suffix)),
                "{text}: {:?}",
                edge.to
            ),
            None => {
                assert!(edge.to.is_none(), "{text}: {:?}", edge.to);
                assert_eq!(
                    edge.diagnostics[0].code,
                    DiagnosticCode::UnresolvedReference
                );
            }
        }
    }
}
#[path = "../../../fuzz/fuzz_targets/support/reference_resolution.rs"]
mod reference_resolution;

#[test]
fn generated_reference_chains_preserve_every_component_and_refuse_invalid_suffixes() {
    for first in u8::MIN..=u8::MAX {
        reference_resolution::check(&[first, 0, 1, 2, 15, 22, 255, 254, 253]);
    }
    reference_resolution::check(include_bytes!(
        "../../../fuzz/corpus/reference_resolution/branch-chains"
    ));
}
#[test]
fn references_refuse_duplicate_provisions_and_exclude_quoted_or_unapplied_articles() {
    for present in [false, true] {
        for quoted in [
            "<Article Num=\"1\"><Paragraph Num=\"1\"><Sentence>Quoted text</Sentence></Paragraph></Article>",
            "<LawBody><MainProvision><Article Num=\"1\"><Sentence>Quoted law body</Sentence></Article></MainProvision></LawBody>",
        ] {
            let actual = if present {
                "<Article Num=\"1\"><Paragraph Num=\"1\"><Sentence>Actual text</Sentence></Paragraph></Article>"
            } else {
                ""
            };
            let mut law = revision();
            law.tree = xml::parse_xml(&format!("<Law><LawBody><MainProvision>{actual}<Article Num=\"2\"><Paragraph Num=\"1\"><Sentence>第一条</Sentence></Paragraph><Paragraph Num=\"2\"><Sentence><QuoteStruct>{quoted}</QuoteStruct></Sentence></Paragraph><AmendProvision><NewProvision><Article Num=\"1\"><Sentence>Unapplied amendment</Sentence></Article></NewProvision></AmendProvision></Article></MainProvision></LawBody></Law>")).unwrap();
            let edges = references::analyze(&BTreeMap::from([(
                "DEMO_ACT".into(),
                ("version".into(), law),
            )]));
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0].to.is_some(), present);
            if let Some(target) = &edges[0].to {
                assert_eq!(
                    target.path,
                    "/Law/LawBody[index=1]/MainProvision[index=1]/Article[num=1]"
                );
            } else {
                assert_eq!(
                    edges[0].diagnostics[0].code,
                    DiagnosticCode::UnresolvedReference
                );
            }
        }
    }
    let mut law = revision();
    law.tree = xml::parse_xml("<Law><LawBody><MainProvision><Article Num=\"1\"><Sentence>First</Sentence></Article><Article Num=\"1\"><Sentence>Second</Sentence></Article><Article Num=\"2\"><Sentence>第一条</Sentence></Article></MainProvision></LawBody></Law>").unwrap();
    let edges = references::analyze(&BTreeMap::from([(
        "DEMO_ACT".into(),
        ("version".into(), law),
    )]));
    assert_eq!(edges.len(), 1);
    assert!(edges[0].to.is_none());
}
