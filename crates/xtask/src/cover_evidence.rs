#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(crate) struct CoverCounts {
    pub(crate) reached: usize,
    pub(crate) total: usize,
}

pub(crate) const fn is_complete(actual: Option<CoverCounts>, required: Option<usize>) -> bool {
    match actual {
        None => required.is_none(),
        Some(counts) => {
            counts.total > 0
                && counts.reached == counts.total
                && match required {
                    Some(required) => counts.total == required,
                    None => true,
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoverCounts, is_complete};

    #[test]
    fn cover_evidence_requires_nonempty_complete_counts_and_every_required_witness() {
        for (counts, required, expected) in [
            (None, None, true),
            (None, Some(0), false),
            (None, Some(1), false),
            (Some((0, 0)), None, false),
            (Some((0, 1)), None, false),
            (Some((1, 0)), None, false),
            (Some((1, 1)), None, true),
            (Some((1, 1)), Some(0), false),
            (Some((1, 1)), Some(1), true),
            (Some((1, 1)), Some(2), false),
            (Some((usize::MAX, usize::MAX)), Some(usize::MAX), true),
            (Some((usize::MAX - 1, usize::MAX)), Some(usize::MAX), false),
            (Some((usize::MAX, usize::MAX - 1)), None, false),
        ] {
            assert_eq!(
                is_complete(
                    counts.map(|(reached, total)| CoverCounts { reached, total }),
                    required
                ),
                expected,
                "{counts:?} {required:?}"
            );
        }
    }
}
