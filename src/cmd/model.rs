//! Roblox model file inspection.
//!
//! `.rbxm` and `.rbxmx` serve two different Open Cloud asset types with the
//! same extension and the same `model/x-rbxm` content type:
//!
//! - `Animation` - a `KeyframeSequence` or `CurveAnimation` at the root
//! - `Model` - anything else
//!
//! The extension alone cannot tell them apart, so the file is parsed with the
//! same DOM readers Rojo uses. This mirrors how Asphalt and Tarmac classify
//! uploads.
//!
//! Inspection is best effort: a malformed file is reported rather than
//! guessed. The caller decides whether to fall back to `Model`.

use std::fmt;

use anyhow::{Context, Result};

/// Container format of a Roblox model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// `.rbxm` - the binary format.
    Binary,
    /// `.rbxmx` - the XML format.
    Xml,
}

impl fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelFormat::Binary => f.write_str("binary"),
            ModelFormat::Xml => f.write_str("xml"),
        }
    }
}

/// What kind of asset a Roblox model file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    /// An animation: `KeyframeSequence` or `CurveAnimation` at the root.
    Animation,
    /// A model: everything else that is valid.
    Model,
}

impl ModelKind {
    /// The Open Cloud `assetType` string.
    pub fn asset_type(self) -> &'static str {
        match self {
            ModelKind::Animation => "Animation",
            ModelKind::Model => "Model",
        }
    }
}

/// Pick the format from the file extension.
pub fn format_for(path: &std::path::Path) -> Option<ModelFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rbxm" => Some(ModelFormat::Binary),
        "rbxmx" => Some(ModelFormat::Xml),
        _ => None,
    }
}

/// Advisory chosen from one reported failure, not a documented provider limit.
const XML_ANIMATION_WARNING_BYTES: u64 = 2 * 1024 * 1024;

/// Call only after resolving the asset type from the original model body.
pub fn large_xml_animation_warning(
    path: &std::path::Path,
    asset_type: &str,
    bytes: u64,
) -> Option<String> {
    (format_for(path) == Some(ModelFormat::Xml)
        && asset_type == "Animation"
        && bytes >= XML_ANIMATION_WARNING_BYTES)
        .then(|| format!(
            "{}: large XML Animation ({bytes} bytes, 2 MiB advisory threshold, not a Roblox limit). One such upload reportedly failed with HTTP 500; the cause is unconfirmed. Consider exporting binary .rbxm in Studio and verifying poses before a new upload. No conversion is performed. Reconcile existing pending receipts with their original bytes; do not retry an uncertain publication with a converted file.",
            path.display()
        ))
}

/// Best-effort, read-only advisory for registry dry-run. This deliberately does
/// not turn that command into full upload preflight or guess malformed types.
pub fn inspect_large_xml_animation(path: &std::path::Path) -> Option<String> {
    if format_for(path) != Some(ModelFormat::Xml)
        || std::fs::metadata(path).ok()?.len() < XML_ANIMATION_WARNING_BYTES
    {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let root = classify(&bytes, ModelFormat::Xml).ok()?;
    large_xml_animation_warning(path, root.kind().asset_type(), bytes.len() as u64)
}

/// Parse the file and classify it.
///
/// Errors carry the reason so the caller can surface it instead of silently
/// uploading under the wrong asset type.
pub fn classify(data: &[u8], format: ModelFormat) -> Result<RoootClass> {
    let dom = match format {
        ModelFormat::Binary => rbx_binary::from_reader(data)
            .context("cannot parse the .rbxm file as a binary Roblox model")?,
        ModelFormat::Xml => rbx_xml::from_reader(data, rbx_xml::DecodeOptions::new())
            .context("cannot parse the .rbxmx file as an XML Roblox model")?,
    };

    let root = dom.root();
    let first_ref = root
        .children()
        .first()
        .copied()
        .context("the file has no instance at its root")?;
    let first = dom
        .get_by_ref(first_ref)
        .context("the root instance is missing from the file")?;

    Ok(RoootClass {
        class: first.class.to_string(),
    })
}

/// The class name of the first root instance.
#[derive(Debug, Clone)]
pub struct RoootClass {
    pub class: String,
}

impl RoootClass {
    /// Classify based on the root class name.
    pub fn kind(&self) -> ModelKind {
        match self.class.as_str() {
            "KeyframeSequence" | "CurveAnimation" => ModelKind::Animation,
            _ => ModelKind::Model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANIMATION_XML: &str = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Item class="KeyframeSequence" referent="RBX0">
		<Properties>
			<string name="Name">TestAnimation</string>
		</Properties>
	</Item>
</roblox>"#;

    const MODEL_XML: &str = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Item class="Model" referent="RBX0">
		<Properties>
			<string name="Name">TestModel</string>
		</Properties>
	</Item>
</roblox>"#;

    const CURVE_XML: &str = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4">
	<Item class="CurveAnimation" referent="RBX0">
		<Properties>
			<string name="Name">TestCurve</string>
		</Properties>
	</Item>
</roblox>"#;

    fn classify_xml(xml: &str) -> ModelKind {
        classify(xml.as_bytes(), ModelFormat::Xml).unwrap().kind()
    }

    #[test]
    fn large_xml_advisory_does_not_apply_to_binary_models_or_small_animations() {
        use std::path::Path;
        for (path, kind, bytes, expected) in [
            ("a.rbxm", "Animation", 3 * 1024 * 1024, false),
            ("a.rbxmx", "Model", 3 * 1024 * 1024, false),
            (
                "a.rbxmx",
                "Animation",
                XML_ANIMATION_WARNING_BYTES - 1,
                false,
            ),
            ("a.RBXMX", "Animation", XML_ANIMATION_WARNING_BYTES, true),
        ] {
            assert_eq!(
                large_xml_animation_warning(Path::new(path), kind, bytes).is_some(),
                expected
            );
        }
    }

    #[test]
    fn keyframe_sequence_is_an_animation() {
        assert_eq!(classify_xml(ANIMATION_XML), ModelKind::Animation);
    }

    #[test]
    fn curve_animation_is_an_animation() {
        assert_eq!(classify_xml(CURVE_XML), ModelKind::Animation);
    }

    #[test]
    fn model_stays_a_model() {
        assert_eq!(classify_xml(MODEL_XML), ModelKind::Model);
    }

    #[test]
    fn asset_type_strings_are_stable() {
        assert_eq!(ModelKind::Animation.asset_type(), "Animation");
        assert_eq!(ModelKind::Model.asset_type(), "Model");
    }

    #[test]
    fn format_comes_from_the_extension() {
        use std::path::Path;
        assert_eq!(format_for(Path::new("a.rbxm")), Some(ModelFormat::Binary));
        assert_eq!(format_for(Path::new("a.RBXMX")), Some(ModelFormat::Xml));
        assert_eq!(format_for(Path::new("a.fbx")), None);
        assert_eq!(format_for(Path::new("a")), None);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_guess() {
        let err = classify(b"not a model", ModelFormat::Xml).unwrap_err();
        assert!(
            err.to_string().contains("cannot parse"),
            "error should explain the parse failure: {err}"
        );
    }

    #[test]
    fn empty_document_is_an_error() {
        let empty = r#"<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd" version="4"></roblox>"#;
        assert!(classify(empty.as_bytes(), ModelFormat::Xml).is_err());
    }
}
