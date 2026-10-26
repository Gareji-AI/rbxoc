//! Asset ID registry.
//!
//! A manifest records which local files map to which Roblox asset IDs, and a
//! lockfile stores the IDs that were actually uploaded. `rbxoc assets codegen`
//! then emits Luau or TypeScript so game code writes `Assets.ui.button`
//! instead of a bare number.
//!
//! The split follows Tarmac and Asphalt:
//! - `rbxoc.assets.toml` (manifest, hand-written): globs, creator, codegen options
//! - `rbxoc.assets.lock.toml` (lockfile, generated): path -> assetId, commit it
//!
//! Uploading is out of scope for codegen. `assets register` records IDs that
//! came from `asset create`, so a sync loop can be built by an agent.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cmd::codegen::{self, Language, Options, Style};

/// Manifest file name.
pub const MANIFEST_NAME: &str = "rbxoc.assets.toml";
/// Lockfile name.
pub const LOCKFILE_NAME: &str = "rbxoc.assets.lock.toml";
/// Separate from `.env` so locking never changes credential discovery boundaries.
pub const MUTATION_LOCK_NAME: &str = ".rbxoc-assets-sync.lock";

/// Creator that owns uploaded assets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Creator {
    /// `user` or `group`.
    #[serde(rename = "type")]
    pub kind: String,
    pub id: u64,
}

/// Code generation settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodegenConfig {
    /// `luau` or `typescript`.
    #[serde(default = "default_language")]
    pub language: String,
    /// `flat` or `nested`.
    #[serde(default = "default_style")]
    pub style: String,
    /// Where to write the generated file.
    pub output_path: PathBuf,
    /// Variable name inside the generated file.
    #[serde(default = "default_variable")]
    pub variable: String,
    /// Output file base name. Defaults to the lowercased `variable` (e.g. `Assets.luau`).
    #[serde(default)]
    pub output_basename: Option<String>,
    /// Banner tool name. Defaults to `rbxoc`.
    #[serde(default = "default_generator")]
    pub generator: String,
    #[serde(default)]
    pub strip_extensions: bool,
    #[serde(default)]
    pub content: bool,
    /// Use explicitly resolved Image IDs for image references, never Decal IDs.
    #[serde(default)]
    pub image_ids: bool,
    /// Alpha-bleed transparent PNG edges before upload (Asphalt default: true).
    #[serde(default = "default_bleed")]
    pub bleed: bool,
}

fn default_bleed() -> bool {
    true
}

fn default_language() -> String {
    "luau".into()
}
fn default_style() -> String {
    "nested".into()
}
fn default_variable() -> String {
    "Assets".into()
}
fn default_generator() -> String {
    "rbxoc".into()
}

/// One input group.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Inputs {
    /// Glob for the source files, relative to the manifest.
    pub path: String,
    /// When set, lock/codegen keys drop everything through this segment (e.g. `assets/`).
    #[serde(default)]
    pub path_key_from: Option<String>,
    /// Override manifest `[codegen].bleed` for this input.
    #[serde(default)]
    pub bleed: Option<bool>,
}

/// The manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub creator: Creator,
    pub codegen: CodegenConfig,
    #[serde(default)]
    pub inputs: BTreeMap<String, Inputs>,
}

/// The lockfile.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Lockfile {
    pub version: u32,
    /// `relative path -> assetId`
    #[serde(default)]
    pub assets: BTreeMap<String, u64>,
    /// Only successful publication receipts establish content freshness.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub publications: BTreeMap<String, PublicationReceipt>,
    /// An entry without an operation ID is an uncertain publication, never retryable.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pending: BTreeMap<String, PendingUpload>,
    /// Explicit offline Decal -> Image associations; original IDs stay intact.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub image_ids: BTreeMap<String, ImageReference>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageReference {
    pub decal_id: u64,
    pub image_id: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublicationReceipt {
    pub asset_id: u64,
    pub fingerprint: String,
    pub creator: Creator,
    pub asset_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PendingUpload {
    pub fingerprint: String,
    pub creator: Creator,
    pub asset_type: String,
    /// Exact create display name; absent for content-only updates and legacy receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Local UTC intent time recorded before the request, not provider acceptance time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<String>,
    /// Some(ID) means an in-place update of this exact registered asset.
    /// Missing in v0.3 receipts, which always represent a create.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_asset_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub bleed: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Freshness {
    pub current: Vec<String>,
    pub changed: Vec<String>,
    pub unverified: Vec<String>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            version: 1,
            assets: BTreeMap::new(),
            publications: BTreeMap::new(),
            pending: BTreeMap::new(),
            image_ids: BTreeMap::new(),
        }
    }
}

