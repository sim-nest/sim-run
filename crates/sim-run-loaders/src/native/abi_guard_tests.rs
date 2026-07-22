use super::{copy_owned_bytes, native_manifest};
use sim_kernel::{AbiVersion, Error, LibManifest, LibTarget, NativeAbiOwnedBytes, Symbol, Version};

fn manifest_with_target(target: LibTarget) -> LibManifest {
    LibManifest {
        id: Symbol::new("demo"),
        version: Version("0.1.0".to_owned()),
        abi: AbiVersion { major: 1, minor: 0 },
        target,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: Vec::new(),
    }
}

#[test]
fn empty_owned_bytes_do_not_build_null_slice() {
    assert_eq!(
        copy_owned_bytes("test", NativeAbiOwnedBytes::empty()).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn null_pointer_with_positive_length_is_rejected() {
    let bogus = NativeAbiOwnedBytes {
        ptr: std::ptr::null_mut(),
        len: 8,
        cap: 8,
    };
    let err = copy_owned_bytes("test", bogus).expect_err("null ptr with len must be rejected");
    assert!(matches!(err, Error::HostError(m) if m.contains("invalid native byte buffer")));
}

#[test]
fn length_beyond_capacity_is_rejected() {
    // A dangling non-null pointer: never dereferenced because len > cap fails
    // the guard before any slice is constructed.
    let mut byte = 0u8;
    let bogus = NativeAbiOwnedBytes {
        ptr: &mut byte,
        len: 64,
        cap: 1,
    };
    let err = copy_owned_bytes("test", bogus).expect_err("len past cap must be rejected");
    assert!(matches!(err, Error::HostError(m) if m.contains("invalid native byte buffer")));
}

#[test]
fn native_manifest_forces_native_target_over_guest_host_registered() {
    // A guest `.so` cannot keep a trusted `host-registered` label: the loader
    // rewrites the target to Native so trust comes from loader authority.
    let manifest = native_manifest(manifest_with_target(LibTarget::HostRegistered)).unwrap();
    assert_eq!(manifest.target, LibTarget::Native);
}

#[test]
fn native_manifest_accepts_native_target() {
    let manifest = native_manifest(manifest_with_target(LibTarget::Native)).unwrap();
    assert_eq!(manifest.target, LibTarget::Native);
}
