use super::proof_native_log as native_log;
use super::*;

#[kani::proof]
fn mutation_capacity_preserves_headroom_and_selects_the_largest_allowed_worker_count() {
    let available: u64 = kani::any();
    let requested: usize = kani::any();
    let workers = proof_mutation_capacity::workers(available, requested);
    assert_eq!(proof_mutation_capacity::MAXIMUM_WORKERS, 6);
    assert_eq!(proof_mutation_capacity::RESERVE_BYTES, 2_147_483_648);
    assert_eq!(proof_mutation_capacity::WORKER_BYTES, 1_073_741_824);
    assert!(workers <= requested && workers <= 6);
    if workers > 0 {
        assert!(available >= 2_147_483_648 + workers as u64 * 1_073_741_824);
    }
    if workers < requested && workers < 6 {
        assert!(available < 2_147_483_648 + (workers as u64 + 1) * 1_073_741_824);
    }
    kani::cover!(workers == 0 && requested == 0);
    kani::cover!(workers == 0 && requested > 0);
    kani::cover!(workers == 1 && available == 3_221_225_472 && requested == 6);
    kani::cover!(workers == 6 && requested > 6);
    kani::cover!(workers == 2 && requested == 2 && available > 8_589_934_592);
}

#[kani::proof]
fn mutation_evidence_requires_complete_selection_including_a_single_shard() {
    let list_only: bool = kani::any();
    let partial: bool = kani::any();
    assert_eq!(
        proof_complete_mutation_inventory::required(list_only, partial),
        matches!((list_only, partial), (false, false))
    );
    kani::cover!(!list_only && !partial);
    kani::cover!(!list_only && partial);
    kani::cover!(list_only && !partial);
    kani::cover!(list_only && partial);
}

#[kani::proof]
fn input_limits_admit_the_boundary_and_refuse_every_larger_size() {
    let size: usize = kani::any();
    let limit: usize = kani::any();
    assert_eq!(input_fits(size, limit), size < limit || size == limit);
    kani::cover!(size < limit);
    kani::cover!(size == limit);
    kani::cover!(size > limit);
}

#[kani::proof]
fn knowledge_cutoffs_include_equal_observations() {
    let rank: u8 = kani::any();
    kani::assume(rank < 3);
    let order = match rank {
        0 => core::cmp::Ordering::Less,
        1 => core::cmp::Ordering::Equal,
        _ => core::cmp::Ordering::Greater,
    };
    assert_eq!(knowledge_cutoff_accepts(order), rank <= 1);
    kani::cover!(rank == 0);
    kani::cover!(rank == 1);
    kani::cover!(rank == 2);
}

#[kani::proof]
#[kani::unwind(67)]
fn image_digests_require_exactly_64_lowercase_hex_digits_within_65_bytes() {
    let data: [u8; 65] = kani::any();
    let length: u8 = kani::any();
    kani::assume(length <= 65);
    let digest = &data[..usize::from(length)];
    let invalid = digest
        .iter()
        .any(|byte| *byte < b'0' || (*byte > b'9' && *byte < b'a') || *byte > b'f');
    let expected = length == 64 && !invalid;
    assert_eq!(proof_image_reference::valid_digest(digest), expected);
    kani::cover!(length == 64 && expected);
    kani::cover!(length == 64 && !expected);
    kani::cover!(length != 64);
}

#[kani::proof]
fn native_log_limits_check_every_machine_size_length() {
    let bytes: usize = kani::any();
    assert_eq!(native_log::MAXIMUM_BYTES, 8_388_608);
    assert_eq!(native_log::allows(bytes), bytes <= 8_388_608);
    kani::cover!(bytes == 0);
    kani::cover!(bytes > 0 && bytes < native_log::MAXIMUM_BYTES);
    kani::cover!(bytes == native_log::MAXIMUM_BYTES);
    kani::cover!(bytes > native_log::MAXIMUM_BYTES);
}

