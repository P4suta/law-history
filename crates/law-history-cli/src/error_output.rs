#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureOutput {
    Json,
    Text,
}

pub const fn failure_output(json: bool, reported: bool) -> FailureOutput {
    match (json, reported) {
        (true, false) => FailureOutput::Json,
        _ => FailureOutput::Text,
    }
}
