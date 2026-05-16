pub fn tracebox_mode() -> String {
    std::env::var("TRACEBOX_MODE").unwrap_or_default()
}
