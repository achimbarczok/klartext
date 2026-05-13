use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::errors::ValidationError;

/// Supported audio formats for transcription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedFormat {
    Wav,
    Mp3,
}

/// Validate a file for transcription readiness.
///
/// Checks are performed in order:
/// 1. File extension must be `.wav` or `.mp3` (case-insensitive)
/// 2. File must be readable (can be opened)
/// 3. File header must contain valid WAV (RIFF) or MP3 (ID3/sync word) magic bytes
pub fn validate_audio_file(path: &Path) -> Result<SupportedFormat, ValidationError> {
    // Step 1: Check file extension
    let format = check_extension(path)?;

    // Step 2: Check file readability
    let mut file = File::open(path).map_err(|e| ValidationError::FileNotReadable {
        path: path.to_path_buf(),
        source: e,
    })?;

    // Step 3: Read header bytes and validate magic bytes
    let mut header = [0u8; 12];
    let bytes_read = file.read(&mut header).map_err(|e| ValidationError::FileNotReadable {
        path: path.to_path_buf(),
        source: e,
    })?;

    validate_header(path, format, &header[..bytes_read])?;

    Ok(format)
}

/// Check the file extension and return the corresponding format.
fn check_extension(path: &Path) -> Result<SupportedFormat, ValidationError> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    match extension.to_lowercase().as_str() {
        "wav" => Ok(SupportedFormat::Wav),
        "mp3" => Ok(SupportedFormat::Mp3),
        _ => Err(ValidationError::UnsupportedExtension {
            path: path.to_path_buf(),
            extension: extension.to_string(),
        }),
    }
}

/// Validate the file header magic bytes for the expected format.
fn validate_header(
    path: &Path,
    format: SupportedFormat,
    header: &[u8],
) -> Result<(), ValidationError> {
    match format {
        SupportedFormat::Wav => validate_wav_header(path, header),
        SupportedFormat::Mp3 => validate_mp3_header(path, header),
    }
}

/// WAV files must start with "RIFF" (bytes: 0x52, 0x49, 0x46, 0x46).
fn validate_wav_header(path: &Path, header: &[u8]) -> Result<(), ValidationError> {
    if header.len() < 4 {
        return Err(ValidationError::InvalidAudioHeader {
            path: path.to_path_buf(),
            detail: "File too short to contain a valid WAV header".to_string(),
        });
    }

    if &header[..4] == b"RIFF" {
        Ok(())
    } else {
        Err(ValidationError::InvalidAudioHeader {
            path: path.to_path_buf(),
            detail: format!(
                "Expected RIFF header, found bytes: {:02X} {:02X} {:02X} {:02X}",
                header[0], header[1], header[2], header[3]
            ),
        })
    }
}

/// MP3 files must start with either:
/// - ID3 tag: bytes 0x49, 0x44, 0x33 ("ID3")
/// - MPEG sync word: 0xFF followed by a byte with upper 3 bits set (0xE0 mask)
fn validate_mp3_header(path: &Path, header: &[u8]) -> Result<(), ValidationError> {
    if header.len() < 2 {
        return Err(ValidationError::InvalidAudioHeader {
            path: path.to_path_buf(),
            detail: "File too short to contain a valid MP3 header".to_string(),
        });
    }

    // Check for ID3 tag (needs at least 3 bytes)
    if header.len() >= 3 && &header[..3] == b"ID3" {
        return Ok(());
    }

    // Check for MPEG sync word: 0xFF followed by byte with bits 0xE0 set
    if header[0] == 0xFF && (header[1] & 0xE0) == 0xE0 {
        return Ok(());
    }

    Err(ValidationError::InvalidAudioHeader {
        path: path.to_path_buf(),
        detail: format!(
            "Expected ID3 tag or MPEG sync word, found bytes: {:02X} {:02X}",
            header[0], header[1]
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_valid_wav_extension_lowercase() {
        let mut file = NamedTempFile::with_suffix(".wav").unwrap();
        // Write a valid RIFF header
        file.write_all(b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Wav);
    }

    #[test]
    fn test_valid_wav_extension_uppercase() {
        let mut file = NamedTempFile::with_suffix(".WAV").unwrap();
        file.write_all(b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Wav);
    }

    #[test]
    fn test_valid_wav_extension_mixed_case() {
        let mut file = NamedTempFile::with_suffix(".Wav").unwrap();
        file.write_all(b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Wav);
    }

    #[test]
    fn test_valid_mp3_with_id3_tag() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    #[test]
    fn test_valid_mp3_with_sync_word_fb() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(&[0xFF, 0xFB, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    #[test]
    fn test_valid_mp3_with_sync_word_fa() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(&[0xFF, 0xFA, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    #[test]
    fn test_valid_mp3_with_sync_word_f3() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(&[0xFF, 0xF3, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    #[test]
    fn test_valid_mp3_with_sync_word_e0() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        // 0xFF 0xE0 is the minimum valid sync word (all sync bits set)
        file.write_all(&[0xFF, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]).unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }

    #[test]
    fn test_unsupported_extension() {
        let file = NamedTempFile::with_suffix(".ogg").unwrap();
        let result = validate_audio_file(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::UnsupportedExtension { extension, .. } => {
                assert_eq!(extension, "ogg");
            }
            other => panic!("Expected UnsupportedExtension, got: {:?}", other),
        }
    }

    #[test]
    fn test_unsupported_extension_empty() {
        let file = NamedTempFile::with_suffix("noext").unwrap();
        let result = validate_audio_file(file.path());
        // File without a proper extension separator - tempfile adds the suffix directly
        // The path will end with "noext" but won't have a dot-separated extension
        assert!(result.is_err());
    }

    #[test]
    fn test_file_not_readable() {
        let path = Path::new("/nonexistent/path/to/file.wav");
        let result = validate_audio_file(path);
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::FileNotReadable { .. } => {}
            other => panic!("Expected FileNotReadable, got: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_wav_header() {
        let mut file = NamedTempFile::with_suffix(".wav").unwrap();
        file.write_all(b"NOT_RIFF_DATA_HERE").unwrap();
        let result = validate_audio_file(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { detail, .. } => {
                assert!(detail.contains("Expected RIFF header"));
            }
            other => panic!("Expected InvalidAudioHeader, got: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_mp3_header() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(b"NOT_MP3_DATA_HERE!").unwrap();
        let result = validate_audio_file(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { detail, .. } => {
                assert!(detail.contains("Expected ID3 tag or MPEG sync word"));
            }
            other => panic!("Expected InvalidAudioHeader, got: {:?}", other),
        }
    }

    #[test]
    fn test_wav_file_too_short() {
        let mut file = NamedTempFile::with_suffix(".wav").unwrap();
        file.write_all(b"RI").unwrap(); // Only 2 bytes, need at least 4
        let result = validate_audio_file(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { detail, .. } => {
                assert!(detail.contains("too short"));
            }
            other => panic!("Expected InvalidAudioHeader, got: {:?}", other),
        }
    }

    #[test]
    fn test_mp3_file_too_short() {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(b"X").unwrap(); // Only 1 byte, need at least 2
        let result = validate_audio_file(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            ValidationError::InvalidAudioHeader { detail, .. } => {
                assert!(detail.contains("too short"));
            }
            other => panic!("Expected InvalidAudioHeader, got: {:?}", other),
        }
    }

    #[test]
    fn test_mp3_extension_uppercase() {
        let mut file = NamedTempFile::with_suffix(".MP3").unwrap();
        file.write_all(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        let result = validate_audio_file(file.path());
        assert_eq!(result.unwrap(), SupportedFormat::Mp3);
    }
}