/// Load the manifest from a directory. Errors explain what is missing.
pub fn load_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join(MANIFEST_NAME);
    if !path.exists() {
        bail!(
            "{} not found in {}. Create one to declare the creator, codegen, and inputs",
            MANIFEST_NAME,
            dir.display()
        );
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Reject values that would produce broken output later.
fn validate(manifest: &Manifest) -> Result<()> {
    if manifest.creator.id == 0 {
        bail!("creator.id must be a positive Roblox user or group ID");
    }
    if !matches!(manifest.creator.kind.as_str(), "user" | "group") {
        bail!(
            "creator.type must be \"user\" or \"group\", got \"{}\"",
            manifest.creator.kind
        );
    }
    if !matches!(manifest.codegen.language.as_str(), "luau" | "typescript") {
        bail!(
            "codegen.language must be \"luau\" or \"typescript\", got \"{}\"",
            manifest.codegen.language
        );
    }
    if !matches!(manifest.codegen.style.as_str(), "flat" | "nested") {
        bail!(
            "codegen.style must be \"flat\" or \"nested\", got \"{}\"",
            manifest.codegen.style
        );
    }
    if !codegen::is_valid_identifier(&manifest.codegen.variable) {
        bail!("codegen.variable must be a non-reserved identifier");
    }
    if let Some(base) = &manifest.codegen.output_basename
        && (base.is_empty() || base.contains(['/', '\\']) || matches!(base.as_str(), "." | ".."))
    {
        bail!("codegen.output_basename must be a nonempty file name, not a path");
    }
    Ok(())
}

/// Load the lockfile. A missing file is an empty registry, not an error.
pub fn load_lockfile(dir: &Path) -> Result<Lockfile> {
    let path = dir.join(LOCKFILE_NAME);
    if !path.exists() {
        return Ok(Lockfile::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

/// Write the lockfile. Deterministic key order via BTreeMap.
pub fn save_lockfile(dir: &Path, lock: &Lockfile) -> Result<PathBuf> {
    let path = dir.join(LOCKFILE_NAME);
    let body = toml::to_string_pretty(lock).context("cannot serialize the lockfile")?;
    let header =
        "# This file is automatically @generated by rbxoc.\n# Commit it so the IDs stay stable.\n";
    atomic_write(&path, format!("{header}{body}").as_bytes())?;
    Ok(path)
}

/// Replace only after the complete new state reaches disk. A failed write or
/// rename leaves the previous receipt intact; temporary files clean up on drop.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("output has no parent directory")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create temporary file for {}", path.display()))?;
    temp.write_all(bytes)
        .with_context(|| format!("cannot write {}", path.display()))?;
    temp.as_file()
        .sync_all()
        .context("cannot flush receipt to disk")?;
    temp.persist(path)
        .with_context(|| format!("cannot atomically replace {}", path.display()))?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Serialize publication and local registry mutations in the same directory.
pub fn registry_guard(dir: &Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(MUTATION_LOCK_NAME))?;
    file.try_lock()
        .context("another process is changing this asset registry; retry when it finishes")?;
    Ok(file)
}

/// Resolve the manifest globs into sorted relative paths.
pub fn collect_paths(dir: &Path, manifest: &Manifest) -> Result<Vec<String>> {
    Ok(collect_sources(dir, manifest)?.into_keys().collect())
}

/// Resolve keys once, retaining preprocessing settings and rejecting ambiguous
/// path_key_from mappings instead of silently choosing the first source.
pub fn collect_sources(dir: &Path, manifest: &Manifest) -> Result<BTreeMap<String, Source>> {
    let mut found = BTreeMap::new();
    for (name, input) in &manifest.inputs {
        let pattern = dir.join(&input.path);
        let pattern_str = pattern.to_string_lossy().to_string();
        // The glob crate needs forward slashes on Windows.
        let pattern_str = pattern_str.replace('\\', "/");
        let mut matched = 0usize;
        for entry in glob::glob(&pattern_str)
            .with_context(|| format!("invalid glob for input {name}: {}", input.path))?
        {
            let path = entry.with_context(|| format!("cannot read a match of input {name}"))?;
            if !path.is_file() {
                continue;
            }
            if path.file_name().is_some_and(|n| n == ".gitkeep") {
                continue;
            }
            if is_non_asset_sidecar(&path) {
                continue;
            }
            let rel = path.strip_prefix(dir).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            let key = normalize_path_key(&rel, input);
            let source = Source {
                path,
                bleed: input.bleed.unwrap_or(manifest.codegen.bleed),
            };
            if let Some(previous) = found.insert(key.clone(), source.clone())
                && previous != source
            {
                bail!(
                    "manifest key {key:?} is ambiguous: {} and {} (possibly different bleed settings)",
                    previous.path.display(),
                    source.path.display()
                );
            }
            matched += 1;
        }
        if matched == 0 {
            // Not fatal: the assets may simply not be uploaded yet.
            eprintln!("warning: input {name} matched no files ({})", input.path);
        }
    }
    Ok(found)
}

/// Exact manifest/lock keys, not globs. A bad selection never falls back to all.
pub fn select_paths(paths: &[String], selection: &[String]) -> Result<Vec<String>> {
    if selection.is_empty() {
        return Ok(paths.to_vec());
    }
    let mut selected = std::collections::BTreeSet::new();
    for path in selection {
        let key = path.replace('\\', "/");
        if !paths.contains(&key) {
            bail!(
                "selected path {key:?} is not a manifest key; use `assets status` keys, including path_key_from normalization"
            );
        }
        selected.insert(key);
    }
    Ok(selected.into_iter().collect())
}

/// Bounded-memory source hash plus the versioned preprocessing contract. A
/// change to SVG rasterization defaults must increment this pipeline version.
pub fn fingerprint(source: &Source) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"rbxoc-preprocess-v1\0");
    hash.update(&[u8::from(source.bleed)]);
    let extension = source
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    hash.update(extension.as_bytes());
    hash.update(b"\0");
    let mut file = std::fs::File::open(&source.path).with_context(|| {
        format!(
            "cannot read {} for publication fingerprint",
            source.path.display()
        )
    })?;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub fn freshness(
    sources: &BTreeMap<String, Source>,
    paths: &[String],
    manifest: &Manifest,
    lock: &Lockfile,
) -> Result<Freshness> {
    let mut state = Freshness::default();
    for path in paths {
        let Some(id) = lock.assets.get(path) else {
            continue;
        };
        match lock
            .publications
            .get(path)
            .filter(|receipt| receipt.asset_id == *id)
        {
            None => state.unverified.push(path.clone()),
            Some(receipt) => {
                let source = sources
                    .get(path)
                    .with_context(|| format!("no source for {path}"))?;
                if receipt.creator == manifest.creator
                    && receipt.fingerprint == fingerprint(source)?
                {
                    state.current.push(path.clone());
                } else {
                    state.changed.push(path.clone());
                }
            }
        }
    }
    Ok(state)
}

fn is_non_asset_sidecar(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "md" | "json" | "txt" | "toml")
    )
}

