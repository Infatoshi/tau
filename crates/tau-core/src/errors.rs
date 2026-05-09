pub fn cancelled() -> anyhow::Error {
    anyhow::anyhow!("cancelled")
}

pub fn provider_error(message: String) -> anyhow::Error {
    anyhow::anyhow!(message)
}

pub fn empty_compaction_summary() -> anyhow::Error {
    anyhow::anyhow!("provider returned an empty compaction summary")
}

pub fn session_missing_header() -> anyhow::Error {
    anyhow::anyhow!("session missing header")
}

pub fn missing_home_dir() -> anyhow::Error {
    anyhow::anyhow!("could not find home directory")
}

pub fn no_session_matches(hash: &str) -> anyhow::Error {
    anyhow::anyhow!("no session matches {hash}")
}

pub fn ambiguous_session_hash(hash: &str) -> anyhow::Error {
    anyhow::anyhow!("ambiguous session hash {hash}")
}
