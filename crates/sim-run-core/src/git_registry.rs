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
    crates_io::{cache_artifact_path_with_file, compare_versions},
};
use sha2::{Digest, Sha256};

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
/// explicit and must use `http://`. The resolver reads a text version index at
/// `packages/<package>/index.txt`, selects the newest version matching the
/// requested requirement, fetches the named artifact file, verifies it against
/// the row's SHA-256 digest, and caches the verified bytes under a hash-prefixed
/// artifact file.
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
        let artifact = verified_cache_artifact_path(&self.cache_dir, &spec.package, &selected);
        if artifact.is_file() {
            verify_cached_artifact(&artifact, spec, &selected)?;
        } else {
            let bytes = http_get(
                &self.artifact_url(&spec.package, &selected.version, &selected.file_name)?,
                MAX_ARTIFACT_BYTES,
            )?;
            verify_artifact_bytes(&bytes, spec, &selected)?;
            let parent = artifact
                .parent()
                .ok_or_else(|| CliError::new("git registry cache artifact has no parent"))?;
            fs::create_dir_all(parent)
                .map_err(|err| CliError::new(format!("create git registry cache: {err}")))?;
            fs::write(&artifact, &bytes)
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
    sha256: [u8; 32],
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
    let file_name = parts.next().ok_or_else(|| {
        CliError::new("git registry index row must use: <version> <file-name> <sha256-hex>")
    })?;
    let sha256 = parts.next().ok_or_else(|| {
        CliError::new("git registry index row must use: <version> <file-name> <sha256-hex>")
    })?;
    if parts.next().is_some() {
        return Err(CliError::new(
            "git registry index row has too many fields; expected: <version> <file-name> <sha256-hex>",
        ));
    }
    let file_name = safe_artifact_file_name(file_name)?;
    let sha256 = parse_sha256_hex(sha256)?;
    Ok(Some(IndexedArtifact {
        version: version.to_owned(),
        file_name,
        sha256,
    }))
}

fn verified_cache_artifact_path(
    cache_dir: &Path,
    package: &str,
    selected: &IndexedArtifact,
) -> PathBuf {
    cache_artifact_path_with_file(
        cache_dir,
        package,
        &selected.version,
        &content_addressed_file_name(&selected.file_name, &selected.sha256),
    )
}

fn content_addressed_file_name(file_name: &str, sha256: &[u8; 32]) -> String {
    format!("sha256-{}-{file_name}", sha256_hex(sha256))
}

fn verify_cached_artifact(
    artifact: &Path,
    spec: &CratesIoSpec,
    selected: &IndexedArtifact,
) -> Result<(), CliError> {
    let metadata = fs::metadata(artifact).map_err(|err| {
        CliError::new(format!(
            "read git registry cache metadata {}: {err}",
            artifact.display()
        ))
    })?;
    if metadata.len() > MAX_ARTIFACT_BYTES as u64 {
        return Err(CliError::new(format!(
            "cached git registry artifact {} exceeds {} bytes",
            artifact.display(),
            MAX_ARTIFACT_BYTES
        )));
    }
    let bytes = fs::read(artifact).map_err(|err| {
        CliError::new(format!(
            "read git registry cache artifact {}: {err}",
            artifact.display()
        ))
    })?;
    verify_artifact_bytes(&bytes, spec, selected)
}

fn verify_artifact_bytes(
    bytes: &[u8],
    spec: &CratesIoSpec,
    selected: &IndexedArtifact,
) -> Result<(), CliError> {
    let got = sha256(bytes);
    if got != selected.sha256 {
        return Err(CliError::new(format!(
            "git registry artifact {}@{} hash mismatch (expected {}, got {})",
            spec.package,
            selected.version,
            sha256_hex(&selected.sha256),
            sha256_hex(&got),
        )));
    }
    Ok(())
}

fn sha256(input: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(input);
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    bytes
}

fn parse_sha256_hex(hex: &str) -> Result<[u8; 32], CliError> {
    if hex.len() != 64 {
        return Err(CliError::new(format!(
            "git registry artifact sha256 must be 64 hex characters, got {}",
            hex.len()
        )));
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Result<u8, CliError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(CliError::new(format!(
            "git registry artifact sha256 contains non-hex byte 0x{byte:02x}"
        ))),
    }
}

fn sha256_hex(bytes: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
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
        return decode_git_registry_chunked(body, cap);
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

fn decode_git_registry_chunked(body: &[u8], cap: usize) -> Result<Vec<u8>, CliError> {
    sim_lib_net_core::decode_chunked(body, cap).map_err(map_git_registry_chunked_error)
}

fn map_git_registry_chunked_error(error: sim_lib_net_core::NetError) -> CliError {
    use sim_lib_net_core::NetError;
    match error {
        NetError::InvalidChunkSize(detail) => CliError::new(format!(
            "chunked git registry body has invalid chunk size: {detail}"
        )),
        NetError::TruncatedChunk => CliError::new("chunked git registry body is truncated"),
        NetError::InvalidChunkDelimiter => {
            CliError::new("chunked git registry body has a bad delimiter")
        }
        NetError::OversizeBody(cap) => {
            CliError::new(format!("chunked git registry body exceeds {cap} bytes"))
        }
        other => CliError::new(format!("chunked git registry body is invalid: {other}")),
    }
}

#[cfg(test)]
#[path = "git_registry_tests.rs"]
mod tests;
