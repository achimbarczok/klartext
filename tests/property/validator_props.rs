// Feature: klartext-rust, Property 1: File extension validation correctness
//
// **Validates: Requirements 3.1**
//
// For any file path, the File_Validator SHALL accept the file if and only if its extension
// (case-insensitive) is one of the Supported_Audio_Formats (.wav, .mp3). All other extensions
// SHALL be rejected with an `UnsupportedExtension` error.

use std::io::Write;

use proptest::prelude::*;
use tempfile::Builder;

use klartext_rust::core::validator::{validate_audio_file, SupportedFormat};
use klartext_rust::errors::ValidationError;

/// Valid WAV header bytes (RIFF magic).
const WAV_HEADER: &[u8] = b"RIFF\x00\x00\x00\x00WAVEfmt ";

/// Valid MP3 header bytes (ID3 tag).
const MP3_HEADER: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x00";

/// Strategy to generate random file extension strings including empty, unicode, and mixed case.
fn arb_extension() -> impl Strategy<Value = String> {
    prop_oneof![
        // Empty extension
        Just(String::new()),
        // Common invalid extensions
        prop::sample::select(vec![
            "ogg".to_string(),
            "flac".to_string(),
            "aac".to_string(),
            "txt".to_string(),
            "pdf".to_string(),
            "exe".to_string(),
            "m4a".to_string(),
            "wma".to_string(),
        ]),
        // Random ASCII strings (1-10 chars)
        "[a-zA-Z0-9]{1,10}",
        // Unicode strings
        "\\PC{1,8}",
    ]
}

/// Determine if an extension should be accepted as wav (case-insensitive).
fn is_wav_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("wav")
}

/// Determine if an extension should be accepted as mp3 (case-insensitive).
fn is_mp3_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("mp3")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 1: Valid wav extensions (any case) are accepted and return SupportedFormat::Wav.
    #[test]
    fn valid_wav_extensions_accepted(
        ext in prop::sample::select(vec![
            "wav".to_string(),
            "WAV".to_string(),
            "Wav".to_string(),
            "wAv".to_string(),
            "waV".to_string(),
            "WAv".to_string(),
            "wAV".to_string(),
            "WaV".to_string(),
        ])
    ) {
        let suffix = format!(".{}", ext);
        let mut file = Builder::new()
            .suffix(&suffix)
            .tempfile()
            .unwrap();
        file.write_all(WAV_HEADER).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_ok(), "Expected Ok for .{} extension, got {:?}", ext, result);
        prop_assert_eq!(result.unwrap(), SupportedFormat::Wav);
    }

    /// Property 1: Valid mp3 extensions (any case) are accepted and return SupportedFormat::Mp3.
    #[test]
    fn valid_mp3_extensions_accepted(
        ext in prop::sample::select(vec![
            "mp3".to_string(),
            "MP3".to_string(),
            "Mp3".to_string(),
            "mP3".to_string(),
            "MP3".to_string(),
            "mp3".to_string(),
        ])
    ) {
        let suffix = format!(".{}", ext);
        let mut file = Builder::new()
            .suffix(&suffix)
            .tempfile()
            .unwrap();
        file.write_all(MP3_HEADER).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_ok(), "Expected Ok for .{} extension, got {:?}", ext, result);
        prop_assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    /// Property 1: Random extensions that are NOT wav or mp3 (case-insensitive) produce
    /// UnsupportedExtension error.
    #[test]
    fn invalid_extensions_rejected(ext in arb_extension()) {
        // Skip if the generated extension happens to be a valid one
        prop_assume!(!is_wav_extension(&ext) && !is_mp3_extension(&ext));

        // For empty extension, create a file without a dot-separated extension.
        // For non-empty, use the generated extension as suffix.
        let suffix = if ext.is_empty() {
            String::new()
        } else {
            format!(".{}", ext)
        };

        // Some unicode extensions may not be valid for file creation on all platforms.
        // We attempt to create the file and skip if the OS rejects it.
        let file_result = if suffix.is_empty() {
            Builder::new().prefix("noext").suffix("").tempfile()
        } else {
            Builder::new().suffix(&suffix).tempfile()
        };

        let mut file = match file_result {
            Ok(f) => f,
            Err(_) => {
                // OS rejected the filename (e.g., invalid unicode on Windows)
                // This is fine - skip this case
                return Ok(());
            }
        };

        // Write some arbitrary content (doesn't matter since extension check comes first)
        file.write_all(b"arbitrary content").unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(
            result.is_err(),
            "Expected error for extension '{}', got {:?}",
            ext,
            result
        );

        match result.unwrap_err() {
            ValidationError::UnsupportedExtension { .. } => {
                // This is the expected error variant
            }
            other => {
                prop_assert!(
                    false,
                    "Expected UnsupportedExtension for extension '{}', got {:?}",
                    ext,
                    other
                );
            }
        }
    }
}

