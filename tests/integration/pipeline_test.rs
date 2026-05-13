//! Integration tests for the Klartext-Rust audio processing pipeline.
//!
//! Tests cover:
//! - Audio conversion pipeline (WAV format conversion and verification)
//! - Export round-trip with file I/O (TXT and Markdown)
//! - Full pipeline: validate → convert → verify WAV output format
//!
//! Note: Tests requiring a real MP3 fixture file are gated behind the
//! presence of `tests/fixtures/sample.mp3`. If the fixture is not present,
//! those tests are skipped with a message.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use tempfile::TempDir;

use klartext_rust::core::converter::{cleanup_temp_file, convert_to_wav};
use klartext_rust::core::exporter::{export, format_markdown, parse_markdown_header, ExportFormat};
use klartext_rust::core::validator::{validate_audio_file, SupportedFormat};
use klartext_rust::models::TranscriptionResult;

/// Helper: Create a synthetic WAV file at a given sample rate and channel count.
fn create_synthetic_wav(dir: &Path, name: &str, sample_rate: u32, channels: u16) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut writer = WavWriter::create(&path, spec).expect("Failed to create WAV writer");

    // Generate 0.5 seconds of a 440Hz sine wave (per channel)
    let num_samples = (sample_rate as f32 * 0.5) as usize;
    for i in 0..num_samples {
        let sample = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate as f32).sin();
        for _ in 0..channels {
            writer.write_sample(sample).expect("Failed to write sample");
        }
    }

    writer.finalize().expect("Failed to finalize WAV");
    path
}

/// Test: Converting a 44100Hz stereo WAV produces a 16kHz mono WAV output.
///
/// Validates: Requirements 4.1, 4.2
#[test]
fn test_wav_conversion_pipeline() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    // Create a 44100Hz stereo WAV file
    let input_path = create_synthetic_wav(dir.path(), "input_44100_stereo.wav", 44_100, 2);

    // Validate the input file
    let format = validate_audio_file(&input_path).expect("Validation should pass for valid WAV");
    assert_eq!(format, SupportedFormat::Wav);

    // Convert to 16kHz mono
    let converted = convert_to_wav(&input_path).expect("Conversion should succeed");
    assert!(converted.is_temporary, "Non-compatible WAV should produce a temporary file");
    assert!(converted.wav_path.exists(), "Converted file should exist");

    // Verify the output WAV format
    let reader = WavReader::open(&converted.wav_path).expect("Should open converted WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "Output should be 16kHz");
    assert_eq!(spec.channels, 1, "Output should be mono");

    // Verify the output has samples (non-empty)
    let sample_count = reader.len();
    assert!(sample_count > 0, "Output should contain audio samples");

    // Cleanup
    cleanup_temp_file(&converted);
    assert!(
        !converted.wav_path.exists(),
        "Temp file should be removed after cleanup"
    );
}

/// Test: A 16kHz mono WAV file is not re-converted (passthrough).
///
/// Validates: Requirements 4.1
#[test]
fn test_compatible_wav_passthrough() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    // Create a 16kHz mono WAV file (already compatible)
    let input_path = create_synthetic_wav(dir.path(), "input_16000_mono.wav", 16_000, 1);

    // Convert — should return the original path without creating a temp file
    let converted = convert_to_wav(&input_path).expect("Conversion should succeed");
    assert!(
        !converted.is_temporary,
        "Compatible WAV should not be marked temporary"
    );
    assert_eq!(
        converted.wav_path, input_path,
        "Compatible WAV should return original path"
    );
}

/// Test: Full pipeline validate → convert → verify WAV output format.
///
/// Validates: Requirements 4.1, 4.2
#[test]
fn test_full_pipeline_validate_convert_verify() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    // Create a 48kHz stereo WAV (common recording format)
    let input_path = create_synthetic_wav(dir.path(), "recording_48k.wav", 48_000, 2);

    // Step 1: Validate
    let format = validate_audio_file(&input_path).expect("Validation should pass");
    assert_eq!(format, SupportedFormat::Wav);

    // Step 2: Convert
    let converted = convert_to_wav(&input_path).expect("Conversion should succeed");
    assert!(converted.is_temporary);

    // Step 3: Verify output format
    let reader = WavReader::open(&converted.wav_path).expect("Should open converted WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "Must be 16kHz for Parakeet TDT");
    assert_eq!(spec.channels, 1, "Must be mono for Parakeet TDT");
    assert!(
        matches!(spec.sample_format, SampleFormat::Float | SampleFormat::Int),
        "Must be PCM format"
    );

    // Verify audio duration is approximately correct
    // Input: 0.5s at 48kHz stereo → Output: ~0.5s at 16kHz mono = ~8000 samples
    let output_samples = reader.len();
    let expected_samples = 8_000u32;
    let tolerance = 100u32; // Allow some tolerance for resampling
    assert!(
        output_samples.abs_diff(expected_samples) < tolerance,
        "Output sample count {} should be approximately {} (±{})",
        output_samples,
        expected_samples,
        tolerance
    );

    // Cleanup
    cleanup_temp_file(&converted);
}

