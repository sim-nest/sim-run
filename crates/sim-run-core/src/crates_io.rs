use std::{
    cmp::Ordering,
    env, fmt, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::CliError;
#[cfg(feature = "registry")]
use crate::GitRegistryResolver;

const DEFAULT_FALLBACK_REQ: &str = "^0.1";
pub(crate) const ARTIFACT_FILE: &str = "artifact.simlib";

/// A parsed `crates.io:NAME@REQ` source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CratesIoSpec {
    /// crates.io package name.
    pub package: String,
    /// Version requirement applied to that package.
    pub requirement: VersionReq,
}

impl CratesIoSpec {
    /// Builds a spec from a package name and version requirement.
    ///
    /// Returns an error when the package name is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_run_core::CratesIoSpec;
    ///
    /// let spec = CratesIoSpec::new("sim-codec-lisp", "^0.1".parse().unwrap()).unwrap();
    /// assert_eq!(spec.package, "sim-codec-lisp");
    /// assert!(CratesIoSpec::new("", "*".parse().unwrap()).is_err());
    /// ```
    pub fn new(package: impl Into<String>, requirement: VersionReq) -> Result<Self, CliError> {
        let package = package.into();
        if package.is_empty() {
            return Err(CliError::new("crates.io package name is empty"));
        }
        Ok(Self {
            package,
            requirement,
        })
    }
}

impl FromStr for CratesIoSpec {
    type Err = CliError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let Some((package, requirement)) = source.split_once('@') else {
            return Err(CliError::new("crates.io source must use NAME@REQ syntax"));
        };
        Self::new(package.to_owned(), requirement.parse::<VersionReq>()?)
    }
}

impl fmt::Display for CratesIoSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.package, self.requirement)
    }
}

/// Version requirement syntax accepted by the CLI crates.io resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionReq {
    raw: String,
    kind: VersionReqKind,
}

impl VersionReq {
    /// Reports whether a concrete version satisfies this requirement.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_run_core::VersionReq;
    ///
    /// let caret: VersionReq = "^0.1".parse().unwrap();
    /// assert!(caret.matches("0.1.9"));
    /// assert!(!caret.matches("0.2.0"));
    /// ```
    pub fn matches(&self, version: &str) -> bool {
        match &self.kind {
            VersionReqKind::Any => true,
            VersionReqKind::Exact(exact) => version == exact,
            VersionReqKind::Caret(minimum) => {
                compare_versions(version, minimum) != Ordering::Less
                    && same_caret_range(version, minimum)
            }
        }
    }

    /// Returns the original requirement text.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl FromStr for VersionReq {
    type Err = CliError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        if source.is_empty() {
            return Err(CliError::new("crates.io version requirement is empty"));
        }
        let kind = if source == "*" {
            VersionReqKind::Any
        } else if let Some(minimum) = source.strip_prefix('^') {
            if minimum.is_empty() {
                return Err(CliError::new("crates.io caret requirement is empty"));
            }
            VersionReqKind::Caret(minimum.to_owned())
        } else if let Some(exact) = source.strip_prefix('=') {
            if exact.is_empty() {
                return Err(CliError::new("crates.io exact requirement is empty"));
            }
            VersionReqKind::Exact(exact.to_owned())
        } else {
            VersionReqKind::Exact(source.to_owned())
        };
        Ok(Self {
            raw: source.to_owned(),
            kind,
        })
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VersionReqKind {
    Any,
    Exact(String),
    Caret(String),
}

/// A crates.io source resolved to a local artifact path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCratesIoSource {
    /// The spec that was resolved.
    pub requested: CratesIoSpec,
    /// Resolved package name.
    pub package: String,
    /// Concrete version selected for the package.
    pub version: String,
    /// Local path to the resolved artifact.
    pub artifact: PathBuf,
}

/// A crates.io artifact available without network access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CratesIoListing {
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) artifact: PathBuf,
    pub(crate) source: CratesIoListingSource,
}

/// Where an offline crates.io artifact listing came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CratesIoListingSource {
    Cache,
    Registry,
}

/// crates.io-style artifact resolver owned by the CLI layer.
///
/// The default resolver performs **no network access**. It resolves a spec only
/// from an already-seeded local cache or an explicitly registered registry
/// artifact. With the `registry` feature, callers can install `GitRegistryResolver`
/// so unresolved specs fetch from an explicit git registry artifact endpoint and
/// then enter the same cache layout.
#[derive(Clone, Debug)]
pub struct CratesIoResolver {
    cache_dir: PathBuf,
    registry: Vec<RegistryArtifact>,
    #[cfg(feature = "registry")]
    git_registry: Option<GitRegistryResolver>,
}