#[kani::proof]
fn inline_title_attributes_require_each_independent_condition() {
    let line: bool = kani::any();
    let style: bool = kani::any();
    let known_value: bool = kani::any();
    assert_eq!(
        inline_title_attribute_allowed(line, style, known_value),
        matches!((line, style, known_value), (true, true, true))
    );
    kani::cover!(!line && !style && !known_value);
    kani::cover!(line && !style && !known_value);
    kani::cover!(!line && style && !known_value);
    kani::cover!(!line && !style && known_value);
    kani::cover!(line && style && !known_value);
    kani::cover!(line && !style && known_value);
    kani::cover!(!line && style && known_value);
    kani::cover!(line && style && known_value);
}

#[kani::proof]
fn revision_identity_changes_for_each_independent_preserved_axis() {
    let content: bool = kani::any();
    let repeal: bool = kani::any();
    let enforcement: bool = kani::any();
    let changed = revision_has_changes(content, repeal, enforcement);
    assert_eq!(!changed, !content && !repeal && !enforcement);
    kani::cover!(!content && !repeal && !enforcement);
    kani::cover!(content && !repeal && !enforcement);
    kani::cover!(!content && repeal && !enforcement);
    kani::cover!(!content && !repeal && enforcement);
    kani::cover!(content && repeal && !enforcement);
    kani::cover!(content && !repeal && enforcement);
    kani::cover!(!content && repeal && enforcement);
    kani::cover!(content && repeal && enforcement);
}

#[kani::proof]
fn ruby_annotations_are_excluded_only_in_reading_scope_and_never_reenter_content() {
    let parent: RubyContext = kani::any();
    let role: RubyRole = kani::any();
    let result = ruby_context(parent, role);
    let reading = parent == RubyContext::Reading
        || (parent == RubyContext::Ruby && role == RubyRole::Reading);
    assert_eq!(result == RubyContext::Reading, reading);
    assert_eq!(
        result == RubyContext::Ruby,
        !reading && role == RubyRole::Ruby
    );
    assert_eq!(
        result == RubyContext::Content,
        !reading && role != RubyRole::Ruby
    );
    kani::cover!(parent == RubyContext::Content && role == RubyRole::Content);
    kani::cover!(parent == RubyContext::Content && role == RubyRole::Ruby);
    kani::cover!(parent == RubyContext::Content && role == RubyRole::Reading);
    kani::cover!(parent == RubyContext::Ruby && role == RubyRole::Content);
    kani::cover!(parent == RubyContext::Ruby && role == RubyRole::Ruby);
    kani::cover!(parent == RubyContext::Ruby && role == RubyRole::Reading);
    kani::cover!(parent == RubyContext::Reading && role == RubyRole::Content);
    kani::cover!(parent == RubyContext::Reading && role == RubyRole::Ruby);
    kani::cover!(parent == RubyContext::Reading && role == RubyRole::Reading);
}

#[kani::proof]
fn cover_evidence_requires_all_declared_and_required_witnesses() {
    use super::proof_cover_evidence::{self as cover_evidence, CoverCounts};
    let actual: Option<CoverCounts> = kani::any();
    let required: Option<usize> = kani::any();
    let complete = cover_evidence::is_complete(actual, required);
    match actual {
        None => assert_eq!(complete, required.is_none()),
        Some(counts) => {
            let matches_requirement = match required {
                Some(expected) => expected == counts.total,
                None => true,
            };
            assert_eq!(
                complete,
                counts.reached > 0 && counts.reached == counts.total && matches_requirement
            );
            kani::cover!(counts.total == 0);
            kani::cover!(counts.total > 0 && counts.reached < counts.total);
            kani::cover!(counts.total > 0 && counts.reached > counts.total);
            kani::cover!(counts.total > 0 && counts.reached == counts.total && required.is_none());
            if let Some(expected) = required {
                kani::cover!(
                    counts.total > 0 && counts.reached == counts.total && expected == counts.total
                );
                kani::cover!(
                    counts.total > 0 && counts.reached == counts.total && expected != counts.total
                );
            }
        }
    }
    kani::cover!(actual.is_none() && required.is_none());
    kani::cover!(actual.is_none() && required.is_some());
}

