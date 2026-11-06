use super::*;

#[test]
fn provision_context_checks_every_parent_role_transition_in_an_independent_matrix() {
    use ProvisionContext::{Article, Body, Excluded, Main, Root, Start};
    for context in [Start, Root, Body, Main, Article, Excluded] {
        let expected = match context {
            Start => [Root, Excluded, Excluded, Excluded, Excluded, Excluded],
            Root => [Excluded, Body, Main, Excluded, Excluded, Excluded],
            Body => [Excluded, Excluded, Main, Excluded, Excluded, Excluded],
            Main => [Excluded, Excluded, Excluded, Main, Article, Excluded],
            Article | Excluded => [Excluded; 6],
        };
        for role in [
            ProvisionRole::Law,
            ProvisionRole::Body,
            ProvisionRole::Main,
            ProvisionRole::Subdivision,
            ProvisionRole::Article,
            ProvisionRole::Other,
        ] {
            let column = match role {
                ProvisionRole::Law => 0,
                ProvisionRole::Body => 1,
                ProvisionRole::Main => 2,
                ProvisionRole::Subdivision => 3,
                ProvisionRole::Article => 4,
                ProvisionRole::Other => 5,
            };
            assert_eq!(
                provision_context(context, role),
                expected[column],
                "{context:?}, {role:?}"
            );
        }
    }
}

#[test]
fn snapshot_evidence_checks_independent_calendar_tuples_and_both_candidate_states() {
    let dates = [
        (1969, 12, 30),
        (1969, 12, 31),
        (1970, 1, 1),
        (1970, 1, 2),
        (1999, 12, 31),
        (2000, 1, 1),
        (2000, 2, 28),
        (2000, 2, 29),
        (2000, 3, 1),
    ];
    for observed in dates {
        for at in dates {
            for matches in [false, true] {
                assert_eq!(
                    current_snapshot_is_evidence(
                        CivilDay::new(observed.0, observed.1, observed.2).unwrap(),
                        CivilDay::new(at.0, at.1, at.2).unwrap(),
                        matches,
                    ),
                    matches && observed <= at,
                    "{observed:?}, {at:?}, {matches}",
                );
            }
        }
    }
}

#[test]
fn title_attribute_conditions_remain_independent_for_every_combination() {
    for mask in 0..8 {
        assert_eq!(
            inline_title_attribute_allowed(mask & 1 != 0, mask & 2 != 0, mask & 4 != 0),
            mask == 7
        );
    }
}

#[test]
fn each_independent_revision_axis_requires_a_derived_identity() {
    for mask in 0..8 {
        assert_eq!(
            revision_has_changes(mask & 1 != 0, mask & 2 != 0, mask & 4 != 0),
            mask != 0
        );
    }
}

#[test]
fn ruby_context_preserves_content_and_absorbs_every_reading_descendant() {
    use RubyContext::{Content as C, Reading as A, Ruby as R};
    use RubyRole::{Content as Text, Reading as Rt, Ruby};
    for (parent, role, expected) in [
        (C, Text, C),
        (C, Ruby, R),
        (C, Rt, C),
        (R, Text, C),
        (R, Ruby, R),
        (R, Rt, A),
        (A, Text, A),
        (A, Ruby, A),
        (A, Rt, A),
    ] {
        assert_eq!(ruby_context(parent, role), expected);
    }
}

#[test]
fn termination_evidence_does_not_confuse_an_unknown_end_with_no_end_claim() {
    for (claimed, date, expected) in [
        (
            false,
            TerminationDateEvidence::Absent,
            TerminationEvidence::NoClaim,
        ),
        (
            false,
            TerminationDateEvidence::Uncertain,
            TerminationEvidence::UnknownEnd,
        ),
        (
            false,
            TerminationDateEvidence::Known,
            TerminationEvidence::KnownEnd,
        ),
        (
            true,
            TerminationDateEvidence::Absent,
            TerminationEvidence::UnknownEnd,
        ),
        (
            true,
            TerminationDateEvidence::Uncertain,
            TerminationEvidence::UnknownEnd,
        ),
        (
            true,
            TerminationDateEvidence::Known,
            TerminationEvidence::KnownEnd,
        ),
    ] {
        assert_eq!(termination_evidence(claimed, date), expected);
    }
}

#[test]
fn evidence_selection_distinguishes_absence_agreement_and_conflicting_values() {
    for (first, second, equal, expected) in [
        (false, false, false, EvidenceChoice::Absent),
        (false, false, true, EvidenceChoice::Absent),
        (true, false, false, EvidenceChoice::First),
        (true, false, true, EvidenceChoice::First),
        (false, true, false, EvidenceChoice::Second),
        (false, true, true, EvidenceChoice::Second),
        (true, true, false, EvidenceChoice::Conflict),
        (true, true, true, EvidenceChoice::First),
    ] {
        assert_eq!(evidence_choice(first, second, equal), expected);
    }
}