fn normalize_path_key(rel: &str, input: &Inputs) -> String {
    if let Some(anchor) = &input.path_key_from {
        let anchor = anchor.replace('\\', "/");
        if let Some(idx) = rel.find(&anchor) {
            return rel[idx + anchor.len()..].to_string();
        }
    }
    rel.to_string()
}

/// Convert the manifest's codegen settings into generator options.
pub fn codegen_options(config: &CodegenConfig) -> Options {
    Options {
        language: if config.language == "typescript" {
            Language::TypeScript
        } else {
            Language::Luau
        },
        style: if config.style == "flat" {
            Style::Flat
        } else {
            Style::Nested
        },
        strip_extensions: config.strip_extensions,
        content: config.content,
        generator: config.generator.clone(),
    }
}

/// File name for generated output (includes extension).
pub fn output_file_name(config: &CodegenConfig) -> String {
    let extension = if config.language == "typescript" {
        "d.ts"
    } else {
        "luau"
    };
    match &config.output_basename {
        Some(base) if base.contains('.') => base.clone(),
        Some(base) => format!("{base}.{extension}"),
        None => format!("{}.{}", config.variable.to_lowercase(), extension),
    }
}

/// Write generated source from the lockfile. Returns the output path.
pub fn write_generated(dir: &Path, manifest: &Manifest, lock: &Lockfile) -> Result<PathBuf> {
    let source = render(manifest, lock)?;
    let sources = collect_sources(dir, manifest)?;
    let out_file = prepare_output_path(dir, manifest, &sources)?;
    atomic_write(&out_file, source.as_bytes())?;
    Ok(out_file)
}