#[kani::proof]
fn termination_claims_with_no_established_date_remain_unknown_instead_of_absent() {
    let claimed: bool = kani::any();
    let date: TerminationDateEvidence = kani::any();
    let result = termination_evidence(claimed, date);
    assert_eq!(
        result == TerminationEvidence::NoClaim,
        !claimed && date == TerminationDateEvidence::Absent
    );
    assert_eq!(
        result == TerminationEvidence::KnownEnd,
        date == TerminationDateEvidence::Known
    );
    assert_eq!(
        result == TerminationEvidence::UnknownEnd,
        date == TerminationDateEvidence::Uncertain
            || (claimed && date == TerminationDateEvidence::Absent)
    );
    kani::cover!(!claimed && date == TerminationDateEvidence::Absent);
    kani::cover!(!claimed && date == TerminationDateEvidence::Uncertain);
    kani::cover!(!claimed && date == TerminationDateEvidence::Known);
    kani::cover!(claimed && date == TerminationDateEvidence::Absent);
    kani::cover!(claimed && date == TerminationDateEvidence::Uncertain);
    kani::cover!(claimed && date == TerminationDateEvidence::Known);
}

#[kani::proof]
fn complementary_evidence_preserves_provided_values_and_refuses_conflicting_claims() {
    let first: bool = kani::any();
    let second: bool = kani::any();
    let equal: bool = kani::any();
    let choice = evidence_choice(first, second, equal);
    assert_eq!(choice == EvidenceChoice::Absent, !first && !second);
    assert_eq!(choice == EvidenceChoice::First, first && (!second || equal));
    assert_eq!(choice == EvidenceChoice::Second, !first && second);
    assert_eq!(
        choice == EvidenceChoice::Conflict,
        first && second && !equal
    );
}

#[kani::proof]
fn enforcement_date_roles_never_promote_absent_or_scheduled_values_to_distinct_values() {
    let present: bool = kani::any();
    let matches_schedule: bool = kani::any();
    let role = enforcement_date_role(present, matches_schedule);
    assert_eq!(role == EnforcementDateRole::Unspecified, !present);
    assert_eq!(
        role == EnforcementDateRole::ScheduledPlaceholder,
        present && matches_schedule
    );
    assert_eq!(
        role == EnforcementDateRole::DistinctValue,
        present && !matches_schedule
    );
}

#[kani::proof]
fn provision_context_admits_exact_parent_roles_and_never_reenters_after_exclusion() {
    let context: ProvisionContext = kani::any();
    let role: ProvisionRole = kani::any();
    let result = provision_context(context, role);
    let allowed = matches!(
        (context, role),
        (ProvisionContext::Start, ProvisionRole::Law)
            | (
                ProvisionContext::Root,
                ProvisionRole::Body | ProvisionRole::Main
            )
            | (ProvisionContext::Body, ProvisionRole::Main)
            | (
                ProvisionContext::Main,
                ProvisionRole::Subdivision | ProvisionRole::Article
            )
    );
    assert_eq!(result != ProvisionContext::Excluded, allowed);
    assert_eq!(
        result == ProvisionContext::Article,
        context == ProvisionContext::Main && role == ProvisionRole::Article
    );
    assert_eq!(
        result == ProvisionContext::Root,
        context == ProvisionContext::Start && role == ProvisionRole::Law
    );
    assert_eq!(
        result == ProvisionContext::Body,
        context == ProvisionContext::Root && role == ProvisionRole::Body
    );
    assert_eq!(
        result == ProvisionContext::Main,
        matches!(
            (context, role),
            (
                ProvisionContext::Root | ProvisionContext::Body,
                ProvisionRole::Main
            ) | (ProvisionContext::Main, ProvisionRole::Subdivision)
        )
    );
    assert_eq!(
        provision_context(ProvisionContext::Excluded, role),
        ProvisionContext::Excluded
    );
    kani::cover!(result == ProvisionContext::Article);
    kani::cover!(context == ProvisionContext::Excluded && role == ProvisionRole::Law);
}

