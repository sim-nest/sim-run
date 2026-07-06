use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    CliError, CratesIoSpec, ResolvedCratesIoSource,
    crates_io::{ARTIFACT_FILE, cache_artifact_path_with_file, compare_versions},
};

/// Environment variable that enables git-registry-backed artifact resolution.
pub const GIT_REGISTRY_ENDPOINT_ENV: &str = "SIM_GIT_REGISTRY_ENDPOINT";

/// Environment variable that opts in to fetching from a non-loopback host over
/// insecure `http://`. Without it, only loopback endpoints are permitted, so an
/// unauthenticated cleartext fetch cannot reach a remote host by default (F8).
pub const GIT_REGISTRY_ALLOW_INSECURE_ENV: &str = "SIM_GIT_REGISTRY_ALLOW_INSECURE";

/// Maximum bytes accepted for a registry index response (F18).
const MAX_INDEX_BYTES: usize = 1 << 20; // 1 MiB
/// Maximum bytes accepted for a registry artifact response (F18).
const MAX_ARTIFACT_BYTES: usize = 64 << 20; // 64 MiB

/// Networked resolver for SIM library artifacts hosted by a git-forge package
/// registry.
///
/// This is an HTTP fetch of a prebuilt artifact from a git forge's package
/// registry (Forgejo, Gitea, GitHub, GitLab all expose one) -- not a `git
/// clone`. The vendor is not baked in: the endpoint is configured at runtime
/// (e.g. a self-hosted forge at `http://forge.example/sim`). The endpoint is
/// explicit and must use `http://`. The
/// resolver reads a text version index at `packages/<package>/index.txt`, selects
/// the newest version matching the requested requirement, fetches the named
/// artifact file, and stores it in the same cache layout used by
/// [`crate::CratesIoResolver`].
#[derive(Clone, Debug)]
pub struct GitRegistryResolver {
    endpoint: String,
    cache_dir: PathBuf,
}

impl GitRegistryResolver {
    /// Builds a resolver from a git registry artifact endpoint and cache root.
    pub fn new(endpoint: impl Into<String>, cache_dir: PathBuf) -> Result<Self, CliError> {
        let endpoint = normalize_endpoint(endpoint.into())?;
        Ok(Self {
            endpoint,
            cache_dir,
        })
    }

    /// Returns the endpoint this resolver fetches from.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Returns the cache root this resolver writes into.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Resolves a package requirement by fetching from the git registry endpoint.
    pub fn resolve(&self, spec: &CratesIoSpec) -> Result<ResolvedCratesIoSource, CliError> {
        let selected = self.select_version(spec)?;
        let artifact = cache_artifact_path_with_file(
            &self.cache_dir,
            &spec.package,
            &selected.version,
            &selected.file_name,
        );
        if !artifact.is_file() {
            let bytes = http_get(
                &self.artifact_url(&spec.package, &selected.version, &selected.file_name)?,
                MAX_ARTIFACT_BYTES,
            )?;
            let parent = artifact
                .parent()
                .ok_or_else(|| CliError::new("git registry cache artifact has no parent"))?;
            fs::create_dir_all(parent)
                .map_err(|err| CliError::new(format!("create git registry cache: {err}")))?;
            fs::write(&artifact, bytes)
                .map_err(|err| CliError::new(format!("write git registry cache: {err}")))?;
        }
        Ok(ResolvedCratesIoSource {
            requested: spec.clone(),
            package: spec.package.clone(),
            version: selected.version,
            artifact,
        })
    }

    fn select_version(&self, spec: &CratesIoSpec) -> Result<IndexedArtifact, CliError> {
        let index = http_get(&self.index_url(&spec.package)?, MAX_INDEX_BYTES)?;
        let index = String::from_utf8(index)
            .map_err(|err| CliError::new(format!("git registry index is not UTF-8: {err}")))?;
        let mut versions = Vec::new();
        for line in index.lines() {
            let Some(artifact) = index_artifact(line)? else {
                continue;
            };
            if spec.requirement.matches(&artifact.version) {
                versions.push(artifact);
            }
        }
        versions.sort_by(|left, right| compare_versions(&right.version, &left.version));
        versions.into_iter().next().ok_or_else(|| {
            CliError::new(format!(
                "git registry has no version matching {} for {}",
                spec.requirement, spec.package
            ))
        })
    }

    fn index_url(&self, package: &str) -> Result<String, CliError> {
        Ok(format!(
            "{}/packages/{}/index.txt",
            self.endpoint,
            url_path_component(package)?
        ))
    }