// Feature: klartext-rust, Property 2: Audio header validation detects invalid data
//
// **Validates: Requirements 3.2**
//
// For any byte sequence that does not begin with a valid WAV (RIFF) or MP3 (ID3/sync) header,
// the File_Validator SHALL reject the file with an `InvalidAudioHeader` error. For any byte
// sequence that begins with a valid header, the validator SHALL accept it.

/// Strategy to generate random byte vectors (4–1024 bytes) that do NOT start with
/// a valid WAV (RIFF) or MP3 (ID3/sync word) header.
fn arb_invalid_header_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 4..=1024).prop_filter(
        "must not start with a valid audio header",
        |bytes| {
            // Not a valid WAV header (first 4 bytes != "RIFF")
            let is_riff = bytes.len() >= 4 && &bytes[..4] == b"RIFF";
            // Not a valid MP3 ID3 header (first 3 bytes != "ID3")
            let is_id3 = bytes.len() >= 3 && &bytes[..3] == b"ID3";
            // Not a valid MP3 sync word (0xFF followed by byte with upper 3 bits set)
            let is_sync = bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0;
            !is_riff && !is_id3 && !is_sync
        },
    )
}

/// Strategy to generate valid WAV content: "RIFF" prefix followed by random bytes.
fn arb_valid_wav_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=1020).prop_map(|mut tail| {
        let mut bytes = b"RIFF".to_vec();
        bytes.append(&mut tail);
        bytes
    })
}

/// Strategy to generate valid MP3 content with ID3 header: "ID3" prefix followed by random bytes.
fn arb_valid_mp3_id3_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=1021).prop_map(|mut tail| {
        let mut bytes = b"ID3".to_vec();
        bytes.append(&mut tail);
        bytes
    })
}

/// Strategy to generate valid MP3 content with sync word: 0xFF + (0xE0 | lower_bits) followed by random bytes.
fn arb_valid_mp3_sync_bytes() -> impl Strategy<Value = Vec<u8>> {
    (any::<u8>(), proptest::collection::vec(any::<u8>(), 0..=1022)).prop_map(
        |(lower_bits, mut tail)| {
            let sync_byte = 0xE0 | (lower_bits & 0x1F); // Ensure upper 3 bits are set
            let mut bytes = vec![0xFF, sync_byte];
            bytes.append(&mut tail);
            bytes
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 2: Random bytes without valid audio headers are rejected with InvalidAudioHeader
    /// when written to a .wav file.
    #[test]
    fn invalid_wav_headers_rejected(content in arb_invalid_header_bytes()) {
        let mut file = Builder::new()
            .suffix(".wav")
            .tempfile()
            .unwrap();
        file.write_all(&content).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_err(), "Expected error for invalid WAV header, got {:?}", result);
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { .. } => {
                // Expected
            }
            other => {
                prop_assert!(false, "Expected InvalidAudioHeader, got {:?}", other);
            }
        }
    }

    /// Property 2: Random bytes without valid audio headers are rejected with InvalidAudioHeader
    /// when written to a .mp3 file.
    #[test]
    fn invalid_mp3_headers_rejected(content in arb_invalid_header_bytes()) {
        let mut file = Builder::new()
            .suffix(".mp3")
            .tempfile()
            .unwrap();
        file.write_all(&content).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_err(), "Expected error for invalid MP3 header, got {:?}", result);
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { .. } => {
                // Expected
            }
            other => {
                prop_assert!(false, "Expected InvalidAudioHeader, got {:?}", other);
            }
        }
    }

    /// Property 2: Bytes starting with "RIFF" are accepted as valid WAV files.
    #[test]
    fn valid_wav_headers_accepted(content in arb_valid_wav_bytes()) {
        let mut file = Builder::new()
            .suffix(".wav")
            .tempfile()
            .unwrap();
        file.write_all(&content).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_ok(), "Expected Ok for valid WAV header, got {:?}", result);
        prop_assert_eq!(result.unwrap(), SupportedFormat::Wav);
    }

    /// Property 2: Bytes starting with "ID3" are accepted as valid MP3 files.
    #[test]
    fn valid_mp3_id3_headers_accepted(content in arb_valid_mp3_id3_bytes()) {
        let mut file = Builder::new()
            .suffix(".mp3")
            .tempfile()
            .unwrap();
        file.write_all(&content).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_ok(), "Expected Ok for valid MP3 ID3 header, got {:?}", result);
        prop_assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    /// Property 2: Bytes starting with 0xFF followed by a byte with upper 3 bits set (sync word)
    /// are accepted as valid MP3 files.
    #[test]
    fn valid_mp3_sync_headers_accepted(content in arb_valid_mp3_sync_bytes()) {
        let mut file = Builder::new()
            .suffix(".mp3")
            .tempfile()
            .unwrap();
        file.write_all(&content).unwrap();
        file.flush().unwrap();

        let result = validate_audio_file(file.path());
        prop_assert!(result.is_ok(), "Expected Ok for valid MP3 sync word header, got {:?}", result);
        prop_assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }
}