#[kani::proof]
fn current_snapshot_evidence_requires_an_observed_day_and_a_matching_candidate() {
    let observed = arbitrary_day();
    let at = arbitrary_day();
    let matches_candidate: bool = kani::any();
    assert_eq!(
        current_snapshot_is_evidence(observed, at, matches_candidate),
        matches_candidate && observed.parts() <= at.parts()
    );
    kani::cover!(matches_candidate && observed == at);
    kani::cover!(matches_candidate && at < observed);
    kani::cover!(!matches_candidate && observed < at);
}

#[kani::proof]
fn json_errors_never_duplicate_an_existing_report() {
    let json: bool = kani::any();
    let reported: bool = kani::any();
    assert_eq!(
        super::error_output::failure_output(json, reported)
            == super::error_output::FailureOutput::Json,
        json && !reported
    );
}

#[kani::proof]
fn observation_updates_keep_newest_and_reject_conflicting_ties() {
    let comparison = match kani::any::<u8>() {
        0 => core::cmp::Ordering::Less,
        1 => core::cmp::Ordering::Equal,
        _ => core::cmp::Ordering::Greater,
    };
    let identical: bool = kani::any();
    let result = observation_choice(comparison, identical);
    assert_eq!(
        result == ObservationChoice::Replace,
        comparison == core::cmp::Ordering::Greater
    );
    assert_eq!(
        result == ObservationChoice::Conflict,
        comparison == core::cmp::Ordering::Equal && !identical
    );
    assert_eq!(
        result == ObservationChoice::Keep,
        comparison == core::cmp::Ordering::Less
            || (comparison == core::cmp::Ordering::Equal && identical)
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn number_addresses_require_nonempty_nonsentinel_delimiter_free_values() {
    let bytes: [u8; 4] = kani::any();
    let length: u8 = kani::any();
    kani::assume(length <= 4);
    let length = usize::from(length);
    let mut valid = length != 0 && !(length == 1 && bytes[0] == b'0');
    for index in 0..length {
        valid &= bytes[index] != b'/' && bytes[index] != b'[' && bytes[index] != b']';
    }
    assert_eq!(number_can_address(&bytes[..length]), valid);
    kani::cover!(valid && length == 4);
    kani::cover!(!valid && length == 0);
    kani::cover!(!valid && length == 1 && bytes[0] == b'0');
    kani::cover!(!valid && length == 4 && bytes[3] == b'/');
}

#[kani::proof]
#[kani::unwind(8)]
fn schema_names_check_every_bounded_byte_sequence() {
    let bytes: [u8; 4] = kani::any();
    let length: u8 = kani::any();
    kani::assume(length <= 4);
    let length = usize::from(length);
    let letter =
        |byte: u8| (b'a'..=b'z').contains(&byte) || (b'A'..=b'Z').contains(&byte) || byte == b'_';
    let mut valid = length != 0;
    for index in 0..length {
        valid &= letter(bytes[index]) || (index > 0 && (b'0'..=b'9').contains(&bytes[index]));
    }
    assert_eq!(schema_name(&bytes[..length]), valid);
    kani::cover!(valid && length == 4);
    kani::cover!(!valid && length == 0);
}

#[kani::proof]
fn structural_classification_distinguishes_content_order_and_membership() {
    let attributes: bool = kani::any();
    let text: bool = kani::any();
    let order: bool = kani::any();
    let members: bool = kani::any();
    let result = structural_change(attributes, text, order, members);
    assert_eq!(result == StructuralChange::Moved, !order && members);
    assert_eq!(
        result == StructuralChange::Unchanged,
        attributes && text && (order || !members)
    );
    assert_eq!(
        result == StructuralChange::Changed,
        (!attributes || !text) && (order || !members)
    );
    kani::cover!(result == StructuralChange::Moved && !attributes);
    kani::cover!(result == StructuralChange::Unchanged && !members);
}

#[kani::proof]
fn civil_day_validates_all_components() {
    let year: u16 = kani::any();
    let month: u8 = kani::any();
    let day: u8 = kani::any();
    let leap = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
    let month_days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    let expected = if year == 0 || year > 9999 {
        Err(DateError::Year)
    } else if month_days == 0 {
        Err(DateError::Month)
    } else if day == 0 || day > month_days {
        Err(DateError::Day)
    } else {
        Ok((year, month, day))
    };
    assert_eq!(
        CivilDay::new(year, month, day).map(CivilDay::parts),
        expected
    );
}

fn arbitrary_day() -> CivilDay {
    let result = CivilDay::new(kani::any(), kani::any(), kani::any());
    kani::assume(result.is_ok());
    result.unwrap()
}

#[kani::proof]
fn civil_day_order_matches_components() {
    let left = arbitrary_day();
    let right = arbitrary_day();
    assert_eq!(left.cmp(&right), left.parts().cmp(&right.parts()));
    assert_eq!(left == right, left.parts() == right.parts());
}

#[kani::proof]
fn unknown_enforcement_never_becomes_effective() {
    let at = arbitrary_day();
    let repeal = if kani::any() {
        Some(arbitrary_day())
    } else {
        None
    };
    assert_eq!(effective_state(None, repeal, at), EffectiveState::Uncertain);
}

#[kani::proof]
fn effectivity_observes_inclusive_start_and_repeal() {
    let start = arbitrary_day();
    let at = arbitrary_day();
    let repeal = arbitrary_day();
    let expected = if at < start {
        EffectiveState::Future
    } else if at < repeal {
        EffectiveState::Effective
    } else {
        EffectiveState::Repealed
    };
    assert_eq!(effective_state(Some(start), Some(repeal), at), expected);
    assert_eq!(
        effective_state(Some(start), None, start),
        EffectiveState::Effective
    );
    assert_eq!(
        effective_state(Some(start), Some(start), start),
        EffectiveState::Repealed
    );
    kani::cover!(start == at && at == repeal);
    kani::cover!(repeal < at && at < start);
}

#[kani::proof]
fn effectivity_without_repeal_changes_only_at_start() {
    let start = arbitrary_day();
    let at = arbitrary_day();
    assert_eq!(
        effective_state(Some(start), None, at),
        if at < start {
            EffectiveState::Future
        } else {
            EffectiveState::Effective
        }
    );
}

#[kani::proof]
fn patch_requires_all_preconditions() {
    let base: bool = kani::any();
    let original: bool = kani::any();
    let conflict: bool = kani::any();
    let decision = patch_decision(base, original, conflict);
    assert_eq!(
        decision == PatchDecision::Apply,
        base && original && !conflict
    );
    assert_eq!(decision == PatchDecision::WrongBase, !base);
    assert_eq!(decision == PatchDecision::WrongOriginal, base && !original);
    assert_eq!(
        decision == PatchDecision::Conflict,
        base && original && conflict
    );
}

#[kani::proof]
fn match_decision_preserves_error_priority_and_checked_span() {
    let length: usize = kani::any();
    let first: Option<usize> = kani::any();
    let repeated: bool = kani::any();
    let result = match_range(length, first, repeated);
    let expected = if length == 0 {
        Err(TextEditError::EmptyOriginal)
    } else if let Some(start) = first {
        if repeated {
            Err(TextEditError::AmbiguousOriginal)
        } else if let Some(end) = start.checked_add(length) {
            Ok(start..end)
        } else {
            Err(TextEditError::TooLong)
        }
    } else {
        Err(TextEditError::MissingOriginal)
    };
    assert_eq!(result, expected);
    kani::cover!(length > 0 && first == Some(0) && !repeated);
    kani::cover!(length == 0 && repeated);
    kani::cover!(length == usize::MAX && first == Some(1) && !repeated);
}

#[kani::proof]
fn replacement_plan_checks_every_span_and_output_length() {
    let length: usize = kani::any();
    let start: usize = kani::any();
    let end: usize = kani::any();
    let inserted: usize = kani::any();
    let result = plan_replacement(length, start..end, inserted);
    if start > end || end > length {
        assert!(matches!(result, Err(TextEditError::InvalidSpan)));
    } else {
        let remaining = start + (length - end);
        let admitted = remaining <= 33_554_432 && inserted <= 33_554_432 - remaining;
        assert_eq!(result.is_ok(), admitted);
        if let Ok(plan) = result {
            assert_eq!(plan.span, start..end);
            assert_eq!(plan.output_length, remaining + inserted);
        }
    }
    kani::cover!(start == 0 && end == length && inserted == 0);
    kani::cover!(start > end);
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_preserves_surrounding_utf8() {
    assert_eq!(
        replace_unique("甲乙丙", "乙", "新").as_deref(),
        Ok("甲新丙")
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_refuses_repeated_originals() {
    assert_eq!(
        replace_unique("甲甲", "甲", "新"),
        Err(TextEditError::AmbiguousOriginal)
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_refuses_missing_originals() {
    assert_eq!(
        replace_unique("乙", "甲", "新"),
        Err(TextEditError::MissingOriginal)
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_refuses_overlapping_originals() {
    assert_eq!(
        replace_unique("甲甲甲", "甲甲", "新"),
        Err(TextEditError::AmbiguousOriginal)
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_refuses_empty_originals() {
    assert_eq!(
        replace_unique("甲", "", "新"),
        Err(TextEditError::EmptyOriginal)
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_removes_a_prefix() {
    assert_eq!(replace_unique("甲乙", "甲", "").as_deref(), Ok("乙"));
}

#[kani::proof]
#[kani::unwind(16)]
fn text_replacement_expands_a_suffix() {
    assert_eq!(
        replace_unique("甲乙", "乙", "新規").as_deref(),
        Ok("甲新規")
    );
}

#[kani::proof]
fn replacement_length_rejects_overflow_and_limit_excess() {
    let remaining: usize = kani::any();
    let inserted: usize = kani::any();
    let admitted = remaining <= 33_554_432 && inserted <= 33_554_432 - remaining;
    assert_eq!(replacement_length(remaining, inserted).is_ok(), admitted);
    if let Ok(length) = replacement_length(remaining, inserted) {
        assert_eq!(length, remaining + inserted);
        assert!(length <= 33_554_432);
    }
    kani::cover!(remaining == 0 && inserted == 0);
    kani::cover!(remaining == usize::MAX && inserted == 1);
}

#[cfg(feature = "proof-negative-control")]
#[kani::proof]
fn false_claim_that_an_applicable_patch_has_the_wrong_base() {
    kani::assert(
        patch_decision(true, true, false) == PatchDecision::WrongBase,
        "The negative control claims that an applicable patch has the wrong base",
    );
}

#[kani::proof]
fn same_day_order_requires_complete_unique_sequences() {
    let revisions: usize = kani::any();
    let sequences: usize = kani::any();
    let expected = match revisions {
        0 => sequences == 0,
        1 => sequences <= 1,
        _ => sequences == revisions,
    };
    assert_eq!(enforcement_group_is_ordered(revisions, sequences), expected);
    kani::cover!(revisions == 1 && sequences == 0);
    kani::cover!(revisions == usize::MAX && sequences == usize::MAX);
}

#[kani::proof]
fn decimal_digits_match_ascii_and_fullwidth_codepoints() {
    let character: char = kani::any();
    let code = character as u32;
    let expected = match code {
        0x30..=0x39 => Some((code - 0x30) as u8),
        0xff10..=0xff19 => Some((code - 0xff10) as u8),
        _ => None,
    };
    assert_eq!(decimal_digit(character), expected);
    kani::cover!(character == '0');
    kani::cover!(character == '９');
    kani::cover!(character == 'a');
}

#[kani::proof]
fn decimal_accumulation_requires_a_digit_and_an_exact_u32_result() {
    let number: u32 = kani::any();
    let digit: u8 = kani::any();
    let mathematical = u64::from(number) * 10 + u64::from(digit);
    let expected = if digit < 10 && mathematical <= u64::from(u32::MAX) {
        Some(mathematical as u32)
    } else {
        None
    };
    assert_eq!(append_decimal_digit(number, digit), expected);
    kani::cover!(number == u32::MAX / 10 && digit == 5);
    kani::cover!(number == u32::MAX / 10 && digit == 6);
    kani::cover!(number == 0 && digit == u8::MAX);
}
