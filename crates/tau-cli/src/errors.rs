use std::path::Path;

pub fn parse_config(path: &Path, err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("parse {}: {err}", path.display())
}

pub fn missing_any_env(vars: &[&str]) -> anyhow::Error {
    anyhow::anyhow!("{} is required", vars.join(" or "))
}

pub fn init_logging(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("initialize logging: {err}")
}
