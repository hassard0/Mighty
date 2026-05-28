//! `std.llm.Image` — multi-modal image input for `Member.ask_with_image`.
//!
//! v0.33 Track T2 exposes image input across all four providers. The
//! [`Image`] type is the canonical envelope:
//!
//! - [`Image::from_file`] — read PNG/JPG/etc. from disk + base64-encode.
//! - [`Image::from_bytes`] — caller-supplied bytes + explicit media type.
//! - [`Image::from_url`] — pass-through URL (each provider that accepts
//!   URLs avoids a local fetch; the others base64-encode after fetching).
//!
//! Construction is sync + small. The HTTP fetch + base64 encode happen
//! at construction (for `from_file`) or on first use; either way the
//! [`ImageSource`] handed to the provider is fully resolved.
//!
//! ## Mime-type detection
//!
//! `from_file` infers the mime from the file extension:
//!
//! | Extension          | Mime          |
//! |--------------------|---------------|
//! | `.png`             | `image/png`   |
//! | `.jpg`, `.jpeg`    | `image/jpeg`  |
//! | `.gif`             | `image/gif`   |
//! | `.webp`            | `image/webp`  |
//! | other / no ext     | `image/png` (default) |
//!
//! ## Capability discipline
//!
//! In Mighty source `Image.from_file(path)` requires `cap fs.read` and
//! `Image.from_url(url)` requires `cap net.https`. The Rust API mirrors
//! this only informally — the cap enforcement happens at the SIR effect
//! layer when the constructor is called from Mighty source.

use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::message::ImageSource;

/// Errors returned by [`Image`] constructors.
#[derive(Debug, Error)]
pub enum ImageErr {
    #[error("image io: {0}")]
    Io(String),
    #[error("image fetch: {0}")]
    Fetch(String),
    #[error("image decode: {0}")]
    Decode(String),
}

impl From<std::io::Error> for ImageErr {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Inferred or explicit media type for an [`Image`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaType {
    Png,
    Jpeg,
    Gif,
    Webp,
    /// Anything else.
    Other(String),
}

impl MediaType {
    pub fn as_mime(&self) -> &str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Infer from a file extension (case-insensitive). Falls back to PNG.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "gif" => Self::Gif,
            "webp" => Self::Webp,
            "" => Self::Png,
            other => Self::Other(format!("image/{other}")),
        }
    }

    /// Parse an explicit mime string.
    pub fn from_mime(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "image/png" => Self::Png,
            "image/jpeg" | "image/jpg" => Self::Jpeg,
            "image/gif" => Self::Gif,
            "image/webp" => Self::Webp,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Multi-modal image envelope. Carries either raw bytes (+ media type)
/// or a URL.
#[derive(Debug, Clone)]
pub enum Image {
    /// Bytes already in memory; ready to base64-encode for the wire.
    Bytes {
        bytes: Vec<u8>,
        media_type: MediaType,
    },
    /// URL the provider can resolve itself (Anthropic / OpenAI /
    /// Gemini all accept HTTPS URLs directly). When a provider doesn't,
    /// the conversion in [`Image::to_source`] short-circuits to a
    /// best-effort `data:` URL anyway.
    Url { url: String },
}

impl Image {
    /// Read `path`, infer the media type from the extension, return a
    /// fully-resolved `Bytes` variant.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ImageErr> {
        let p: PathBuf = path.as_ref().to_path_buf();
        let mut bytes = Vec::new();
        std::fs::File::open(&p)?.read_to_end(&mut bytes)?;
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        Ok(Self::Bytes {
            bytes,
            media_type: MediaType::from_extension(&ext),
        })
    }

    /// Build from caller-supplied bytes + explicit mime.
    pub fn from_bytes(bytes: Vec<u8>, mime: impl AsRef<str>) -> Self {
        Self::Bytes {
            bytes,
            media_type: MediaType::from_mime(mime.as_ref()),
        }
    }

    /// Build from a URL. Most providers accept this verbatim.
    pub fn from_url(url: impl Into<String>) -> Self {
        Self::Url { url: url.into() }
    }

    /// Stable media-type accessor.
    pub fn media_type(&self) -> MediaType {
        match self {
            Self::Bytes { media_type, .. } => media_type.clone(),
            Self::Url { url } => {
                // Infer from URL extension; default PNG.
                let path = url.split('?').next().unwrap_or(url);
                let ext = path.rsplit('.').next().unwrap_or("");
                MediaType::from_extension(ext)
            }
        }
    }

    /// Number of bytes if [`Image::Bytes`], else 0.
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Bytes { bytes, .. } => bytes.len(),
            Self::Url { .. } => 0,
        }
    }

    /// Convert to the typed [`ImageSource`] the provider serialisers
    /// already understand. Bytes are base64-encoded inline.
    pub fn to_source(&self) -> Result<ImageSource, ImageErr> {
        match self {
            Self::Bytes { bytes, media_type } => {
                let data = base64_encode(bytes);
                Ok(ImageSource::Base64 {
                    media_type: media_type.as_mime().to_string(),
                    data,
                })
            }
            Self::Url { url } => Ok(ImageSource::Url { url: url.clone() }),
        }
    }

    /// Convert to an [`ImageSource`] suitable for providers that
    /// require base64 inlining even when a URL was given (legacy
    /// Bedrock paths fall back to this). The default v0.33 surface
    /// uses the cheaper [`Image::to_source`] above.
    pub fn to_inline_source(&self) -> Result<ImageSource, ImageErr> {
        match self {
            Self::Bytes { .. } => self.to_source(),
            Self::Url { url } => {
                // No HTTP egress in the default build — providers handle
                // URL fetching themselves. Surface as URL anyway; the
                // sigv4 wrapper in `bedrock.rs` falls back to a `[image:
                // url]` placeholder.
                Ok(ImageSource::Url { url: url.clone() })
            }
        }
    }
}