    fn artifact_url(
        &self,
        package: &str,
        version: &str,
        file_name: &str,
    ) -> Result<String, CliError> {
        Ok(format!(
            "{}/packages/{}/{}/{}",
            self.endpoint,
            url_path_component(package)?,
            url_path_component(version)?,
            url_path_component(file_name)?
        ))
    }
}

fn normalize_endpoint(endpoint: String) -> Result<String, CliError> {
    let endpoint = endpoint.trim().trim_end_matches('/').to_owned();
    if endpoint.is_empty() {
        return Err(CliError::new("git registry endpoint is empty"));
    }
    if !endpoint.starts_with("http://") {
        return Err(CliError::new(
            "git registry endpoint must use http:// in this build",
        ));
    }
    // F8: this build has no TLS client, so the fetch is unauthenticated
    // cleartext. Confine it to loopback by default; reaching a remote host over
    // insecure http:// requires an explicit opt-in.
    let url = HttpUrl::parse(&endpoint)?;
    if !host_is_loopback(&url.host) && !insecure_remote_allowed() {
        return Err(CliError::new(format!(
            "git registry endpoint host {} is not loopback; refusing an unauthenticated http:// \
             fetch to a remote host (set {} to override)",
            url.host, GIT_REGISTRY_ALLOW_INSECURE_ENV
        )));
    }
    Ok(endpoint)
}

fn host_is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn insecure_remote_allowed() -> bool {
    std::env::var_os(GIT_REGISTRY_ALLOW_INSECURE_ENV).is_some_and(|value| !value.is_empty())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IndexedArtifact {
    version: String,
    file_name: String,
}

fn index_artifact(line: &str) -> Result<Option<IndexedArtifact>, CliError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut parts = line.split_ascii_whitespace();
    let Some(version) = parts.next() else {
        return Ok(None);
    };
    let file_name = safe_artifact_file_name(parts.next().unwrap_or(ARTIFACT_FILE))?;
    Ok(Some(IndexedArtifact {
        version: version.to_owned(),
        file_name,
    }))
}

fn safe_artifact_file_name(file_name: &str) -> Result<String, CliError> {
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains('/')
        || file_name.contains('\\')
    {
        return Err(CliError::new(format!(
            "git registry artifact file name is not a safe path component: {file_name}"
        )));
    }
    Ok(file_name.to_owned())
}

fn url_path_component(component: &str) -> Result<String, CliError> {
    if component.is_empty() {
        return Err(CliError::new("git registry URL component is empty"));
    }
    let mut encoded = String::new();
    for byte in component.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(encoded)
}

fn http_get(url: &str, cap: usize) -> Result<Vec<u8>, CliError> {
    let parsed = HttpUrl::parse(url)?;
    let mut stream = TcpStream::connect((parsed.host.as_str(), parsed.port)).map_err(|err| {
        CliError::new(format!(
            "connect git registry {}:{}: {err}",
            parsed.host, parsed.port
        ))
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| CliError::new(format!("set git registry read timeout: {err}")))?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: sim-run-core/{}\r\nConnection: close\r\n\r\n",
        parsed.path,
        parsed.host_header(),
        env!("CARGO_PKG_VERSION")
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| CliError::new(format!("write git registry request: {err}")))?;
    // F18: bound the whole response (headers + body). `take(cap + 1)` lets us
    // detect an over-cap stream without reading it unboundedly into memory.
    let mut response = Vec::new();
    stream
        .take((cap as u64).saturating_add(1))
        .read_to_end(&mut response)
        .map_err(|err| CliError::new(format!("read git registry response: {err}")))?;
    if response.len() > cap {
        return Err(CliError::new(format!(
            "git registry response from {url} exceeds {cap} bytes"
        )));
    }
    parse_http_response(url, &response, cap)
}

#[derive(Debug, PartialEq, Eq)]
struct HttpUrl {
    host: String,
    port: u16,
    path: String,
}