/// Test: Export to TXT round-trip with file I/O.
///
/// Validates: Requirements 8.1
#[test]
fn test_export_txt_round_trip() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = dir.path().join("transcription.txt");

    let result = TranscriptionResult {
        text: "Dies ist ein Integrationstest. Ü Ö Ä ß sind wichtig.".to_string(),
        source_file: PathBuf::from("/audio/interview.wav"),
        timestamp: Utc.with_ymd_and_hms(2024, 6, 15, 14, 30, 0).unwrap(),
    };

    // Export to TXT
    export(&result, &output_path, ExportFormat::Txt).expect("TXT export should succeed");

    // Read back and verify
    let content = fs::read_to_string(&output_path).expect("Should read exported file");
    assert_eq!(
        content, result.text,
        "TXT export should contain exactly the transcription text"
    );

    // Verify UTF-8 encoding by checking the raw bytes
    let bytes = fs::read(&output_path).expect("Should read file bytes");
    let decoded = String::from_utf8(bytes).expect("File should be valid UTF-8");
    assert_eq!(decoded, result.text);
}

/// Test: Export to Markdown round-trip with file I/O and metadata verification.
///
/// Validates: Requirements 9.1
#[test]
fn test_export_markdown_round_trip() {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let output_path = dir.path().join("transcription.md");

    let result = TranscriptionResult {
        text: "Hallo Welt, dies ist eine Transkription.".to_string(),
        source_file: PathBuf::from("/audio/podcast episode 42.mp3"),
        timestamp: Utc.with_ymd_and_hms(2024, 3, 20, 9, 15, 30).unwrap(),
    };

    // Export to Markdown
    export(&result, &output_path, ExportFormat::Markdown).expect("Markdown export should succeed");

    // Read back the file
    let content = fs::read_to_string(&output_path).expect("Should read exported file");

    // Verify structure
    assert!(content.starts_with("# Transcription\n"), "Should have heading");
    assert!(
        content.contains("podcast episode 42.mp3"),
        "Should contain source filename"
    );
    assert!(
        content.contains("2024-03-20T09:15:30Z"),
        "Should contain ISO 8601 timestamp"
    );
    assert!(
        content.contains("Hallo Welt, dies ist eine Transkription."),
        "Should contain transcription text"
    );

    // Verify metadata round-trip via parse_markdown_header
    let metadata = parse_markdown_header(&content).expect("Should parse markdown header");
    assert_eq!(metadata.source_filename, "podcast episode 42.mp3");
    assert_eq!(
        metadata.timestamp,
        Utc.with_ymd_and_hms(2024, 3, 20, 9, 15, 30).unwrap()
    );

    // Verify UTF-8 encoding
    let bytes = fs::read(&output_path).expect("Should read file bytes");
    String::from_utf8(bytes).expect("File should be valid UTF-8");
}

/// Test: MP3 conversion pipeline with a real fixture file.
///
/// This test is skipped if `tests/fixtures/sample.mp3` does not exist.
/// To run this test, place a valid MP3 file at that path.
///
/// Validates: Requirements 4.1, 4.2
#[test]
fn test_mp3_conversion_with_fixture() {
    let fixture_path = PathBuf::from("tests/fixtures/sample.mp3");

    if !fixture_path.exists() {
        eprintln!(
            "SKIPPED: test_mp3_conversion_with_fixture - \
             Place a valid MP3 file at tests/fixtures/sample.mp3 to enable this test"
        );
        return;
    }

    // Validate the MP3 fixture
    let format = validate_audio_file(&fixture_path).expect("MP3 fixture should pass validation");
    assert_eq!(format, SupportedFormat::Mp3);

    // Convert to WAV
    let converted = convert_to_wav(&fixture_path).expect("MP3 conversion should succeed");
    assert!(converted.is_temporary, "MP3 conversion should produce a temp file");
    assert!(converted.wav_path.exists(), "Converted WAV should exist");

    // Verify output format
    let reader = WavReader::open(&converted.wav_path).expect("Should open converted WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "Output should be 16kHz");
    assert_eq!(spec.channels, 1, "Output should be mono");
    assert!(reader.len() > 0, "Output should have samples");

    // Cleanup
    cleanup_temp_file(&converted);
    assert!(!converted.wav_path.exists(), "Temp file should be cleaned up");
}
