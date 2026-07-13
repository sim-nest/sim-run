#![cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{
    Cx, DefaultFactory, EncodeOptions, Expr, LibSource, LoaderRegistry, NoopEvalPolicy, QuoteMode,
    ReadPolicy, Symbol, native_dynamic_load_capability,
};

const LISP_CODEC_PATCHES: &[(&str, &str, &str)] = &[
    ("sim-nest", "sim-sdk", "."),
    ("sim-citizen", "sim-citizen", "crates/sim-citizen"),
    (
        "sim-citizen-derive",
        "sim-citizen",
        "crates/sim-citizen-derive",
    ),
    ("sim-codec", "sim-codecs", "crates/sim-codec"),
    ("sim-codec-binary", "sim-codecs", "crates/sim-codec-binary"),
    ("sim-cookbook", "sim-foundation", "crates/sim-cookbook"),
    ("sim-kernel", "sim-kernel", "."),
    ("sim-lib-core", "sim-runtime", "crates/sim-lib-core"),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];

#[test]
fn native_lisp_codec_loads_and_decodes_through_cli_loader() {
    let plugin_path = build_lisp_codec_dylib();
    assert!(
        plugin_path.is_file(),
        "missing Lisp codec dylib {plugin_path:?}"
    );
    let target_dir = plugin_path
        .parent()
        .and_then(Path::parent)
        .expect("dylib should live in target/<profile>")
        .to_owned();

    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.grant(native_dynamic_load_capability());
    LoaderRegistry::new()
        .with_loader(sim_run_loaders::NativeDylibLoader)
        .load_and_register(&mut cx, LibSource::Path(plugin_path.clone()))
        .expect("native loader should register codec/lisp");

    let codec = Symbol::qualified("codec", "lisp");
    let script = std::fs::read_to_string(lisp_codec_recipe_setup())
        .expect("loadable Lisp codec recipe setup should be readable");
    let decoded = decode_with_codec(&mut cx, &codec, Input::Text(script), ReadPolicy::default())
        .expect("loaded Lisp codec should decode a script");
    assert_eq!(
        decoded,
        Expr::Quote {
            mode: QuoteMode::Quote,
            expr: Box::new(Expr::Symbol(Symbol::new("native-codec-loaded"))),
        }
    );

    let encoded = encode_with_codec(&mut cx, &codec, &decoded, EncodeOptions::default())
        .expect("loaded Lisp codec should encode")
        .into_text()
        .expect("Lisp codec output should be text");
    assert_eq!(encoded, "(quote native-codec-loaded)");

    let list = Command::new(env!("CARGO_BIN_EXE_sim"))
        .arg("--load")
        .arg(format!("path:{}", plugin_path.display()))
        .arg("--list")
        .output()
        .expect("run sim --load path:libsim_codec_lisp --list");
    assert!(
        list.status.success(),
        "sim --list failed: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let stdout = String::from_utf8(list.stdout).expect("sim --list stdout should be utf-8");
    assert!(stdout.contains("lib=codec/lisp"), "{stdout}");
    assert!(stdout.contains("exports=1"), "{stdout}");
    assert!(list.stderr.is_empty());

    remove_dir_all_if_exists(&target_dir);
}

#[cfg(feature = "registry")]
#[test]
fn native_lisp_codec_loads_from_git_registry_symbol() {
    let plugin_path = build_lisp_codec_dylib();
    assert!(
        plugin_path.is_file(),
        "missing Lisp codec dylib {plugin_path:?}"
    );
    let target_dir = plugin_path
        .parent()
        .and_then(Path::parent)
        .expect("dylib should live in target/<profile>")
        .to_owned();
    let plugin_bytes = std::fs::read(&plugin_path).expect("Lisp codec dylib should be readable");
    let index = format!("0.1.0 {}\n", lisp_codec_dylib_file_name()).into_bytes();
    let artifact_route = format!(
        "/packages/sim-codec-lisp/0.1.0/{}",
        lisp_codec_dylib_file_name()
    );
    let server = FixtureServer::start([
        ("/packages/sim-codec-lisp/index.txt".to_owned(), index),
        (artifact_route, plugin_bytes),
    ]);
    let cache = unique_cache_dir("lisp-registry");

    let list = Command::new(env!("CARGO_BIN_EXE_sim"))
        .env(sim_run_core::GIT_REGISTRY_ENDPOINT_ENV, server.endpoint())
        .env("SIM_CLI_CACHE_DIR", &cache)
        .arg("--load")
        .arg("symbol:codec/lisp")
        .arg("--list")
        .output()
        .expect("run sim --load symbol:codec/lisp --list");

    assert!(
        list.status.success(),
        "sim --list failed: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let stdout = String::from_utf8(list.stdout).expect("sim --list stdout should be utf-8");
    assert!(stdout.contains("lib=codec/lisp"), "{stdout}");
    assert!(
        stdout.contains("requested=crates.io:sim-codec-lisp@^0.1"),
        "{stdout}"
    );
    assert!(stdout.contains("resolved=path:"), "{stdout}");
    assert!(list.stderr.is_empty());

    server.join();
    remove_dir_all_if_exists(&cache);
    remove_dir_all_if_exists(&target_dir);
}

fn build_lisp_codec_dylib() -> PathBuf {
    let target_dir = unique_target_dir();
    let mut command = Command::new(cargo_bin());
    command.env("RUSTFLAGS", "-D warnings").arg("build");
    if let Some(meta_manifest) = meta_workspace_manifest() {
        command
            .arg("--manifest-path")
            .arg(meta_manifest)
            .arg("-p")
            .arg("sim-codec-lisp");
    } else {
        command.arg("--manifest-path").arg(
            package_path("sim-codec-lisp", "sim-codecs", "crates/sim-codec-lisp")
                .join("Cargo.toml"),
        );
        add_lisp_codec_patch_args(&mut command);
    }
    command
        .arg("--features")
        .arg("native-export")
        .arg("--target-dir")
        .arg(&target_dir);

    let status = command
        .status()
        .expect("cargo build for Lisp codec dylib should start");
    assert!(status.success(), "Lisp codec dylib build failed");
    target_dir.join("debug").join(lisp_codec_dylib_file_name())
}

fn meta_workspace_manifest() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "packages")
    {
        return manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("Cargo.toml"));
    }
    None
}

