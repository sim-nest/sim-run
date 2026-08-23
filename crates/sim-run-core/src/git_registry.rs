use std::{
    fs,
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
        Self::with_policy(endpoint, cache_dir, false)
    }

    /// Builds a resolver with an explicit cleartext-remote policy supplied by
    /// the platform envelope.
    pub fn with_policy(
        endpoint: impl Into<String>,
        cache_dir: PathBuf,
        allow_insecure_remote: bool,
    ) -> Result<Self, CliError> {
        let endpoint = normalize_endpoint(endpoint.into(), allow_insecure_remote)?;
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

fn normalize_endpoint(endpoint: String, allow_insecure_remote: bool) -> Result<String, CliError> {
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
    let url = sim_lib_net_http::Url::parse(&endpoint)
        .map_err(|error| CliError::new(format!("git registry URL: {error}")))?;
    if !host_is_loopback(url.host()) && !allow_insecure_remote {
        return Err(CliError::new(format!(
            "git registry endpoint host {} is not loopback; refusing an unauthenticated http:// \
             fetch to a remote host (set {} to override)",
            url.host(),
            GIT_REGISTRY_ALLOW_INSECURE_ENV
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
    use sim_lib_net_http::{
        Cancellation, Client, Header, Method, Policy, Request, RequestBody, TcpConnector, Url,
    };
    let policy = Policy {
        connect_timeout: Duration::from_secs(10),
        read_timeout: Duration::from_secs(10),
        write_timeout: Duration::from_secs(10),
        total_timeout: Duration::from_secs(30),
        max_response_bytes: cap,
        ..Policy::default()
    };
    let response = Client::new(TcpConnector, policy)
        .execute(Request {
            method: Method::get(),
            url: Url::parse(url)
                .map_err(|error| CliError::new(format!("git registry URL: {error}")))?,
            headers: vec![
                Header::new(
                    "User-Agent",
                    format!("sim-run-core/{}", env!("CARGO_PKG_VERSION")),
                )
                .expect("static header name is valid"),
            ],
            body: RequestBody::Empty,
            deadline: None,
            cancellation: Cancellation::default(),
        })
        .map_err(|error| CliError::new(format!("git registry GET {url}: {error}")))?;
    if response.status != 200 {
        return Err(CliError::new(format!(
            "git registry GET {url} returned HTTP {}",
            response.status
        )));
    }
    Ok(response.into_body())
}

#[cfg(test)]
#[path = "git_registry_tests.rs"]
mod tests;
