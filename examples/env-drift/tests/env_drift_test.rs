use tracebox_env_drift_demo::tracebox_mode;

#[test]
fn depends_on_tracebox_mode() {
    let mode = tracebox_mode();
    assert_eq!(mode, "stable", "TRACEBOX_MODE drift detected");
}
