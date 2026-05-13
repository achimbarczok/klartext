// Feature: klartext-rust, Property 4: TXT export round-trip
//
// **Validates: Requirements 8.1, 8.2**
//
// For any valid TranscriptionResult, exporting to TXT format and then reading the file content
// back SHALL produce a string equal to the original `text` field of the TranscriptionResult.

// Feature: klartext-rust, Property 5: Markdown export metadata round-trip
//
// **Validates: Requirements 9.2, 10.1, 10.2**
//
// For any valid TranscriptionResult, formatting to Markdown via `format_markdown` and then
// parsing the header via `parse_markdown_header` SHALL recover the original source filename
// and timestamp.

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use proptest::prelude::*;
use tempfile::TempDir;

use klartext_rust::core::exporter::{export, format_markdown, parse_markdown_header, ExportFormat};
use klartext_rust::models::TranscriptionResult;

/// Strategy to generate arbitrary unicode text (0–500 chars) for transcription content.
fn arb_text() -> impl Strategy<Value = String> {
    "\\PC{0,500}"
}

/// Strategy to generate valid filenames (no path separators, 1–50 chars).
fn arb_filename() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_.-]{1,50}"
}

/// Strategy to generate random timestamps as DateTime<Utc> from random i64 seconds.
/// We constrain to a reasonable range to avoid overflow issues with chrono.
fn arb_timestamp() -> impl Strategy<Value = DateTime<Utc>> {
    // Range: year 1970 to year 2100 approximately
    (0i64..4_102_444_800i64).prop_map(|secs| Utc.timestamp_opt(secs, 0).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 4: TXT export round-trip.
    /// Exporting a TranscriptionResult to TXT and reading back produces the original text.
    #[test]
    fn txt_export_round_trip(text in arb_text()) {
        let result = TranscriptionResult {
            text: text.clone(),
            source_file: PathBuf::from("/tmp/test.wav"),
            timestamp: Utc::now(),
        };

        let dir = TempDir::new().unwrap();
        let output_path = dir.path().join("output.txt");

        export(&result, &output_path, ExportFormat::Txt).unwrap();

        let read_back = fs::read_to_string(&output_path).unwrap();
        prop_assert_eq!(read_back, text, "TXT round-trip failed: content mismatch");
    }

    /// Property 5: Markdown export metadata round-trip.
    /// Formatting to Markdown and parsing the header recovers the original filename and timestamp.
    #[test]
    fn markdown_metadata_round_trip(
        filename in arb_filename(),
        timestamp in arb_timestamp(),
    ) {
        let source_path = PathBuf::from(format!("/audio/{}", filename));
        let result = TranscriptionResult {
            text: "Some transcription text".to_string(),
            source_file: source_path,
            timestamp,
        };

        let markdown = format_markdown(&result);
        let metadata = parse_markdown_header(&markdown);

        prop_assert!(
            metadata.is_some(),
            "parse_markdown_header returned None for generated markdown"
        );

        let metadata = metadata.unwrap();

        // Assert recovered filename matches original
        prop_assert_eq!(
            &metadata.source_filename,
            &filename,
            "Markdown round-trip failed: filename mismatch"
        );

        // Assert recovered timestamp matches original (seconds precision since format uses %S)
        prop_assert_eq!(
            metadata.timestamp,
            timestamp,
            "Markdown round-trip failed: timestamp mismatch"
        );
    }
}
