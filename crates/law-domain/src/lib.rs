#![no_std]

extern crate alloc;

use alloc::string::String;

const MAX_REPLACEMENT_BYTES: usize = 33_554_432;
const MAX_YEAR: u16 = 9999;
const GREGORIAN_LEAP_CYCLE: u16 = 400;
const CENTURY: u16 = 100;
const LEAP_INTERVAL: u16 = 4;
const FEBRUARY: u8 = 2;
const LEAP_FEBRUARY_DAYS: u8 = 29;
const COMMON_MONTH_DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const DECIMAL_RADIX: u32 = 10;

pub const fn input_fits(size: usize, limit: usize) -> bool {
    size <= limit
}

pub const fn knowledge_cutoff_accepts(order: core::cmp::Ordering) -> bool {
    !matches!(order, core::cmp::Ordering::Greater)
}

pub const fn revision_has_changes(content: bool, repeal: bool, enforcement: bool) -> bool {
    content || repeal || enforcement
}

pub const fn inline_title_attribute_allowed(line: bool, style: bool, known_value: bool) -> bool {
    line && style && known_value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum RubyContext {
    Content,
    Ruby,
    Reading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum RubyRole {
    Content,
    Ruby,
    Reading,
}

pub const fn ruby_context(parent: RubyContext, role: RubyRole) -> RubyContext {
    match (parent, role) {
        (RubyContext::Reading, RubyRole::Content | RubyRole::Ruby | RubyRole::Reading)
        | (RubyContext::Ruby, RubyRole::Reading) => RubyContext::Reading,
        (RubyContext::Content | RubyContext::Ruby, RubyRole::Ruby) => RubyContext::Ruby,
        (RubyContext::Content, RubyRole::Content | RubyRole::Reading)
        | (RubyContext::Ruby, RubyRole::Content) => RubyContext::Content,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceChoice {
    Absent,
    First,
    Second,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationEvidence {
    NoClaim,
    KnownEnd,
    UnknownEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum TerminationDateEvidence {
    Absent,
    Uncertain,
    Known,
}

pub const fn termination_evidence(
    claimed: bool,
    date: TerminationDateEvidence,
) -> TerminationEvidence {
    match (claimed, date) {
        (_, TerminationDateEvidence::Known) => TerminationEvidence::KnownEnd,
        (false, TerminationDateEvidence::Absent) => TerminationEvidence::NoClaim,
        _ => TerminationEvidence::UnknownEnd,
    }
}

pub const fn evidence_choice(first: bool, second: bool, equal: bool) -> EvidenceChoice {
    match (first, second) {
        (false, false) => EvidenceChoice::Absent,
        (true, false) => EvidenceChoice::First,
        (false, true) => EvidenceChoice::Second,
        (true, true) if equal => EvidenceChoice::First,
        (true, true) => EvidenceChoice::Conflict,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementDateRole {
    Unspecified,
    ScheduledPlaceholder,
    DistinctValue,
}

pub const fn enforcement_date_role(present: bool, matches_schedule: bool) -> EnforcementDateRole {
    if !present {
        EnforcementDateRole::Unspecified
    } else if matches_schedule {
        EnforcementDateRole::ScheduledPlaceholder
    } else {
        EnforcementDateRole::DistinctValue
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum ProvisionRole {
    Law,
    Body,
    Main,
    Subdivision,
    Article,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum ProvisionContext {
    Start,
    Root,
    Body,
    Main,
    Article,
    Excluded,
}

pub const fn provision_context(context: ProvisionContext, role: ProvisionRole) -> ProvisionContext {
    match context {
        ProvisionContext::Start => match role {
            ProvisionRole::Law => ProvisionContext::Root,
            _ => ProvisionContext::Excluded,
        },
        ProvisionContext::Root => match role {
            ProvisionRole::Body => ProvisionContext::Body,
            ProvisionRole::Main => ProvisionContext::Main,
            _ => ProvisionContext::Excluded,
        },
        ProvisionContext::Body => match role {
            ProvisionRole::Main => ProvisionContext::Main,
            _ => ProvisionContext::Excluded,
        },
        ProvisionContext::Main => match role {
            ProvisionRole::Subdivision => ProvisionContext::Main,
            ProvisionRole::Article => ProvisionContext::Article,
            _ => ProvisionContext::Excluded,
        },
        ProvisionContext::Article | ProvisionContext::Excluded => ProvisionContext::Excluded,
    }
}

pub fn number_can_address(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes != b"0"
        && bytes.iter().all(|byte| !matches!(byte, b'/' | b'[' | b']'))
}

pub fn schema_name(bytes: &[u8]) -> bool {
    bytes
        .first()
        .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

pub const fn decimal_digit(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some((character as u32 - '0' as u32) as u8),
        '０'..='９' => Some((character as u32 - '０' as u32) as u8),
        _ => None,
    }
}

pub const fn append_decimal_digit(number: u32, digit: u8) -> Option<u32> {
    if digit as u32 >= DECIMAL_RADIX {
        return None;
    }
    match number.checked_mul(DECIMAL_RADIX) {
        Some(shifted) => shifted.checked_add(digit as u32),
        None => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEditError {
    EmptyOriginal,
    MissingOriginal,
    AmbiguousOriginal,
    TooLong,
    InvalidSpan,
}

pub fn unique_match(text: &str, old: &str) -> Result<core::ops::Range<usize>, TextEditError> {
    let first = text.find(old);
    let repeated = first
        .zip(old.chars().next())
        .is_some_and(|(start, character)| text[start + character.len_utf8()..].contains(old));
    match_range(old.len(), first, repeated)
}

fn match_range(
    length: usize,
    first: Option<usize>,
    repeated: bool,
) -> Result<core::ops::Range<usize>, TextEditError> {
    if length == 0 {
        return Err(TextEditError::EmptyOriginal);
    }
    let start = first.ok_or(TextEditError::MissingOriginal)?;
    if repeated {
        return Err(TextEditError::AmbiguousOriginal);
    }
    let end = start.checked_add(length).ok_or(TextEditError::TooLong)?;
    Ok(start..end)
}

pub fn replacement_length(remaining: usize, inserted: usize) -> Result<usize, TextEditError> {
    match remaining.checked_add(inserted) {
        Some(length) if length <= MAX_REPLACEMENT_BYTES => Ok(length),
        _ => Err(TextEditError::TooLong),
    }
}

pub fn replace_unique(text: &str, old: &str, new: &str) -> Result<String, TextEditError> {
    let plan = plan_replacement(text.len(), unique_match(text, old)?, new.len())?;
    let mut result = String::with_capacity(plan.output_length);
    result.push_str(&text[..plan.span.start]);
    result.push_str(new);
    result.push_str(&text[plan.span.end..]);
    Ok(result)
}

struct ReplacementPlan {
    span: core::ops::Range<usize>,
    output_length: usize,
}

fn plan_replacement(
    text_length: usize,
    span: core::ops::Range<usize>,
    inserted: usize,
) -> Result<ReplacementPlan, TextEditError> {
    if span.start > span.end || span.end > text_length {
        return Err(TextEditError::InvalidSpan);
    }
    let output_length = replacement_length(text_length - (span.end - span.start), inserted)?;
    Ok(ReplacementPlan {
        span,
        output_length,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDay {
    year: u16,
    month: u8,
    day: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateError {
    Year,
    Month,
    Day,
}

impl CivilDay {
    pub const fn new(year: u16, month: u8, day: u8) -> Result<Self, DateError> {
        if year == 0 || year > MAX_YEAR {
            return Err(DateError::Year);
        }
        if month == 0 || month as usize > COMMON_MONTH_DAYS.len() {
            return Err(DateError::Month);
        }
        let maximum = if month == FEBRUARY
            && (year.is_multiple_of(GREGORIAN_LEAP_CYCLE)
                || (year.is_multiple_of(LEAP_INTERVAL) && !year.is_multiple_of(CENTURY)))
        {
            LEAP_FEBRUARY_DAYS
        } else {
            COMMON_MONTH_DAYS[month as usize - 1]
        };
        if day == 0 || day > maximum {
            return Err(DateError::Day);
        }
        Ok(Self { year, month, day })
    }

    pub const fn parts(self) -> (u16, u8, u8) {
        (self.year, self.month, self.day)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveState {
    Effective,
    Future,
    Uncertain,
    Repealed,
}

pub fn effective_state(
    start: Option<CivilDay>,
    repeal: Option<CivilDay>,
    at: CivilDay,
) -> EffectiveState {
    match start {
        None => EffectiveState::Uncertain,
        Some(start) if at < start => EffectiveState::Future,
        Some(_) if repeal.is_some_and(|date| date <= at) => EffectiveState::Repealed,
        Some(_) => EffectiveState::Effective,
    }
}

pub fn current_snapshot_is_evidence(
    observed: CivilDay,
    at: CivilDay,
    matches_candidate: bool,
) -> bool {
    matches_candidate && observed <= at
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchDecision {
    Apply,
    WrongBase,
    WrongOriginal,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralChange {
    Unchanged,
    Changed,
    Moved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationChoice {
    Keep,
    Replace,
    Conflict,
}

pub const fn enforcement_group_is_ordered(revisions: usize, unique_sequences: usize) -> bool {
    unique_sequences <= revisions && (revisions <= 1 || unique_sequences == revisions)
}

pub const fn observation_choice(
    incoming: core::cmp::Ordering,
    identical: bool,
) -> ObservationChoice {
    match incoming {
        core::cmp::Ordering::Less => ObservationChoice::Keep,
        core::cmp::Ordering::Equal if identical => ObservationChoice::Keep,
        core::cmp::Ordering::Equal => ObservationChoice::Conflict,
        core::cmp::Ordering::Greater => ObservationChoice::Replace,
    }
}

pub const fn structural_change(
    attributes_equal: bool,
    text_equal: bool,
    order_equal: bool,
    members_equal: bool,
) -> StructuralChange {
    if !order_equal && members_equal {
        StructuralChange::Moved
    } else if !attributes_equal || !text_equal {
        StructuralChange::Changed
    } else {
        StructuralChange::Unchanged
    }
}

pub const fn patch_decision(
    base_matches: bool,
    original_matches: bool,
    conflict: bool,
) -> PatchDecision {
    if !base_matches {
        PatchDecision::WrongBase
    } else if !original_matches {
        PatchDecision::WrongOriginal
    } else if conflict {
        PatchDecision::Conflict
    } else {
        PatchDecision::Apply
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
#[path = "../../xtask/src/image_reference.rs"]
mod proof_image_reference;

#[cfg(kani)]
mod proofs;

#[cfg(any(test, kani))]
#[path = "../../xtask/src/complete_mutation_inventory.rs"]
mod proof_complete_mutation_inventory;

#[cfg(any(test, kani))]
#[path = "../../xtask/src/mutation_capacity.rs"]
mod proof_mutation_capacity;

#[cfg(any(test, kani))]
#[path = "../../xtask/src/cover_evidence.rs"]
mod proof_cover_evidence;

#[cfg(any(test, kani))]
#[path = "../../xtask/src/native_log.rs"]
mod proof_native_log;

#[cfg(kani)]
#[path = "../../law-history-cli/src/error_output.rs"]
mod error_output;
