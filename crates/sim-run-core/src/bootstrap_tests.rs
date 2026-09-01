#[test]
fn supplied_bootstrap_source_has_no_ambient_discovery() {
    let source = include_str!("command.rs");
    let body = source
        .split("pub fn run_supplied_bootstrap")
        .nth(1)
        .expect("bootstrap consumer")
        .split("/// Runs an already-parsed command")
        .next()
        .unwrap();
    for forbidden in [
        "parse_args",
        "current_dir",
        "std::env",
        "var_os",
        "registry",
    ] {
        assert!(!body.contains(forbidden), "ambient operation: {forbidden}");
    }
    assert!(body.contains("read_files: false"));
    assert!(body.contains("install_supplied_capsule"));
}
