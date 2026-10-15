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
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cmd::codegen::{self, Language, Options, Style};

/// Manifest file name.
pub const MANIFEST_NAME: &str = "rbxoc.assets.toml";
/// Lockfile name.
pub const LOCKFILE_NAME: &str = "rbxoc.assets.lock.toml";

/// Creator that owns uploaded assets.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            version: 1,
            assets: BTreeMap::new(),
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
    std::fs::write(&path, format!("{header}{body}"))
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

/// Resolve the manifest globs into sorted relative paths.
pub fn collect_paths(dir: &Path, manifest: &Manifest) -> Result<Vec<String>> {
    let mut found = Vec::new();
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
            found.push(normalize_path_key(&rel, input));
            matched += 1;
        }
        if matched == 0 {
            // Not fatal: the assets may simply not be uploaded yet.
            eprintln!("warning: input {name} matched no files ({})", input.path);
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

pub fn normalize_path_key_for_input(rel: &str, input: &Inputs) -> String {
    normalize_path_key(rel, input)
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
    let out_dir = dir.join(&manifest.codegen.output_path);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let out_file = out_dir.join(output_file_name(&manifest.codegen));
    std::fs::write(&out_file, &source)
        .with_context(|| format!("cannot write {}", out_file.display()))?;
    Ok(out_file)
}

/// Render the registry as source.
pub fn render(manifest: &Manifest, lock: &Lockfile) -> Result<String> {
    let options = codegen_options(&manifest.codegen);
    let pairs: Vec<(String, u64)> = lock.assets.iter().map(|(k, v)| (k.clone(), *v)).collect();
    let tree = codegen::build_tree(&pairs, &options)?;
    Ok(codegen::generate(
        &tree,
        &manifest.codegen.variable,
        &options,
    ))
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
}
