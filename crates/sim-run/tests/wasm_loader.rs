#![cfg(feature = "wasm")]

use std::{fs, path::PathBuf, process::Command};

use sim_kernel::{AbiVersion, LibTarget, Symbol};
use sim_wasm_abi::{WasmExport, WasmManifest, encode_exports_frame, encode_manifest_frame};

#[test]
fn wasm_feature_loads_fixture_module_through_cli() {
    let fixture = write_wasm_fixture();

    let output = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("--load")
        .arg(format!("path:{}", fixture.display()))
        .arg("--list")
        .output()
        .expect("run sim --load path:fixture.wasm --list");

    assert!(
        output.status.success(),
        "sim --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("sim --list stdout should be utf-8");
    assert!(stdout.contains("loaded libs:"), "{stdout}");
    assert!(stdout.contains("lib=fixture/wasm"), "{stdout}");
    assert!(stdout.contains("requested=path:"), "{stdout}");
    assert!(stdout.contains("resolved=path:"), "{stdout}");
    assert!(stdout.contains("exports=1"), "{stdout}");
    assert!(output.stderr.is_empty());

    let _ = fs::remove_file(fixture);
}

fn write_wasm_fixture() -> PathBuf {
    let path = unique_wasm_path();
    fs::write(&path, wasm_fixture_bytes()).expect("write wasm fixture");
    path
}

fn wasm_fixture_bytes() -> Vec<u8> {
    let exports = vec![WasmExport::Value {
        symbol: Symbol::qualified("fixture", "value"),
    }];
    let manifest = WasmManifest {
        id: Symbol::qualified("fixture", "wasm"),
        version: "0.1.0".to_owned(),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::WasmComponent,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: exports.clone(),
    };
    let manifest_frame = encode_manifest_frame(&manifest).unwrap();
    let exports_frame = encode_exports_frame(&exports).unwrap();
    wat::parse_str(format!(
        r#"(module
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 2048))
            (data (i32.const 0) "{}")
            (data (i32.const 1024) "{}")
            (func (export "sim_alloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.tee $ptr
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)
            (func (export "sim_manifest") (result i64)
                i64.const {})
            (func (export "sim_exports") (result i64)
                i64.const {})
            (func (export "sim_call") (param i32) (param i32) (param i32) (param i32) (result i64)
                i64.const 0)
        )"#,
        wat_bytes(manifest_frame.bytes()),
        wat_bytes(exports_frame.bytes()),
        pack_frame_ref(0, manifest_frame.bytes().len()),
        pack_frame_ref(1024, exports_frame.bytes().len()),
    ))
    .expect("hand-written wasm fixture should assemble")
}

fn pack_frame_ref(ptr: u32, len: usize) -> u64 {
    ((len as u64) << 32) | ptr as u64
}

fn wat_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn unique_wasm_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-run-wasm-fixture-{}-{nanos}.wasm",
        std::process::id()
    ))
}