impl CratesIoResolver {
    /// Builds a cache-only resolver rooted at a cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            registry: Vec::new(),
            #[cfg(feature = "registry")]
            git_registry: None,
        }
    }

    /// Returns the default cache directory.
    ///
    /// Honors `SIM_CLI_CACHE_DIR`, then `XDG_CACHE_HOME`, then `HOME`, and
    /// falls back to the system temporary directory.
    pub fn default_cache_dir() -> PathBuf {
        if let Ok(path) = env::var("SIM_CLI_CACHE_DIR") {
            return PathBuf::from(path);
        }
        if let Ok(path) = env::var("XDG_CACHE_HOME") {
            return PathBuf::from(path).join("sim").join("libs");
        }
        if let Ok(path) = env::var("HOME") {
            return PathBuf::from(path).join(".cache").join("sim").join("libs");
        }
        env::temp_dir().join("sim").join("libs")
    }

    /// Returns the cache directory this resolver reads from.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Registers an in-memory registry artifact available for resolution.
    pub fn add_registry_artifact(
        &mut self,
        package: impl Into<String>,
        version: impl Into<String>,
        artifact: impl Into<PathBuf>,
    ) {
        self.registry.push(RegistryArtifact {
            package: package.into(),
            version: version.into(),
            artifact: artifact.into(),
        });
    }

    /// Registers an in-memory registry artifact, returning `self`.
    pub fn with_registry_artifact(
        mut self,
        package: impl Into<String>,
        version: impl Into<String>,
        artifact: impl Into<PathBuf>,
    ) -> Self {
        self.add_registry_artifact(package, version, artifact);
        self
    }

    /// Sets the networked git registry artifact resolver used after cache and
    /// explicit in-memory registry misses.
    #[cfg(feature = "registry")]
    pub fn add_git_registry_resolver(&mut self, resolver: GitRegistryResolver) {
        self.git_registry = Some(resolver);
    }

    /// Sets the networked git registry artifact resolver, returning `self`.
    #[cfg(feature = "registry")]
    pub fn with_git_registry_resolver(mut self, resolver: GitRegistryResolver) -> Self {
        self.add_git_registry_resolver(resolver);
        self
    }

    /// Builds a git registry resolver from an endpoint and this resolver's cache.
    #[cfg(feature = "registry")]
    pub fn with_git_registry_endpoint(
        mut self,
        endpoint: impl Into<String>,
    ) -> Result<Self, CliError> {
        let resolver = GitRegistryResolver::new(endpoint, self.cache_dir.clone())?;
        self.add_git_registry_resolver(resolver);
        Ok(self)
    }

    /// Resolves a spec to a cached artifact, seeding the cache when needed.
    ///
    /// Prefers an already-cached artifact, then a registered registry artifact.
    /// With the `registry` feature and an installed git registry resolver, misses
    /// can fetch through that resolver. Otherwise misses fail closed with a
    /// clear cache-only error rather than pretending to fetch one.
    pub fn resolve(&self, spec: &CratesIoSpec) -> Result<ResolvedCratesIoSource, CliError> {
        if let Some((version, artifact)) = self.cached_artifact(spec)? {
            return Ok(ResolvedCratesIoSource {
                requested: spec.clone(),
                package: spec.package.clone(),
                version,
                artifact,
            });
        }
        if let Some(entry) = self.registry_artifact(spec) {
            let artifact = self.cache_artifact(&entry)?;
            return Ok(ResolvedCratesIoSource {
                requested: spec.clone(),
                package: entry.package,
                version: entry.version,
                artifact,
            });
        }
        #[cfg(feature = "registry")]
        if let Some(git_registry) = &self.git_registry {
            return git_registry.resolve(spec);
        }
        Err(CliError::new(format!(
            "crates.io network fetch is not implemented (cache-only resolver); seed the cache for {spec}"
        )))
    }

    pub(crate) fn available_artifacts(&self) -> Result<Vec<CratesIoListing>, CliError> {
        let mut listings = self.registry_artifact_listings();
        listings.extend(self.cached_artifact_listings()?);
        listings.sort_by(|left, right| {
            left.package
                .cmp(&right.package)
                .then_with(|| compare_versions(&right.version, &left.version))
                .then_with(|| left.artifact.cmp(&right.artifact))
                .then_with(|| left.source.cmp(&right.source))
        });
        Ok(listings)
    }

    fn registry_artifact_listings(&self) -> Vec<CratesIoListing> {
        self.registry
            .iter()
            .map(|entry| CratesIoListing {
                package: entry.package.clone(),
                version: entry.version.clone(),
                artifact: entry.artifact.clone(),
                source: CratesIoListingSource::Registry,
            })
            .collect()
    }

    fn cached_artifact_listings(&self) -> Result<Vec<CratesIoListing>, CliError> {
        let root = self.cache_dir.join("crates.io");
        let Ok(package_entries) = fs::read_dir(&root) else {
            return Ok(Vec::new());
        };
        let mut listings = Vec::new();
        for package_entry in package_entries {
            let package_entry = package_entry.map_err(|err| {
                CliError::new(format!("read crates.io cache {}: {err}", root.display()))
            })?;
            if !package_entry
                .file_type()
                .map(|ty| ty.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let package = package_entry.file_name().to_string_lossy().into_owned();
            let version_root = package_entry.path();
            for version_entry in fs::read_dir(&version_root).map_err(|err| {
                CliError::new(format!(
                    "read crates.io cache {}: {err}",
                    version_root.display()
                ))
            })? {
                let version_entry = version_entry.map_err(|err| {
                    CliError::new(format!(
                        "read crates.io cache {}: {err}",
                        version_root.display()
                    ))
                })?;
                if !version_entry
                    .file_type()
                    .map(|ty| ty.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Some(artifact) = cached_artifact_file(&version_entry.path())? {
                    listings.push(CratesIoListing {
                        package: package.clone(),
                        version: version_entry.file_name().to_string_lossy().into_owned(),
                        artifact,
                        source: CratesIoListingSource::Cache,
                    });
                }
            }
        }
        Ok(listings)
    }

    fn cached_artifact(&self, spec: &CratesIoSpec) -> Result<Option<(String, PathBuf)>, CliError> {
        let package_dir = self.cache_dir.join("crates.io").join(&spec.package);
        let Ok(entries) = fs::read_dir(&package_dir) else {
            return Ok(None);
        };
        let mut matches = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                CliError::new(format!(
                    "read crates.io cache {}: {err}",
                    package_dir.display()
                ))
            })?;
            if !entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false) {
                continue;
            }
            let version = entry.file_name().to_string_lossy().into_owned();
            if let Some(artifact) = cached_artifact_file(&entry.path())?
                && spec.requirement.matches(&version)
            {
                matches.push((version, artifact));
            }
        }
        matches.sort_by(|left, right| compare_versions(&right.0, &left.0));
        Ok(matches.into_iter().next())
    }

    fn registry_artifact(&self, spec: &CratesIoSpec) -> Option<RegistryArtifact> {
        let mut matches = self
            .registry
            .iter()
            .filter(|entry| {
                entry.package == spec.package && spec.requirement.matches(&entry.version)
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| compare_versions(&right.version, &left.version));
        matches.into_iter().next()
    }

    fn cache_artifact(&self, entry: &RegistryArtifact) -> Result<PathBuf, CliError> {
        let artifact = cache_artifact_path(&self.cache_dir, &entry.package, &entry.version);
        let parent = artifact
            .parent()
            .ok_or_else(|| CliError::new("crates.io cache artifact has no parent"))?;
        fs::create_dir_all(parent)
            .map_err(|err| CliError::new(format!("create crates.io cache: {err}")))?;
        fs::copy(&entry.artifact, &artifact).map_err(|err| {
            CliError::new(format!(
                "cache crates.io artifact {}: {err}",
                entry.artifact.display()
            ))
        })?;
        Ok(artifact)
    }
}

