use std::process::Command;

// conformance: physics selection loads the runtime library through the shared bootloader.

#[test]
fn physics_selection_registers_the_runtime_library() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .args(["physics", "browse"])
        .output()
        .expect("run sim physics browse");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("loaded libs: codec/lisp, sim/physics"),
        "{stderr}"
    );
    assert!(
        stderr.contains("no loaded lib claims cli/main/physics or cli/main"),
        "{stderr}"
    );
}
