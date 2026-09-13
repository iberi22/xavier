//! MIME and format detection for generic document ingestion.
//!
//! Inspects initial magic bytes to determine document types reliably,
//! falling back to path extensions when necessary.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DetectedFormat {
    Pdf,
    Png,
    Jpeg,
    Webp,
    Gif,
    Markdown,
    Text,
    BinaryUnknown,
}

impl DetectedFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Markdown => "text/markdown",
            Self::Text => "text/plain",
            Self::BinaryUnknown => "application/octet-stream",
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }

    pub fn is_pdf(&self) -> bool {
        matches!(self, Self::Pdf)
    }

    pub fn is_textual(&self) -> bool {
        matches!(self, Self::Markdown | Self::Text)
    }
}

/// Detects the format of raw document bytes using magic bytes and optional file path.
pub fn detect_document_format(bytes: &[u8], path: Option<&Path>) -> DetectedFormat {
    // 1. Magic bytes detection (first 16-32 bytes)
    if bytes.len() >= 4 {
        // PDF magic bytes: %PDF- (0x25, 0x50, 0x44, 0x46, 0x2D)
        if bytes.starts_with(b"%PDF-") {
            return DetectedFormat::Pdf;
        }

        // PNG magic bytes: 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A
        if bytes.len() >= 8 && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        {
            return DetectedFormat::Png;
        }

        // JPEG magic bytes: 0xFF, 0xD8, 0xFF
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return DetectedFormat::Jpeg;
        }

        // WEBP: 'RIFF' .... 'WEBP'
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return DetectedFormat::Webp;
        }

        // GIF magic bytes: GIF87a or GIF89a
        if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
            return DetectedFormat::Gif;
        }
    }

    // 2. Fallback to path extension if available
    if let Some(p) = path {
        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            match ext_lower.as_str() {
                "pdf" => return DetectedFormat::Pdf,
                "png" => return DetectedFormat::Png,
                "jpg" | "jpeg" => return DetectedFormat::Jpeg,
                "webp" => return DetectedFormat::Webp,
                "gif" => return DetectedFormat::Gif,
                "md" | "markdown" => return DetectedFormat::Markdown,
                "txt" | "rs" | "py" | "ts" | "js" | "json" | "toml" | "yaml" | "yml" | "csv" => {
                    return DetectedFormat::Text;
                }
                _ => {}
            }
        }
    }

    // 3. Fallback to UTF-8 validation
    if std::str::from_utf8(bytes).is_ok() {
        if let Some(p) = path {
            if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case("md") {
                    return DetectedFormat::Markdown;
                }
            }
        }
        return DetectedFormat::Text;
    }

    DetectedFormat::BinaryUnknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_pdf() {
        let dummy_pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n";
        assert_eq!(detect_document_format(dummy_pdf, None), DetectedFormat::Pdf);
    }

    #[test]
    fn test_magic_png() {
        let dummy_png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        assert_eq!(
            detect_document_format(&dummy_png, None),
            DetectedFormat::Png
        );
    }

    #[test]
    fn test_magic_jpeg() {
        let dummy_jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(
            detect_document_format(&dummy_jpg, None),
            DetectedFormat::Jpeg
        );
    }

    #[test]
    fn test_magic_webp() {
        let dummy_webp = b"RIFF....WEBPVP8 ";
        assert_eq!(
            detect_document_format(dummy_webp, None),
            DetectedFormat::Webp
        );
    }

    #[test]
    fn test_text_fallback() {
        let text = b"# Title\n\nThis is markdown.";
        assert_eq!(
            detect_document_format(text, Some(Path::new("note.md"))),
            DetectedFormat::Markdown
        );
    }
}