/// Validate the actual destination, including symlink aliases, before uploads
/// or generation can overwrite source files, the manifest, or receipts.
pub fn prepare_output_path(
    dir: &Path,
    manifest: &Manifest,
    sources: &BTreeMap<String, Source>,
) -> Result<PathBuf> {
    let out_dir = dir.join(&manifest.codegen.output_path);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let out_file = out_dir.join(output_file_name(&manifest.codegen));
    if out_file.is_dir() {
        bail!("generated output {} is a directory", out_file.display());
    }
    let destination = if out_file.exists() {
        out_file.canonicalize()?
    } else {
        out_dir
            .canonicalize()?
            .join(out_file.file_name().context("output has no file name")?)
    };
    let root = dir.canonicalize()?;
    let mut protected = vec![
        root.join(MANIFEST_NAME),
        root.join(LOCKFILE_NAME),
        root.join(MUTATION_LOCK_NAME),
    ];
    for path in [dir.join(MANIFEST_NAME), dir.join(LOCKFILE_NAME)] {
        if path.exists() {
            protected.push(path.canonicalize()?);
        }
    }
    for source in sources.values() {
        protected.push(source.path.canonicalize()?);
    }
    if destination.starts_with(root.join(".rbxoc")) || protected.contains(&destination) {
        bail!(
            "generated output {} would overwrite a source, manifest, receipt, or credential/state file; choose a different codegen destination",
            out_file.display()
        );
    }
    tempfile::NamedTempFile::new_in(&out_dir)
        .with_context(|| format!("cannot write output in {}", out_dir.display()))?;
    Ok(out_file)
}

/// Render the registry as source.
pub fn render(manifest: &Manifest, lock: &Lockfile) -> Result<String> {
    let options = codegen_options(&manifest.codegen);
    codegen::validate_keys(lock.assets.keys().map(String::as_str), &options)?;
    let pairs: Vec<(String, u64)> = lock.assets.iter().map(|(key, id)| {
        let resolved = if manifest.codegen.image_ids && is_image_entry(key, lock) {
            let image = lock.image_ids.get(key).filter(|image| image.decal_id == *id && image.image_id > 0)
                .with_context(|| format!("{key}: no resolved Image ID for Decal {id}; use `asset image-id` and `assets register --id {id} --image-id IMAGE_ID`; do not re-upload"))?;
            image.image_id
        } else { *id };
        Ok((key.clone(), resolved))
    }).collect::<Result<_>>()?;
    let tree = codegen::build_tree(&pairs, &options)?;
    Ok(codegen::generate(
        &tree,
        &manifest.codegen.variable,
        &options,
    ))
}