impl Default for CratesIoResolver {
    fn default() -> Self {
        Self::new(Self::default_cache_dir())
    }
}

#[derive(Clone, Debug)]
struct RegistryArtifact {
    package: String,
    version: String,
    artifact: PathBuf,
}

pub(crate) fn fallback_spec_for_symbol(symbol: &str) -> Option<CratesIoSpec> {
    if let Some(codec) = symbol.strip_prefix("codec/") {
        return fallback_spec(format!("sim-codec-{codec}"));
    }
    if !symbol.is_empty() && !symbol.contains('/') {
        return fallback_spec(format!("sim-lib-{symbol}"));
    }
    None
}

fn fallback_spec(package: String) -> Option<CratesIoSpec> {
    CratesIoSpec::new(package, DEFAULT_FALLBACK_REQ.parse().ok()?).ok()
}

pub(crate) fn cache_artifact_path(cache_dir: &Path, package: &str, version: &str) -> PathBuf {
    cache_artifact_path_with_file(cache_dir, package, version, ARTIFACT_FILE)
}

pub(crate) fn cache_artifact_path_with_file(
    cache_dir: &Path,
    package: &str,
    version: &str,
    file_name: &str,
) -> PathBuf {
    cache_dir
        .join("crates.io")
        .join(package)
        .join(version)
        .join(file_name)
}