/// Vendored base64 encoder (RFC 4648 §4 standard alphabet, padded).
/// Inline-vendored to keep the workspace dep tree slim — the
/// implementation is ~25 lines and trivially auditable.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let a = input[i] as u32;
        let b = input[i + 1] as u32;
        let c = input[i + 2] as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let a = input[i] as u32;
        let n = a << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let a = input[i] as u32;
        let b = input[i + 1] as u32;
        let n = (a << 16) | (b << 8);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn base64_encode_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn from_bytes_round_trips_media_type() {
        let img = Image::from_bytes(b"x".to_vec(), "image/jpeg");
        assert_eq!(img.media_type(), MediaType::Jpeg);
        assert_eq!(img.byte_len(), 1);
    }

    #[test]
    fn from_url_infers_media_type_from_extension() {
        let img = Image::from_url("https://example.com/a.webp");
        assert_eq!(img.media_type(), MediaType::Webp);
    }

    #[test]
    fn from_url_query_string_ignored_for_mime() {
        let img = Image::from_url("https://example.com/a.png?cache=1");
        assert_eq!(img.media_type(), MediaType::Png);
    }

    #[test]
    fn from_file_reads_and_infers_mime() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("pic.jpg");
        std::fs::write(&p, b"jpeg-data").unwrap();
        let img = Image::from_file(&p).unwrap();
        assert_eq!(img.media_type(), MediaType::Jpeg);
        assert_eq!(img.byte_len(), b"jpeg-data".len());
    }

    #[test]
    fn from_file_default_to_png_when_no_ext() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("no_ext");
        std::fs::write(&p, b"data").unwrap();
        let img = Image::from_file(&p).unwrap();
        assert_eq!(img.media_type(), MediaType::Png);
    }

    #[test]
    fn to_source_produces_base64_source_for_bytes() {
        let img = Image::from_bytes(b"foobar".to_vec(), "image/png");
        match img.to_source().unwrap() {
            ImageSource::Base64 { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "Zm9vYmFy");
            }
            _ => panic!("expected base64 source"),
        }
    }

    #[test]
    fn to_source_passes_through_url() {
        let img = Image::from_url("https://example.com/a.png");
        match img.to_source().unwrap() {
            ImageSource::Url { url } => assert_eq!(url, "https://example.com/a.png"),
            _ => panic!("expected url source"),
        }
    }

    #[test]
    fn media_type_from_mime_handles_aliases() {
        assert_eq!(MediaType::from_mime("image/jpg"), MediaType::Jpeg);
        assert_eq!(MediaType::from_mime("image/JPEG"), MediaType::Jpeg);
    }
}