pub fn is_image_entry(key: &str, lock: &Lockfile) -> bool {
    lock.image_ids.contains_key(key)
        || lock
            .publications
            .get(key)
            .is_some_and(|receipt| receipt.asset_type == "Decal")
        || matches!(
            Path::new(key)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("svg" | "png" | "jpg" | "jpeg" | "bmp" | "tga")
        )
}

/// Paths that are in the manifest but have no ID yet.
pub fn missing_ids(paths: &[String], lock: &Lockfile) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !lock.assets.contains_key(*p))
        .cloned()
        .collect()
}

/// Import asset IDs from an Asphalt v2 lockfile (hash keys) into a path-keyed lock.
///
/// Asphalt hashes raw file bytes before SVG/bleed processing. Matching files get
/// their existing Roblox IDs without re-uploading.
pub fn import_asphalt_lock(
    dir: &Path,
    asphalt_lock: &Path,
    input_name: &str,
) -> Result<(usize, Lockfile)> {
    let manifest = load_manifest(dir)?;
    let by_hash = load_asphalt_hashes(asphalt_lock, input_name)?;
    let mut lock = load_lockfile(dir)?;
    let mut imported = 0usize;
    for (input_label, input) in &manifest.inputs {
        let pattern = dir.join(&input.path);
        let pattern_str = pattern.to_string_lossy().replace('\\', "/");
        for entry in glob::glob(&pattern_str)
            .with_context(|| format!("invalid glob for input {input_label}: {}", input.path))?
        {
            let path =
                entry.with_context(|| format!("cannot read a match of input {input_label}"))?;
            if !path.is_file()
                || path.file_name().is_some_and(|n| n == ".gitkeep")
                || is_non_asset_sidecar(&path)
            {
                continue;
            }
            let rel = path.strip_prefix(dir).unwrap_or(&path);
            let rel = rel.to_string_lossy().replace('\\', "/");
            let key = normalize_path_key(&rel, input);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("cannot read {} for hash import", path.display()))?;
            if let Some(id) = match_asphalt_hash(&bytes, &by_hash) {
                lock.publications.remove(&key);
                lock.pending.remove(&key);
                lock.image_ids.remove(&key);
                lock.assets.insert(key, id);
                imported += 1;
            }
        }
    }
    Ok((imported, lock))
}

fn match_asphalt_hash(bytes: &[u8], by_hash: &BTreeMap<String, u64>) -> Option<u64> {
    let blake = blake3::hash(bytes).to_hex().to_string();
    if let Some(id) = by_hash.get(&blake) {
        return Some(*id);
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let sha: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    by_hash.get(&sha).copied()
}

fn load_asphalt_hashes(path: &Path, input_name: &str) -> Result<BTreeMap<String, u64>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let doc: toml::Value =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    let mut out = BTreeMap::new();
    let Some(scope) = doc
        .get("inputs")
        .and_then(|v| v.get(input_name))
        .and_then(|v| v.as_table())
    else {
        bail!("no inputs.{input_name} entries found in {}", path.display());
    };
    for (hash, entry) in scope {
        let Some(id) = entry.get("asset_id").and_then(|v| v.as_integer()) else {
            continue;
        };
        if id >= 0 {
            out.insert(hash.clone(), id as u64);
        }
    }
    Ok(out)
}

