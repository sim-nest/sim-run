#![cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]

mod support;

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{
    Cx, DefaultFactory, EncodeOptions, Expr, LoaderRegistry, NoopEvalPolicy, NumberLiteral,
    QuoteMode, ReadPolicy, Symbol, native_dynamic_load_capability,
};

use support::{
    FeatureBuildContext, cargo_bin, maybe_feature_build_context, remove_dir_all_if_exists,
    unique_target_dir,
};

const DIRECT_LISP_ROUND_TRIP: &str = "(quote native-codec-loaded)";
const LISP_CODEC_SOURCE: (&str, &str, &str) =
    ("sim-codec-lisp", "sim-codecs", "crates/sim-codec-lisp");
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
    (
        "sim-lib-numbers-core",
        "sim-numbers",
        "crates/sim-lib-numbers-core",
    ),
    (
        "sim-lib-numbers-f64",
        "sim-numbers",
        "crates/sim-lib-numbers-f64",
    ),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];
const LISP_CODEC_REQUIRED_SOURCES: &[(&str, &str, &str)] = &[
    LISP_CODEC_SOURCE,
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
    (
        "sim-lib-numbers-core",
        "sim-numbers",
        "crates/sim-lib-numbers-core",
    ),
    (
        "sim-lib-numbers-f64",
        "sim-numbers",
        "crates/sim-lib-numbers-f64",
    ),
    ("sim-macros", "sim-foundation", "crates/sim-macros"),
    ("sim-shape", "sim-shape", "."),
    ("sim-value", "sim-foundation", "crates/sim-value"),
];

#[test]
fn native_lisp_codec_loads_and_decodes_through_cli_loader() {
    let Some(context) = maybe_feature_build_context(LISP_CODEC_REQUIRED_SOURCES) else {
        return;
    };
    let plugin_path = build_lisp_codec_dylib(&context);
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
        .load_and_register(&mut cx, sim_run_loaders::path_source(plugin_path.clone()))
        .expect("native loader should register codec/lisp");

    let codec = Symbol::qualified("codec", "lisp");
    let decoded = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text(DIRECT_LISP_ROUND_TRIP.to_owned()),
        ReadPolicy::default(),
    )
    .expect("loaded Lisp codec should decode a direct expression");
    assert_eq!(
        decoded,
        Expr::Quote {
            mode: QuoteMode::Quote,
            expr: Box::new(Expr::Symbol(Symbol::new("native-codec-loaded"))),
        }
    );

    let numeric = decode_with_codec(
        &mut cx,
        &codec,
        Input::Text("(math/add 1 2)".to_owned()),
        ReadPolicy::default(),
    )
    .expect("loaded Lisp codec should decode numeric literals");
    assert_eq!(
        numeric,
        Expr::List(vec![
            Expr::Symbol(Symbol::qualified("math", "add")),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1".to_owned(),
            }),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "2".to_owned(),
            }),
        ])
    );

    let encoded = encode_with_codec(&mut cx, &codec, &decoded, EncodeOptions::default())
        .expect("loaded Lisp codec should encode")
        .into_text()
        .expect("Lisp codec output should be text");
    assert_eq!(encoded, DIRECT_LISP_ROUND_TRIP);

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

#[test]
fn native_lisp_recipe_fixture_keeps_cli_entrypoint_envelope() {
    let Some(context) = maybe_feature_build_context(LISP_CODEC_REQUIRED_SOURCES) else {
        return;
    };
    let plugin_path = build_lisp_codec_dylib(&context);
    let target_dir = plugin_path
        .parent()
        .and_then(Path::parent)
        .expect("dylib should live in target/<profile>")
        .to_owned();

    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.grant(native_dynamic_load_capability());
    LoaderRegistry::new()
        .with_loader(sim_run_loaders::NativeDylibLoader)
        .load_and_register(&mut cx, sim_run_loaders::path_source(plugin_path))
        .expect("native loader should register codec/lisp");

    let codec = Symbol::qualified("codec", "lisp");
    let script = std::fs::read_to_string(lisp_codec_recipe_setup(&context))
        .expect("loadable Lisp codec recipe setup should be readable");
    let decoded = decode_with_codec(&mut cx, &codec, Input::Text(script), ReadPolicy::default())
        .expect("loaded Lisp codec should decode the recipe entrypoint fixture");

    let Expr::List(items) = decoded else {
        panic!("recipe setup fixture should decode to a cli/main entry form");
    };
    assert_eq!(
        items.first(),
        Some(&Expr::Symbol(Symbol::qualified("cli/main", "codec-lisp")))
    );
    let Some(Expr::Map(entries)) = items.get(1) else {
        panic!("recipe setup fixture should carry an option map");
    };
    assert!(entries.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("eval"))
            && value == &Expr::String(DIRECT_LISP_ROUND_TRIP.to_owned())
    }));
    assert!(entries.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("args")) && value == &Expr::List(Vec::new())
    }));
    assert!(entries.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("script")) && value == &Expr::Nil
    }));
    assert!(
        entries.iter().any(|(key, value)| {
            key == &Expr::Symbol(Symbol::new("stdin")) && value == &Expr::Nil
        })
    );

    remove_dir_all_if_exists(&target_dir);
}

