use std::process::Command;

#[test]
fn published_binary_runs_jvm_product_specimen() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("jvm")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("static=14 object=true array=17"));
    assert!(stdout.contains("exception=java/lang/NegativeArraySizeException"));
    assert!(stdout.contains("concat=SIM JVM"));
}