/// Paths that have an ID but no longer match the manifest.
pub fn orphaned_ids(paths: &[String], lock: &Lockfile) -> Vec<String> {
    let known: std::collections::BTreeSet<&String> = paths.iter().collect();
    lock.assets
        .keys()
        .filter(|k| !known.contains(k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            creator: Creator {
                kind: "user".into(),
                id: 1,
            },
            codegen: CodegenConfig {
                language: "luau".into(),
                style: "nested".into(),
                output_path: PathBuf::from("src/shared"),
                variable: "Assets".into(),
                output_basename: None,
                generator: "rbxoc".into(),
                strip_extensions: false,
                content: false,
                image_ids: false,
                bleed: true,
            },
            inputs: BTreeMap::new(),
        }
    }

    #[test]
    fn validates_creator_kind() {
        let mut m = manifest();
        m.creator.kind = "team".into();
        assert!(validate(&m).is_err());
    }

    #[test]
    fn validates_language_and_style() {
        let mut m = manifest();
        m.codegen.language = "python".into();
        assert!(validate(&m).is_err());

        let mut m = manifest();
        m.codegen.style = "tree".into();
        assert!(validate(&m).is_err());
    }

    #[test]
    fn renders_from_lockfile() {
        let mut lock = Lockfile::new();
        lock.assets.insert("ui/button.png".into(), 123);
        let code = render(&manifest(), &lock).unwrap();
        assert!(code.contains("local Assets = {"));
        // Dotted names cannot be bare identifiers, so they are bracketed.
        assert!(code.contains("[\"button.png\"] = \"rbxassetid://123\""));
    }

    #[test]
    fn missing_and_orphaned_are_complementary() {
        let mut lock = Lockfile::new();
        lock.assets.insert("a.png".into(), 1);
        lock.assets.insert("gone.png".into(), 2);
        let paths = vec!["a.png".to_string(), "b.png".to_string()];
        assert_eq!(missing_ids(&paths, &lock), vec!["b.png"]);
        assert_eq!(orphaned_ids(&paths, &lock), vec!["gone.png"]);
    }

    #[test]
    fn lockfile_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::new();
        lock.assets.insert("x/y.png".into(), 42);
        let path = save_lockfile(dir.path(), &lock).unwrap();
        assert!(path.exists());
        let loaded = load_lockfile(dir.path()).unwrap();
        assert_eq!(loaded.assets.get("x/y.png"), Some(&42));
        assert_eq!(loaded.version, 1);
    }

    #[test]
    fn missing_lockfile_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let lock = load_lockfile(dir.path()).unwrap();
        assert!(lock.assets.is_empty());
    }

    #[test]
    fn missing_manifest_explains_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_manifest(dir.path()).unwrap_err().to_string();
        assert!(err.contains(MANIFEST_NAME));
    }

    #[test]
    fn collision_does_not_overwrite_generated_output() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = manifest();
        m.codegen.strip_extensions = true;
        let mut lock = Lockfile::new();
        lock.assets.insert("ui/icon.png".into(), 1);
        let output = write_generated(dir.path(), &m, &lock).unwrap();
        let previous = std::fs::read(&output).unwrap();
        lock.assets.insert("ui/icon.svg".into(), 2);
        assert!(write_generated(dir.path(), &m, &lock).is_err());
        assert_eq!(std::fs::read(output).unwrap(), previous);
    }

    #[test]
    fn selection_is_exact_normalized_and_never_falls_back() {
        let paths = vec!["ui/a.png".into(), "ui/b.png".into()];
        assert_eq!(select_paths(&paths, &[]).unwrap(), paths);
        assert_eq!(
            select_paths(&paths, &["ui\\a.png".into(), "ui/a.png".into()]).unwrap(),
            vec!["ui/a.png"]
        );
        assert!(select_paths(&paths, &["ui/*.png".into()]).is_err());
        assert!(select_paths(&paths, &["ui/a.png".into(), "unknown.png".into()]).is_err());
    }

    #[test]
    fn freshness_requires_receipts_and_tracks_bytes_and_preprocessing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"initial bytes").unwrap();
        let mut m = manifest();
        let source = Source {
            path: path.clone(),
            bleed: true,
        };
        let mut sources = BTreeMap::from([("a.png".into(), source.clone())]);
        let paths = vec!["a.png".into()];
        // Old version-1 lockfiles are still readable, but never called current.
        let mut lock: Lockfile =
            toml::from_str("version = 1\n[assets]\n\"a.png\" = 123\n").unwrap();
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().unverified,
            paths
        );
        lock.publications.insert(
            "a.png".into(),
            PublicationReceipt {
                asset_id: 123,
                fingerprint: fingerprint(&source).unwrap(),
                creator: m.creator.clone(),
                asset_type: "Decal".into(),
            },
        );
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().current,
            paths
        );
        std::fs::write(&path, b"changed bytes").unwrap();
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().changed,
            paths
        );
        std::fs::write(&path, b"initial bytes").unwrap();
        sources.get_mut("a.png").unwrap().bleed = false;
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().changed,
            paths
        );
        sources.get_mut("a.png").unwrap().bleed = true;
        m.creator.id = 99;
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().changed,
            paths
        );
        lock.assets.insert("a.png".into(), 456);
        assert_eq!(
            freshness(&sources, &paths, &m, &lock).unwrap().unverified,
            paths
        );
    }

    #[test]
    fn path_key_from_is_used_and_ambiguous_sources_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for prefix in ["one", "two"] {
            std::fs::create_dir_all(dir.path().join(prefix).join("assets")).unwrap();
            std::fs::write(dir.path().join(prefix).join("assets/icon.png"), b"bytes").unwrap();
        }
        let mut m = manifest();
        m.inputs.insert(
            "first".into(),
            Inputs {
                path: "one/assets/*.png".into(),
                path_key_from: Some("assets/".into()),
                bleed: None,
            },
        );
        assert_eq!(collect_paths(dir.path(), &m).unwrap(), vec!["icon.png"]);
        m.inputs.insert(
            "second".into(),
            Inputs {
                path: "two/assets/*.png".into(),
                path_key_from: Some("assets/".into()),
                bleed: None,
            },
        );
        assert!(
            collect_sources(dir.path(), &m)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }

    #[test]
    fn interrupted_temp_write_preserves_previous_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lockfile::new();
        lock.assets.insert("a.png".into(), 123);
        let path = save_lockfile(dir.path(), &lock).unwrap();
        let previous = std::fs::read(&path).unwrap();
        {
            let mut interrupted = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            interrupted.write_all(b"partial replacement").unwrap();
            // Dropped before persist, as on any write/flush error.
        }
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(load_lockfile(dir.path()).unwrap().assets["a.png"], 123);
        let blocked = dir.path().join("directory-not-a-file");
        std::fs::create_dir(&blocked).unwrap();
        assert!(atomic_write(&blocked, b"cannot replace directory").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
    }

    #[test]
    fn registry_guard_refuses_concurrent_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let guard = registry_guard(dir.path()).unwrap();
        assert!(
            !dir.path().join(".env").exists(),
            "locking must not create the project credential file"
        );
        assert!(registry_guard(dir.path()).is_err());
        drop(guard);
        assert!(registry_guard(dir.path()).is_ok());
    }

    #[test]
    fn image_reference_codegen_is_explicit_and_preserves_decal_ids() {
        let mut m = manifest();
        let mut lock = Lockfile::new();
        lock.assets.insert("ui/icon.png".into(), 123);
        lock.image_ids.insert(
            "ui/icon.png".into(),
            ImageReference {
                decal_id: 123,
                image_id: 456,
            },
        );
        assert!(render(&m, &lock).unwrap().contains("rbxassetid://123"));
        m.codegen.image_ids = true;
        assert!(render(&m, &lock).unwrap().contains("rbxassetid://456"));
        assert_eq!(lock.assets["ui/icon.png"], 123);
        lock.image_ids.clear();
        for language in ["luau", "typescript"] {
            m.codegen.language = language.into();
            let error = render(&m, &lock).unwrap_err().to_string();
            assert!(error.contains("ui/icon.png"));
            assert!(error.contains("Decal 123"));
            assert!(error.contains("do not re-upload"));
        }
        lock.image_ids.insert(
            "ui/icon.png".into(),
            ImageReference {
                decal_id: 999,
                image_id: 456,
            },
        );
        assert!(
            render(&m, &lock).is_err(),
            "stale association must not be used"
        );
    }
}