#[cfg(feature = "registry")]
#[test]
fn native_lisp_codec_loads_from_git_registry_symbol() {
    let Some(context) = maybe_feature_build_context(LISP_CODEC_REQUIRED_SOURCES) else {
        return;
    };
    let plugin_path = build_lisp_codec_dylib(&context);
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
    let index =
        registry_index_row("0.1.0", lisp_codec_dylib_file_name(), &plugin_bytes).into_bytes();
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

fn build_lisp_codec_dylib(context: &FeatureBuildContext) -> PathBuf {
    let target_dir = unique_target_dir("lisp-native-codec");
    let mut command = Command::new(cargo_bin());
    context.configure_build(
        &mut command,
        "sim-codec-lisp",
        LISP_CODEC_SOURCE,
        LISP_CODEC_PATCHES,
    );
    command
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
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

fn lisp_codec_recipe_setup(context: &FeatureBuildContext) -> PathBuf {
    context
        .source_path(
            LISP_CODEC_SOURCE.0,
            LISP_CODEC_SOURCE.1,
            LISP_CODEC_SOURCE.2,
        )
        .join("recipes/02-loadable/native-script/setup.siml")
}

#[cfg(feature = "registry")]
fn unique_cache_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("sim-run-cache-{label}-{nanos}"))
}

#[cfg(feature = "registry")]
fn registry_index_row(version: &str, file_name: &str, bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{version} {file_name} {hex}")
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

#[cfg(feature = "registry")]
struct FixtureServer {
    endpoint: String,
    handle: std::thread::JoinHandle<()>,
}

#[cfg(feature = "registry")]
impl FixtureServer {
    fn start<const N: usize>(routes: [(String, Vec<u8>); N]) -> Self {
        use std::{
            collections::{BTreeMap, BTreeSet},
            io::{ErrorKind, Read, Write},
            net::TcpListener,
            thread,
            time::{Duration, Instant},
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let routes = routes.into_iter().collect::<BTreeMap<_, _>>();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let quiet_period = Duration::from_millis(250);
            let mut seen = BTreeSet::new();
            let mut last_activity = Instant::now();
            while Instant::now() < deadline {
                if seen.len() == routes.len() && last_activity.elapsed() >= quiet_period {
                    break;
                }
                let (mut stream, _) = match listener.accept() {
                    Ok(accepted) => accepted,
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(err) => panic!("fixture server accept failed: {err}"),
                };
                let mut request = [0_u8; 2048];
                let size = match stream.read(&mut request) {
                    Ok(size) => size,
                    Err(err)
                        if matches!(
                            err.kind(),
                            ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof
                        ) =>
                    {
                        continue;
                    }
                    Err(err) => panic!("fixture server read failed: {err}"),
                };
                if size == 0 {
                    continue;
                }
                let request = String::from_utf8_lossy(&request[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap();
                let Some(body) = routes.get(path) else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(response.as_bytes()).is_err() || stream.write_all(body).is_err()
                {
                    continue;
                }
                seen.insert(path.to_owned());
                last_activity = Instant::now();
            }
            assert!(
                seen.len() == routes.len(),
                "fixture server missed routes: {:?}",
                routes
                    .keys()
                    .filter(|path| !seen.contains(*path))
                    .cloned()
                    .collect::<Vec<_>>()
            );
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
