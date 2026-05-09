use reqwest::StatusCode;

pub fn missing_env(var: &str) -> anyhow::Error {
    anyhow::anyhow!("{var} is required")
}

pub fn missing_any_env(vars: &[&str]) -> anyhow::Error {
    anyhow::anyhow!("{} is required", vars.join(" or "))
}

pub fn request_failed(provider: &str, status: StatusCode, body: String) -> anyhow::Error {
    anyhow::anyhow!("{provider} request failed: {status} {body}")
}