#[test]
fn enforcement_date_roles_keep_absence_placeholders_and_distinct_values_separate() {
    for (present, matches_schedule, expected) in [
        (false, false, EnforcementDateRole::Unspecified),
        (false, true, EnforcementDateRole::Unspecified),
        (true, false, EnforcementDateRole::DistinctValue),
        (true, true, EnforcementDateRole::ScheduledPlaceholder),
    ] {
        assert_eq!(enforcement_date_role(present, matches_schedule), expected);
    }
}

#[test]
fn decimal_digits_accept_exactly_ascii_and_fullwidth_decimal_codepoints() {
    for code in 0..=0x10ffff {
        if let Some(character) = char::from_u32(code) {
            let expected = match code {
                0x30..=0x39 => Some((code - 0x30) as u8),
                0xff10..=0xff19 => Some((code - 0xff10) as u8),
                _ => None,
            };
            assert_eq!(decimal_digit(character), expected, "{code:X}");
        }
    }
}

#[test]
fn decimal_accumulation_refuses_invalid_digits_and_overflow_at_each_boundary() {
    for number in [
        0,
        1,
        u32::MAX / 10 - 1,
        u32::MAX / 10,
        u32::MAX / 10 + 1,
        u32::MAX,
    ] {
        for digit in 0..=u8::MAX {
            let mathematical = u64::from(number) * 10 + u64::from(digit);
            let expected = if digit < 10 && mathematical <= u64::from(u32::MAX) {
                Some(mathematical as u32)
            } else {
                None
            };
            assert_eq!(append_decimal_digit(number, digit), expected);
        }
    }
}

#[test]
fn enforcement_order_requires_all_sequences_when_a_day_has_multiple_revisions() {
    for revisions in [0, 1, 2, 3, usize::MAX] {
        for sequences in [0, 1, 2, 3, usize::MAX] {
            let expected = match revisions {
                0 => sequences == 0,
                1 => sequences <= 1,
                _ => sequences == revisions,
            };
            assert_eq!(enforcement_group_is_ordered(revisions, sequences), expected);
        }
    }
}
use alloc::vec::Vec;

#[test]
fn observation_arbitration_checks_all_order_and_identity_combinations() {
    use core::cmp::Ordering::*;
    for (order, same, expected) in [
        (Less, false, ObservationChoice::Keep),
        (Less, true, ObservationChoice::Keep),
        (Equal, false, ObservationChoice::Conflict),
        (Equal, true, ObservationChoice::Keep),
        (Greater, false, ObservationChoice::Replace),
        (Greater, true, ObservationChoice::Replace),
    ] {
        assert_eq!(observation_choice(order, same), expected);
    }
}

#[test]
fn schema_names_admit_only_ascii_letter_or_underscore_then_ascii_name_bytes() {
    assert!(!schema_name(b""));
    for first in u8::MIN..=u8::MAX {
        let starts = first.is_ascii_alphabetic() || first == b'_';
        assert_eq!(schema_name(&[first]), starts);
        for second in u8::MIN..=u8::MAX {
            assert_eq!(
                schema_name(&[first, second]),
                starts && (second.is_ascii_alphanumeric() || second == b'_')
            );
        }
    }
}

