//! Local preprocessing before asset upload.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cmd::alpha_bleed;
use crate::output::ErrorBody;

/// File to send to the Assets API (original path or a rasterized temp PNG).
pub struct UploadFile {
    pub path: PathBuf,
    _temp: Option<tempfile::NamedTempFile>,
}

impl UploadFile {
    pub fn as_path(&self) -> &Path {
        &self.path
    }
}

fn file_error(code: &'static str, message: String, detail: Option<String>) -> ErrorBody {
    ErrorBody {
        code,
        message,
        status: None,
        detail,
        refs: None,
    }
}

fn write_temp_png(png: &[u8]) -> Result<UploadFile, ErrorBody> {
    let mut temp = tempfile::NamedTempFile::new().map_err(|e| ErrorBody {
        code: "temp_file_failed",
        message: format!("cannot create temp file: {e}"),
        status: None,
        detail: None,
        refs: None,
    })?;
    temp.write_all(png).map_err(|e| ErrorBody {
        code: "temp_write_failed",
        message: format!("cannot write temp PNG: {e}"),
        status: None,
        detail: None,
        refs: None,
    })?;
    let path = temp.path().to_path_buf();
    Ok(UploadFile {
        path,
        _temp: Some(temp),
    })
}

fn rasterize_svg(source: &Path) -> Result<Vec<u8>, ErrorBody> {
    let data = std::fs::read(source).map_err(|e| {
        file_error(
            "file_unreadable",
            format!("cannot read SVG: {}", source.display()),
            Some(e.to_string()),
        )
    })?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).map_err(
        |e| {
            file_error(
                "svg_parse_failed",
                format!("cannot parse SVG: {}", source.display()),
                Some(e.to_string()),
            )
        },
    )?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    if width == 0 || height == 0 {
        return Err(file_error(
            "svg_empty",
            format!("SVG has zero size: {}", source.display()),
            None,
        ));
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        file_error(
            "svg_raster_failed",
            format!("cannot allocate pixmap for {}", source.display()),
            None,
        )
    })?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| {
        file_error(
            "svg_encode_failed",
            format!("cannot encode PNG from SVG: {}", source.display()),
            Some(e.to_string()),
        )
    })
}

fn maybe_bleed_png_bytes(png: Vec<u8>, bleed: bool) -> Result<Vec<u8>, ErrorBody> {
    if !bleed {
        return Ok(png);
    }
    let mut img = image::load_from_memory(&png).map_err(|e| {
        file_error(
            "png_decode_failed",
            "cannot decode PNG for alpha bleed".into(),
            Some(e.to_string()),
        )
    })?;
    alpha_bleed::alpha_bleed(&mut img);
    let mut out = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Png,
    )
    .map_err(|e| {
        file_error(
            "png_encode_failed",
            "cannot encode PNG after alpha bleed".into(),
            Some(e.to_string()),
        )
    })?;
    Ok(out)
}

fn bleed_raster_file(source: &Path) -> Result<UploadFile, ErrorBody> {
    let mut img = image::open(source).map_err(|e| {
        file_error(
            "image_open_failed",
            format!("cannot open image: {}", source.display()),
            Some(e.to_string()),
        )
    })?;
    alpha_bleed::alpha_bleed(&mut img);
    let mut png = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png),
        image::ImageFormat::Png,
    )
    .map_err(|e| {
        file_error(
            "png_encode_failed",
            format!("cannot encode PNG: {}", source.display()),
            Some(e.to_string()),
        )
    })?;
    write_temp_png(&png)
}

/// Rasterize SVG, optionally alpha-bleed PNG/BMP/TGA, or pass through unchanged.
pub fn prepare_upload_file(source: &Path, bleed: bool) -> Result<UploadFile, ErrorBody> {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    if ext.as_deref() == Some("svg") {
        let png = rasterize_svg(source)?;
        let png = maybe_bleed_png_bytes(png, bleed)?;
        return write_temp_png(&png);
    }

    if bleed && matches!(ext.as_deref(), Some("png" | "bmp" | "tga")) {
        return bleed_raster_file(source);
    }

    Ok(UploadFile {
        path: source.to_path_buf(),
        _temp: None,
    })
}
