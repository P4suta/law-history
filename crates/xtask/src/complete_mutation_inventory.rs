pub const fn required(list_only: bool, partial_selection: bool) -> bool {
    !list_only && !partial_selection
}

#[cfg(test)]
mod tests {
    #[test]
    fn complete_evidence_includes_one_shard_and_excludes_previews_and_fragments() {
        for (list, partial, complete) in [
            (false, false, true),
            (false, true, false),
            (true, false, false),
            (true, true, false),
        ] {
            assert_eq!(super::required(list, partial), complete);
        }
    }
}
