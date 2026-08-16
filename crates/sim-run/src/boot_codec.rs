use sim_kernel::{
    AbiVersion, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Result, Symbol, Version,
};

pub(crate) const BOOT_CODEC_HOST: &str = "codec/lisp";

/// Minimal codec marker used while a built-in product verb receives CLI args.
pub(crate) struct BootCodec;

impl Lib for BootCodec {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("codec", "lisp"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: Symbol::qualified("codec", "lisp"),
                codec_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.codec_value(Symbol::qualified("codec", "lisp"), cx.factory().bool(true)?)?;
        Ok(())
    }
}