#[test]
fn structural_classification_checks_all_metadata_text_and_order_states() {
    for attributes in [false, true] {
        for text in [false, true] {
            for order in [false, true] {
                for members in [false, true] {
                    let expected = match (attributes, text, order, members) {
                        (_, _, false, true) => StructuralChange::Moved,
                        (true, true, _, _) => StructuralChange::Unchanged,
                        _ => StructuralChange::Changed,
                    };
                    assert_eq!(
                        structural_change(attributes, text, order, members),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn calendar_constructor_matches_independent_month_table() {
    for year in 0..=10_000 {
        for month in 0..=13 {
            for day in [0, 1, 27, 28, 29, 30, 31, 32] {
                let limit = match month {
                    2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    _ => 0,
                };
                let expected = if year == 0 || year > 9999 {
                    Err(DateError::Year)
                } else if limit == 0 {
                    Err(DateError::Month)
                } else if day == 0 || day > limit {
                    Err(DateError::Day)
                } else {
                    Ok((year, month, day))
                };
                assert_eq!(
                    CivilDay::new(year, month, day).map(CivilDay::parts),
                    expected
                );
            }
        }
    }
    assert_eq!(
        CivilDay::new(u16::MAX, u8::MAX, u8::MAX),
        Err(DateError::Year)
    );
    assert_eq!(CivilDay::new(1969, u8::MAX, 1), Err(DateError::Month));
    assert_eq!(CivilDay::new(1969, 1, u8::MAX), Err(DateError::Day));
}

#[test]
fn date_order_and_effectivity_include_both_boundaries() {
    let before = CivilDay::new(1969, 12, 31).unwrap();
    let start = CivilDay::new(1970, 1, 1).unwrap();
    let middle = CivilDay::new(1970, 1, 2).unwrap();
    let repeal = CivilDay::new(1970, 1, 3).unwrap();
    let after = CivilDay::new(1970, 1, 4).unwrap();
    assert!(before < start && start < middle && middle < repeal && repeal < after);
    for at in [before, start, middle, repeal, after] {
        assert_eq!(
            effective_state(None, Some(repeal), at),
            EffectiveState::Uncertain
        );
    }
    for at in [before, start, middle, repeal, after] {
        let expected = if at == before {
            EffectiveState::Future
        } else if at >= repeal {
            EffectiveState::Repealed
        } else {
            EffectiveState::Effective
        };
        assert_eq!(effective_state(Some(start), Some(repeal), at), expected);
        assert_eq!(
            effective_state(Some(start), None, at),
            if at == before {
                EffectiveState::Future
            } else {
                EffectiveState::Effective
            }
        );
    }
    assert_eq!(
        effective_state(Some(start), Some(start), start),
        EffectiveState::Repealed
    );
    assert_eq!(
        effective_state(Some(start), Some(before), before),
        EffectiveState::Future
    );
}

#[test]
fn patch_decisions_preserve_failure_priority() {
    for base in [false, true] {
        for original in [false, true] {
            for conflict in [false, true] {
                let expected = match (base, original, conflict) {
                    (false, _, _) => PatchDecision::WrongBase,
                    (true, false, _) => PatchDecision::WrongOriginal,
                    (true, true, true) => PatchDecision::Conflict,
                    (true, true, false) => PatchDecision::Apply,
                };
                assert_eq!(patch_decision(base, original, conflict), expected);
            }
        }
    }
}

#[test]
fn replacements_preserve_exact_utf8_spans_and_refuse_ambiguity() {
    for (text, old, new, expected) in [
        ("甲乙丙", "乙", "新規", "甲新規丙"),
        ("甲乙", "甲", "", "乙"),
        ("甲乙", "乙", "", "甲"),
        ("甲", "甲", "新", "新"),
        ("prefix/old/suffix", "old", "new", "prefix/new/suffix"),
    ] {
        let range = unique_match(text, old).unwrap();
        assert_eq!(&text[range], old);
        assert_eq!(replace_unique(text, old, new).as_deref(), Ok(expected));
    }
    for (text, old, expected) in [
        ("甲", "", TextEditError::EmptyOriginal),
        ("", "甲", TextEditError::MissingOriginal),
        ("乙", "甲", TextEditError::MissingOriginal),
        ("甲甲", "甲", TextEditError::AmbiguousOriginal),
        ("甲甲甲", "甲甲", TextEditError::AmbiguousOriginal),
        ("ababa", "aba", TextEditError::AmbiguousOriginal),
    ] {
        assert_eq!(unique_match(text, old), Err(expected));
        assert_eq!(replace_unique(text, old, "new"), Err(expected));
    }
}

#[test]
fn replacement_allocation_limit_is_inclusive_and_overflow_safe() {
    let limit = 33_554_432;
    assert_eq!(replacement_length(0, 0), Ok(0));
    assert_eq!(replacement_length(0, limit), Ok(limit));
    assert_eq!(replacement_length(limit, 0), Ok(limit));
    assert_eq!(replacement_length(limit - 1, 1), Ok(limit));
    assert_eq!(replacement_length(limit, 1), Err(TextEditError::TooLong));
    assert_eq!(
        replacement_length(usize::MAX, 1),
        Err(TextEditError::TooLong)
    );
    assert_eq!(
        replacement_length(1, usize::MAX),
        Err(TextEditError::TooLong)
    );
    assert_eq!(
        replacement_length(usize::MAX, usize::MAX),
        Err(TextEditError::TooLong)
    );
}

#[test]
fn replacement_applies_the_limit_to_output_instead_of_input_or_capacity() {
    let limit = 33_554_432;
    let within_limit = alloc::format!("XY{}", "a".repeat(limit - 1));
    let result = replace_unique(&within_limit, "XY", "b").unwrap();
    assert_eq!(result.len(), limit);
    assert!(result.starts_with('b'));
    assert!(result[1..].bytes().all(|byte| byte == b'a'));
    let over_limit = alloc::format!("XY{}", "a".repeat(limit));
    assert_eq!(
        replace_unique(&over_limit, "XY", "b").map(|text| text.len()),
        Err(TextEditError::TooLong)
    );
}

fn words(maximum: usize) -> Vec<String> {
    let mut all = alloc::vec![String::new()];
    let mut level = alloc::vec![String::new()];
    for _ in 0..maximum {
        let mut next = Vec::new();
        for prefix in &level {
            for symbol in ['a', '甲', '乙'] {
                let mut word = prefix.clone();
                word.push(symbol);
                next.push(word);
            }
        }
        all.extend(next.clone());
        level = next;
    }
    all
}

#[test]
fn utf8_search_and_replacement_match_an_independent_exhaustive_oracle() {
    let needles = words(3);
    for text in words(5) {
        for old in &needles {
            let positions: Vec<_> = text
                .char_indices()
                .map(|(index, _)| index)
                .filter(|&index| text.as_bytes()[index..].starts_with(old.as_bytes()))
                .collect();
            let expected = if old.is_empty() {
                Err(TextEditError::EmptyOriginal)
            } else {
                match positions.as_slice() {
                    [] => Err(TextEditError::MissingOriginal),
                    [start] => Ok(*start..*start + old.len()),
                    _ => Err(TextEditError::AmbiguousOriginal),
                }
            };
            assert_eq!(unique_match(&text, old), expected, "{text:?} {old:?}");
            for new in ["", "a", "乙丙"] {
                let expected = expected.clone().map(|span| {
                    alloc::format!("{}{}{}", &text[..span.start], new, &text[span.end..])
                });
                assert_eq!(
                    replace_unique(&text, old, new),
                    expected,
                    "{text:?} {old:?} {new:?}"
                );
            }
        }
    }
}

#[test]
fn span_planning_checks_invalid_order_limits_and_overflow() {
    assert_eq!(
        match_range(usize::MAX, Some(1), false),
        Err(TextEditError::TooLong)
    );
    assert_eq!(
        match_range(0, Some(usize::MAX), true),
        Err(TextEditError::EmptyOriginal)
    );
    for (length, span) in [
        (0, 1..2),
        (1, core::ops::Range { start: 2, end: 1 }),
        (1, 0..2),
    ] {
        assert!(matches!(
            plan_replacement(length, span, 0),
            Err(TextEditError::InvalidSpan)
        ));
    }
    for (length, span, inserted, output) in [
        (0, 0..0, 0, 0),
        (4, 1..3, 2, 4),
        (4, 0..4, 0, 0),
        (33_554_432, 1..2, 1, 33_554_432),
    ] {
        let plan = plan_replacement(length, span.clone(), inserted).unwrap();
        assert_eq!(plan.span, span);
        assert_eq!(plan.output_length, output);
    }
    assert!(matches!(
        plan_replacement(usize::MAX, 0..0, 1),
        Err(TextEditError::TooLong)
    ));
}

#[test]
fn planning_boundary_grid_preserves_exact_ranges_and_error_priority() {
    let boundaries = [0, 1, 2, 33_554_431, 33_554_432, 33_554_433, usize::MAX];
    for length in boundaries {
        for start in boundaries {
            for end in boundaries {
                for inserted in boundaries {
                    let actual = plan_replacement(length, start..end, inserted);
                    if start > end || end > length {
                        assert!(matches!(actual, Err(TextEditError::InvalidSpan)));
                    } else {
                        let remaining = start + (length - end);
                        let expected = remaining
                            .checked_add(inserted)
                            .filter(|&value| value <= 33_554_432);
                        match (actual, expected) {
                            (Ok(plan), Some(size)) => {
                                assert_eq!(plan.span, start..end);
                                assert_eq!(plan.output_length, size);
                            }
                            (Err(TextEditError::TooLong), None) => {}
                            _ => panic!("Invalid plan for {length} {start} {end} {inserted}"),
                        }
                    }
                }
            }
        }
    }
    for length in [0, 1, 2, usize::MAX] {
        for first in [None, Some(0), Some(1), Some(usize::MAX)] {
            for repeated in [false, true] {
                let expected = match (length, first, repeated) {
                    (0, _, _) => Err(TextEditError::EmptyOriginal),
                    (_, None, _) => Err(TextEditError::MissingOriginal),
                    (_, Some(_), true) => Err(TextEditError::AmbiguousOriginal),
                    (_, Some(start), false) => start
                        .checked_add(length)
                        .map(|end| start..end)
                        .ok_or(TextEditError::TooLong),
                };
                assert_eq!(match_range(length, first, repeated), expected);
            }
        }
    }
}
#[test]
fn number_addresses_reject_only_empty_zero_and_path_delimiters() {
    assert!(!super::number_can_address(&[]));
    for first in u8::MIN..=u8::MAX {
        assert_eq!(
            super::number_can_address(&[first]),
            !b"0/[]".contains(&first)
        );
        for second in u8::MIN..=u8::MAX {
            assert_eq!(
                super::number_can_address(&[first, second]),
                !b"/[]"
                    .iter()
                    .any(|delimiter| *delimiter == first || *delimiter == second)
            );
        }
    }
}
