pub const MAXIMUM_WORKERS: usize = 6;
pub const RESERVE_BYTES: u64 = 2_147_483_648;
pub const WORKER_BYTES: u64 = 1_073_741_824;

pub fn workers(available: u64, requested: usize) -> usize {
    let capacity = available.saturating_sub(RESERVE_BYTES) / WORKER_BYTES;
    capacity.min(requested.min(MAXIMUM_WORKERS) as u64) as usize
}

#[cfg(test)]
mod tests {
    #[test]
    fn capacity_matches_an_independent_worker_allocation_oracle() {
        let samples = [0, u64::MAX].into_iter().chain((0..=8).flat_map(|count| {
            let boundary = 2_147_483_648 + count * 1_073_741_824;
            [boundary - 1, boundary, boundary + 1]
        }));
        for available in samples {
            for requested in (0..=8).chain([usize::MAX]) {
                let expected = (1..=6)
                    .filter(|count| {
                        *count <= requested
                            && available >= 2_147_483_648 + *count as u64 * 1_073_741_824
                    })
                    .max()
                    .unwrap_or(0);
                assert_eq!(
                    super::workers(available, requested),
                    expected,
                    "available={available}, requested={requested}"
                );
            }
        }
    }
}
