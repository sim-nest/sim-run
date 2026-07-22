use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread,
};

use crate::crates_io::ARTIFACT_FILE;

use super::*;

#[test]
fn fetches_matching_version_and_caches_artifact() {
    let artifact_bytes = b"artifact";
    let server = FixtureServer::start([
        (
            "/packages/sim-codec-lisp/index.txt",
            [
                index_row("0.0.9", ARTIFACT_FILE, b"old-artifact"),
                index_row("0.1.0", ARTIFACT_FILE, artifact_bytes),
                index_row("0.2.0", ARTIFACT_FILE, b"new-artifact"),
            ]
            .join("\n")
            .into_bytes(),
        ),
        (
            "/packages/sim-codec-lisp/0.1.0/artifact.simlib",
            artifact_bytes.to_vec(),
        ),
    ]);
    let cache = temp_cache("git-registry-fetch");
    let resolver = GitRegistryResolver::new(server.endpoint(), cache.clone()).unwrap();

    let resolved = resolver
        .resolve(&"sim-codec-lisp@^0.1".parse().unwrap())
        .unwrap();

    assert_eq!(resolved.version, "0.1.0");
    assert_eq!(fs::read(&resolved.artifact).unwrap(), artifact_bytes);
    assert!(resolved.artifact.starts_with(&cache));
    assert!(
        resolved
            .artifact
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("sha256-")
    );
    server.join();
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn rejects_tampered_fetch_and_does_not_cache_it() {
    let expected_bytes = b"artifact";
    let server = FixtureServer::start([
        (
            "/packages/sim-codec-lisp/index.txt",
            index_row("0.1.0", ARTIFACT_FILE, expected_bytes).into_bytes(),
        ),
        (
            "/packages/sim-codec-lisp/0.1.0/artifact.simlib",
            b"tampered".to_vec(),
        ),
    ]);
    let cache = temp_cache("git-registry-tampered-fetch");
    let resolver = GitRegistryResolver::new(server.endpoint(), cache.clone()).unwrap();

    let err = resolver
        .resolve(&"sim-codec-lisp@0.1.0".parse().unwrap())
        .unwrap_err()
        .to_string();

    assert!(err.contains("hash mismatch"), "unexpected error: {err}");
    assert!(
        !verified_cache_artifact_path(
            &cache,
            "sim-codec-lisp",
            &IndexedArtifact {
                version: "0.1.0".to_owned(),
                file_name: ARTIFACT_FILE.to_owned(),
                sha256: sha256(expected_bytes),
            },
        )
        .exists()
    );
    server.join();
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn rejects_poisoned_cache_on_reuse() {
    let expected_bytes = b"artifact";
    let selected = IndexedArtifact {
        version: "0.1.0".to_owned(),
        file_name: ARTIFACT_FILE.to_owned(),
        sha256: sha256(expected_bytes),
    };
    let cache = temp_cache("git-registry-poisoned-cache");
    let artifact = verified_cache_artifact_path(&cache, "sim-codec-lisp", &selected);
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"poisoned").unwrap();
    let server = FixtureServer::start([(
        "/packages/sim-codec-lisp/index.txt",
        index_row("0.1.0", ARTIFACT_FILE, expected_bytes).into_bytes(),
    )]);
    let resolver = GitRegistryResolver::new(server.endpoint(), cache.clone()).unwrap();

    let err = resolver
        .resolve(&"sim-codec-lisp@0.1.0".parse().unwrap())
        .unwrap_err()
        .to_string();

    assert!(err.contains("hash mismatch"), "unexpected error: {err}");
    assert_eq!(fs::read(&artifact).unwrap(), b"poisoned");
    server.join();
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn rejects_implicit_or_https_endpoint() {
    assert_eq!(
        GitRegistryResolver::new("", PathBuf::new())
            .unwrap_err()
            .to_string(),
        "git registry endpoint is empty"
    );
    assert_eq!(
        GitRegistryResolver::new("https://example.invalid/sim", PathBuf::new())
            .unwrap_err()
            .to_string(),
        "git registry endpoint must use http:// in this build"
    );
}

#[test]
fn rejects_non_loopback_insecure_endpoint() {
    let err = GitRegistryResolver::new("http://forge.example/sim", PathBuf::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("is not loopback"), "unexpected error: {err}");
}

#[test]
fn registry_rejects_overflowing_chunk_size() {
    let err = decode_git_registry_chunked(b"fffffffffffffffe\r\n", usize::MAX)
        .expect_err("overflowing chunk size must error, not panic")
        .to_string();
    assert!(err.contains("overflow"), "unexpected error: {err}");
}

#[test]
fn registry_rejects_oversized_content_length() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n";
    let err = parse_http_response("http://loopback/index.txt", response, 16)
        .expect_err("body length past the cap must error")
        .to_string();
    assert!(err.contains("exceeds 16 bytes"), "unexpected error: {err}");
}

#[test]
fn registry_caps_chunked_decoded_length() {
    let err = decode_git_registry_chunked(b"8\r\nAAAAAAAA\r\n0\r\n", 4)
        .expect_err("decoded length past the cap must error")
        .to_string();
    assert!(err.contains("exceeds 4 bytes"), "unexpected error: {err}");
}

#[test]
fn rejects_unsafe_artifact_file_names() {
    assert_eq!(
        index_artifact(&format!(
            "0.1.0 ../artifact.simlib {}",
            sha256_hex(&sha256(b"artifact"))
        ))
        .unwrap_err()
        .to_string(),
        "git registry artifact file name is not a safe path component: ../artifact.simlib"
    );
}

#[test]
fn rejects_index_rows_without_a_hash() {
    assert_eq!(
        index_artifact("0.1.0 artifact.simlib")
            .unwrap_err()
            .to_string(),
        "git registry index row must use: <version> <file-name> <sha256-hex>"
    );
}

struct FixtureServer {
    endpoint: String,
    handle: thread::JoinHandle<()>,
}

impl FixtureServer {
    fn start<const N: usize>(routes: [(&'static str, Vec<u8>); N]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let routes = routes
            .into_iter()
            .map(|(path, body)| (path.to_owned(), body))
            .collect::<BTreeMap<_, _>>();
        let handle = thread::spawn(move || {
            for _ in 0..N {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap();
                let Some(body) = routes.get(path) else {
                    let response = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    stream.write_all(response).unwrap();
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

fn temp_cache(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-run-core-git-registry-cache-{}-{label}-{nanos}",
        std::process::id(),
    ))
}

fn index_row(version: &str, file_name: &str, bytes: &[u8]) -> String {
    format!("{version} {file_name} {}", sha256_hex(&sha256(bytes)))
}