fn add_lisp_codec_patch_args(command: &mut Command) {
    for (crate_name, repo_name, source_path) in LISP_CODEC_PATCHES {
        let path = package_path(crate_name, repo_name, source_path);
        command.arg("--config").arg(format!(
            "patch.crates-io.{crate_name}.path={}",
            toml_string(&path)
        ));
    }
}

fn lisp_codec_recipe_setup() -> PathBuf {
    package_path("sim-codec-lisp", "sim-codecs", "crates/sim-codec-lisp")
        .join("recipes/02-loadable/native-script/setup.siml")
}

fn package_path(crate_name: &str, repo_name: &str, source_path: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "packages")
    {
        return manifest_dir
            .parent()
            .expect("meta-workspace package should have a packages parent")
            .join(crate_name);
    }

    let sim_cli_repo = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("sim-run package should live under crates/sim-run");
    if repo_name == "sim-run" {
        return sim_cli_repo.join(source_path);
    }
    sim_cli_repo
        .parent()
        .expect("sim-run checkout should have sibling repos")
        .join(repo_name)
        .join(source_path)
}

fn toml_string(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn unique_target_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("sim-lisp-native-codec-{nanos}"))
}

#[cfg(feature = "registry")]
fn unique_cache_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("sim-run-cache-{label}-{nanos}"))
}

fn lisp_codec_dylib_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "sim_codec_lisp.dll"
    }
    #[cfg(target_os = "macos")]
    {
        "libsim_codec_lisp.dylib"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "libsim_codec_lisp.so"
    }
}

fn remove_dir_all_if_exists(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[cfg(feature = "registry")]
struct FixtureServer {
    endpoint: String,
    handle: std::thread::JoinHandle<()>,
}

#[cfg(feature = "registry")]
impl FixtureServer {
    fn start<const N: usize>(routes: [(String, Vec<u8>); N]) -> Self {
        use std::{
            collections::BTreeMap,
            io::{Read, Write},
            net::TcpListener,
            thread,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let routes = routes.into_iter().collect::<BTreeMap<_, _>>();
        let handle = thread::spawn(move || {
            for _ in 0..N {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap();
                let Some(body) = routes.get(path) else {
                    stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                    continue;
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self { endpoint, handle }
    }

    fn endpoint(&self) -> String {
        self.endpoint.clone()
    }

    fn join(self) {
        self.handle.join().unwrap();
    }
}
