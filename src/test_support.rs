//! External tests are explicitly ignored by default and fail if run unconfigured.
pub fn required_env(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!("Set {name} before running this external test; see tools/test-integration.ps1")
        })
}
