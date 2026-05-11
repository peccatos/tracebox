use crate::evidence::manifest::EnvVar;

/// Environment capture is default-deny.
///
/// Blindly persisting all environment variables would leak:
///
/// - API keys;
/// - cloud credentials;
/// - SSH material;
/// - OAuth tokens;
/// - CI secrets.
///
/// Keep the default list intentionally small. Future CLI/config can add
/// explicit `--env KEY` capture without changing the manifest schema.
const ENV_ALLOWLIST: &[&str] = &[
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "PATH",
    "PWD",
    "HOME",
    "LANG",
    "LC_ALL",
    "RUSTUP_TOOLCHAIN",
    "CARGO_HOME",
    "RUST_BACKTRACE",
];

pub fn collect_env() -> Vec<EnvVar> {
    let mut vars = ENV_ALLOWLIST
        .iter()
        .filter_map(|key| {
            std::env::var(key).ok().map(|value| EnvVar {
                key: (*key).to_string(),
                value,
            })
        })
        .collect::<Vec<_>>();

    // Stable ordering helps future diffing and hashing.
    vars.sort_by(|a, b| a.key.cmp(&b.key));
    vars
}
