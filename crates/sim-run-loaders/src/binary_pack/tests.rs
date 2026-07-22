use sim_kernel::{Export, LibLoader, LibTarget, Symbol, Version};

use super::{BinaryLibPack, BinaryPackLoader, decode_binary_lib_pack, encode_binary_lib_pack};

#[test]
fn binary_pack_loader_accepts_pack_paths_and_magic_bytes() {
    let loader = BinaryPackLoader;
    assert!(loader.can_load(&crate::path_source(std::path::PathBuf::from("lib.l8b"))));
    assert!(loader.can_load(&crate::bytes_source(b"L8PKrest".to_vec())));
    assert!(!loader.can_load(&crate::url_source("https://example.com/lib.l8b")));
    assert!(!loader.can_load(&crate::bytes_source(Vec::new())));
}

#[test]
fn binary_pack_round_trips_reexport_specs() {
    let pack = BinaryLibPack {
        manifest: sim_kernel::LibManifest {
            id: Symbol::qualified("loader", "pack-demo"),
            version: Version("0.3.0".to_owned()),
            abi: sim_kernel::AbiVersion { major: 0, minor: 1 },
            target: LibTarget::DataOnly,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: Symbol::qualified("loader", "tick-pack"),
                function_id: None,
            }],
        },
        exports: vec![crate::ReexportSpec::new(
            crate::ReexportKind::Function,
            Symbol::qualified("loader", "tick-pack"),
            Symbol::new("tick"),
        )],
    };

    let decoded = decode_binary_lib_pack(&encode_binary_lib_pack(&pack).unwrap()).unwrap();

    assert_eq!(decoded, pack);
    assert_eq!(
        decoded.exports[0].export(),
        &Symbol::qualified("loader", "tick-pack")
    );
    assert_eq!(decoded.exports[0].target(), &Symbol::new("tick"));
}
