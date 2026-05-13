use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::errors::ExportError;
use crate::models::{TranscriptionMetadata, TranscriptionResult};

/// Supported export formats.
pub enum ExportFormat {
    Txt,
    Markdown,
}

/// Format a TranscriptionResult as a Markdown string.
///
/// The output format is:
/// ```markdown
/// # Transcription
///
/// - **Source:** <filename>
/// - **Date:** <ISO 8601 timestamp>
///
/// ---
///
/// <transcription text>
/// ```
pub fn format_markdown(result: &TranscriptionResult) -> String {
    let filename = result
        .source_file
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let timestamp = result.timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    format!(
        "# Transcription\n\n- **Source:** {}\n- **Date:** {}\n\n---\n\n{}",
        filename, timestamp, result.text
    )
}

/// Parse metadata from a Markdown-formatted transcription document.
///
/// Extracts the source filename and timestamp from the header section.
/// Returns `None` if the header cannot be parsed.
pub fn parse_markdown_header(markdown: &str) -> Option<TranscriptionMetadata> {
    let mut source_filename: Option<String> = None;
    let mut timestamp: Option<DateTime<Utc>> = None;

    for line in markdown.lines() {
        if let Some(rest) = line.strip_prefix("- **Source:** ") {
            source_filename = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("- **Date:** ") {
            timestamp = rest.parse::<DateTime<Utc>>().ok();
        }
        // Stop parsing after the separator
        if line.starts_with("---") {
            break;
        }
    }

    match (source_filename, timestamp) {
        (Some(source_filename), Some(timestamp)) => Some(TranscriptionMetadata {
            source_filename,
            timestamp,
        }),
        _ => None,
    }
}

/// Export a transcription result to the specified format.
///
/// Writes the content to `output_path` using UTF-8 encoding.
/// Returns `ExportError::IoError` if the file cannot be written.
pub fn export(
    result: &TranscriptionResult,
    output_path: &Path,
    format: ExportFormat,
) -> Result<(), ExportError> {
    let content = match format {
        ExportFormat::Txt => result.text.clone(),
        ExportFormat::Markdown => format_markdown(result),
    };

    fs::write(output_path, content.as_bytes()).map_err(|source| ExportError::IoError {
        path: output_path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use chrono::TimeZone;
    use tempfile::TempDir;

    fn sample_result() -> TranscriptionResult {
        TranscriptionResult {
            text: "Dies ist ein Test.".to_string(),
            source_file: PathBuf::from("/audio/interview.mp3"),
            timestamp: Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap(),
        }
    }

    #[test]
    fn test_export_txt_writes_text_content() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("output.txt");
        let result = sample_result();

        export(&result, &output, ExportFormat::Txt).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert_eq!(content, "Dies ist ein Test.");
    }

    #[test]
    fn test_export_txt_utf8_encoding() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("output.txt");
        let result = TranscriptionResult {
            text: "\u{00DC} \u{00D6} \u{00C4} \u{00DF} \u{2014} \u{201E}Hallo Welt\u{201C}".to_string(),
            source_file: PathBuf::from("/audio/test.wav"),
            timestamp: Utc::now(),
        };

        export(&result, &output, ExportFormat::Txt).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert_eq!(content, "\u{00DC} \u{00D6} \u{00C4} \u{00DF} \u{2014} \u{201E}Hallo Welt\u{201C}");
    }

    #[test]
    fn test_export_txt_io_error_on_invalid_path() {
        let result = sample_result();
        let bad_path = Path::new("/nonexistent/directory/output.txt");

        let err = export(&result, bad_path, ExportFormat::Txt).unwrap_err();
        match err {
            ExportError::IoError { path, .. } => {
                assert_eq!(path, bad_path);
            }
        }
    }

    #[test]
    fn test_format_markdown_structure() {
        let result = sample_result();
        let md = format_markdown(&result);

        assert!(md.starts_with("# Transcription\n"));
        assert!(md.contains("- **Source:** interview.mp3"));
        assert!(md.contains("- **Date:** 2024-01-15T10:30:00Z"));
        assert!(md.contains("---"));
        assert!(md.contains("Dies ist ein Test."));
    }

    #[test]
    fn test_format_markdown_exact_format() {
        let result = sample_result();
        let md = format_markdown(&result);

        let expected = "# Transcription\n\n- **Source:** interview.mp3\n- **Date:** 2024-01-15T10:30:00Z\n\n---\n\nDies ist ein Test.";
        assert_eq!(md, expected);
    }

    #[test]
    fn test_parse_markdown_header_valid() {
        let result = sample_result();
        let md = format_markdown(&result);

        let metadata = parse_markdown_header(&md).unwrap();
        assert_eq!(metadata.source_filename, "interview.mp3");
        assert_eq!(
            metadata.timestamp,
            Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap()
        );
    }

    #[test]
    fn test_parse_markdown_header_returns_none_for_invalid() {
        assert!(parse_markdown_header("just some text").is_none());
        assert!(parse_markdown_header("").is_none());
        assert!(parse_markdown_header("# Transcription\n\n- **Source:** file.mp3").is_none());
    }

    #[test]
    fn test_parse_markdown_header_round_trip() {
        let result = TranscriptionResult {
            text: "Hallo Welt".to_string(),
            source_file: PathBuf::from("/path/to/audio datei.mp3"),
            timestamp: Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap(),
        };

        let md = format_markdown(&result);
        let metadata = parse_markdown_header(&md).unwrap();

        assert_eq!(metadata.source_filename, "audio datei.mp3");
        assert_eq!(
            metadata.timestamp,
            Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap()
        );
    }

    #[test]
    fn test_export_markdown_writes_file() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("output.md");
        let result = sample_result();

        export(&result, &output, ExportFormat::Markdown).unwrap();

        let content = fs::read_to_string(&output).unwrap();
        assert!(content.contains("# Transcription"));
        assert!(content.contains("interview.mp3"));
        assert!(content.contains("Dies ist ein Test."));
    }

    #[test]
    fn test_export_markdown_io_error_on_invalid_path() {
        let result = sample_result();
        let bad_path = Path::new("/nonexistent/directory/output.md");

        let err = export(&result, bad_path, ExportFormat::Markdown).unwrap_err();
        match err {
            ExportError::IoError { path, .. } => {
                assert_eq!(path, bad_path);
            }
        }
    }
}