impl HttpUrl {
    fn parse(url: &str) -> Result<Self, CliError> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| CliError::new("git registry URL must use http://"))?;
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, "/".to_owned()),
        };
        if authority.is_empty() {
            return Err(CliError::new("git registry URL has no host"));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => {
                let port = port.parse::<u16>().map_err(|err| {
                    CliError::new(format!("git registry URL has invalid port: {err}"))
                })?;
                (host.to_owned(), port)
            }
            _ => (authority.to_owned(), 80),
        };
        Ok(Self { host, port, path })
    }

    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn parse_http_response(url: &str, response: &[u8], cap: usize) -> Result<Vec<u8>, CliError> {
    let Some(split) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(CliError::new(format!(
            "git registry response from {url} has no header terminator"
        )));
    };
    let headers = std::str::from_utf8(&response[..split]).map_err(|err| {
        CliError::new(format!(
            "git registry response headers are not UTF-8: {err}"
        ))
    })?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .ok_or_else(|| CliError::new("git registry response has no status line"))?;
    let code = status
        .split_ascii_whitespace()
        .nth(1)
        .ok_or_else(|| CliError::new("git registry response status has no code"))?
        .parse::<u16>()
        .map_err(|err| CliError::new(format!("git registry status code is invalid: {err}")))?;
    if code != 200 {
        return Err(CliError::new(format!(
            "git registry GET {url} returned HTTP {code}"
        )));
    }
    let header_map = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let body = &response[split + 4..];
    if header_map
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        return decode_chunked_body(body, cap);
    }
    if let Some(length) = header_map.get("content-length") {
        let length = length.parse::<usize>().map_err(|err| {
            CliError::new(format!("git registry content length is invalid: {err}"))
        })?;
        if length > cap {
            return Err(CliError::new(format!(
                "git registry response body length {length} exceeds {cap} bytes"
            )));
        }
        if body.len() < length {
            return Err(CliError::new("git registry response body is truncated"));
        }
        return Ok(body[..length].to_vec());
    }
    Ok(body.to_vec())
}

fn decode_chunked_body(mut body: &[u8], cap: usize) -> Result<Vec<u8>, CliError> {
    let mut decoded = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| CliError::new("chunked git registry body has no size line"))?;
        let size_text = std::str::from_utf8(&body[..line_end])
            .map_err(|err| CliError::new(format!("chunk size is not UTF-8: {err}")))?;
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|err| CliError::new(format!("chunk size is invalid: {err}")))?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        // F17: `size + 2` must not wrap. A header like `fffffffffffffffe`
        // parses to usize::MAX-1; the old `size + 2` wrapped to 0 in release,
        // bypassing the truncation guard so `&body[..size]` sliced out of
        // bounds and panicked. Reject the overflow instead.
        let need = size
            .checked_add(2)
            .ok_or_else(|| CliError::new("chunked git registry body chunk size overflow"))?;
        if body.len() < need {
            return Err(CliError::new("chunked git registry body is truncated"));
        }
        // F18: bound the total decoded length as well as the raw response.
        if decoded.len().saturating_add(size) > cap {
            return Err(CliError::new(format!(
                "chunked git registry body exceeds {cap} bytes"
            )));
        }
        decoded.extend_from_slice(&body[..size]);
        if &body[size..need] != b"\r\n" {
            return Err(CliError::new(
                "chunked git registry body has a bad delimiter",
            ));
        }
        body = &body[need..];
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use super::*;

    #[test]
    fn fetches_matching_version_and_caches_artifact() {
        let server = FixtureServer::start([
            (
                "/packages/sim-codec-lisp/index.txt",
                b"0.0.9\n0.1.0\n0.2.0\n".to_vec(),
            ),
            (
                "/packages/sim-codec-lisp/0.1.0/artifact.simlib",
                b"artifact".to_vec(),
            ),
        ]);
        let cache = temp_cache("git-registry-fetch");
        let resolver = GitRegistryResolver::new(server.endpoint(), cache.clone()).unwrap();

        let resolved = resolver
            .resolve(&"sim-codec-lisp@^0.1".parse().unwrap())
            .unwrap();

        assert_eq!(resolved.version, "0.1.0");
        assert_eq!(fs::read(&resolved.artifact).unwrap(), b"artifact");
        assert!(resolved.artifact.starts_with(&cache));
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
        let err = decode_chunked_body(b"fffffffffffffffe\r\n", MAX_ARTIFACT_BYTES)
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
        // One 8-byte chunk decoded under a 4-byte cap must be rejected.
        let err = decode_chunked_body(b"8\r\nAAAAAAAA\r\n0\r\n", 4)
            .expect_err("decoded length past the cap must error")
            .to_string();
        assert!(err.contains("exceeds 4 bytes"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_unsafe_artifact_file_names() {
        assert_eq!(
            index_artifact("0.1.0 ../artifact.simlib")
                .unwrap_err()
                .to_string(),
            "git registry artifact file name is not a safe path component: ../artifact.simlib"
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
}
