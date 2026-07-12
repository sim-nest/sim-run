use std::{fs, process::Command};

#[test]
fn config_flags_do_not_change_minimal_boot_set() {
    let root = std::env::temp_dir().join(format!("sim-run-cli-config-{}", std::process::id()));
    let config_file = root.join("sim.toml");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        &config_file,
        r#"[sim/cookbook]

[[sim/cookbook.loadable_lib]]
id = "numbers/cas"
source = "symbol:numbers/cas"
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("--config-home")
        .arg(root.join("home"))
        .arg("--config-work")
        .arg(root.join("work"))
        .arg("--config-file")
        .arg(&config_file)
        .arg("run")
        .output()
        .expect("run sim with config flags");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("sim: no codec 'lisp' available")
    );

    let _ = fs::remove_dir_all(root);
}