fn cached_artifact_file(version_dir: &Path) -> Result<Option<PathBuf>, CliError> {
    let preferred = version_dir.join(ARTIFACT_FILE);
    if preferred.is_file() {
        return Ok(Some(preferred));
    }
    let Ok(entries) = fs::read_dir(version_dir) else {
        return Ok(None);
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            CliError::new(format!(
                "read crates.io cache {}: {err}",
                version_dir.display()
            ))
        })?;
        if entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files.into_iter().next())
}

pub(crate) fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = version_components(left);
    let right = version_components(right);
    let len = left.len().max(right.len());
    for index in 0..len {
        let ordering = left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn same_caret_range(version: &str, minimum: &str) -> bool {
    let version = version_components(version);
    let minimum = version_components(minimum);
    let major = *minimum.first().unwrap_or(&0);
    let minor = *minimum.get(1).unwrap_or(&0);
    let patch = *minimum.get(2).unwrap_or(&0);
    if major > 0 {
        version.first() == Some(&major)
    } else if minor > 0 {
        version.first() == Some(&0) && version.get(1) == Some(&minor)
    } else {
        version.first() == Some(&0) && version.get(1) == Some(&0) && version.get(2) == Some(&patch)
    }
}

fn version_components(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crates_io_specs_and_requirements() {
        let exact = "sim-codec-lisp@0.1.0".parse::<CratesIoSpec>().unwrap();
        assert_eq!(exact.package, "sim-codec-lisp");
        assert_eq!(exact.requirement.as_str(), "0.1.0");
        assert!(exact.requirement.matches("0.1.0"));
        assert!(!exact.requirement.matches("0.1.1"));

        let caret = "sim-lib-demo@^0.1".parse::<CratesIoSpec>().unwrap();
        assert!(caret.requirement.matches("0.1.0"));
        assert!(caret.requirement.matches("0.1.9"));
        assert!(!caret.requirement.matches("0.2.0"));
    }

    #[test]
    fn cache_hit_resolves_without_registry() {
        let cache = temp_dir("cache-hit");
        let artifact = cache_artifact_path(&cache, "sim-lib-demo", "0.1.0");
        fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        fs::write(&artifact, b"cached").unwrap();
        let resolver = CratesIoResolver::new(cache.clone());

        let resolved = resolver
            .resolve(&"sim-lib-demo@0.1.0".parse().unwrap())
            .unwrap();

        assert_eq!(resolved.version, "0.1.0");
        assert_eq!(resolved.artifact, artifact);
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn cache_miss_is_a_clear_cache_only_failure() {
        let cache = temp_dir("cache-miss");
        let resolver = CratesIoResolver::new(cache.clone());

        let err = resolver
            .resolve(&"sim-lib-demo@0.1.0".parse().unwrap())
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "crates.io network fetch is not implemented (cache-only resolver); \
             seed the cache for sim-lib-demo@0.1.0"
        );
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn fake_registry_resolve_caches_artifact() {
        let cache = temp_dir("fake-registry-cache");
        let source_dir = temp_dir("fake-registry-source");
        fs::create_dir_all(&source_dir).unwrap();
        let source_artifact = source_dir.join("artifact.simlib");
        fs::write(&source_artifact, b"registry").unwrap();
        let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
            "sim-lib-demo",
            "0.1.2",
            source_artifact,
        );

        let resolved = resolver
            .resolve(&"sim-lib-demo@^0.1".parse().unwrap())
            .unwrap();

        assert_eq!(resolved.version, "0.1.2");
        assert_eq!(fs::read(resolved.artifact).unwrap(), b"registry");
        let _ = fs::remove_dir_all(cache);
        let _ = fs::remove_dir_all(source_dir);
    }

    #[test]
    fn fallback_mapping_names_codec_and_lib_packages() {
        assert_eq!(
            fallback_spec_for_symbol("codec/lisp").unwrap().to_string(),
            "sim-codec-lisp@^0.1"
        );
        assert_eq!(
            fallback_spec_for_symbol("demo").unwrap().to_string(),
            "sim-lib-demo@^0.1"
        );
        assert!(fallback_spec_for_symbol("domain/demo").is_none());
    }

    fn temp_dir(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "sim-run-core-crates-{}-{label}",
            std::process::id()
        ))
    }
}
