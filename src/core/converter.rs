// Audio format conversion logic
// Converts audio files to 16kHz mono WAV suitable for Parakeet TDT transcription.

use std::fs::File;
use std::path::{Path, PathBuf};

use hound::{WavReader, WavSpec, WavWriter};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::debug;

use crate::errors::ConversionError;

/// Target sample rate for Parakeet TDT transcription.
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Result of audio conversion, containing the path to the WAV file
/// and whether it is a temporary file that should be cleaned up.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedAudio {
    pub wav_path: PathBuf,
    pub is_temporary: bool,
}

/// Convert an audio file to 16kHz mono WAV.
/// If the input is already a compatible WAV (16kHz, mono, PCM), returns the original path.
pub fn convert_to_wav(input: &Path) -> Result<ConvertedAudio, ConversionError> {
    // First, check if the input is already a compatible WAV
    if is_compatible_wav(input) {
        debug!("File is already compatible WAV: {:?}", input);
        return Ok(ConvertedAudio {
            wav_path: input.to_path_buf(),
            is_temporary: false,
        });
    }

    // Decode the audio file using symphonia
    let samples = decode_audio(input)?;

    // Write the samples to a temporary WAV file
    let temp_path = create_temp_wav_path(input);
    write_wav(&temp_path, &samples)?;

    debug!("Converted {:?} to temporary WAV: {:?}", input, temp_path);

    Ok(ConvertedAudio {
        wav_path: temp_path,
        is_temporary: true,
    })
}

/// Remove temporary conversion artifacts.
pub fn cleanup_temp_file(converted: &ConvertedAudio) {
    if converted.is_temporary {
        if let Err(e) = std::fs::remove_file(&converted.wav_path) {
            debug!(
                "Failed to remove temporary file {:?}: {}",
                converted.wav_path, e
            );
        } else {
            debug!("Removed temporary file: {:?}", converted.wav_path);
        }
    }
}

/// Check if a file is already a compatible WAV (16kHz, mono, PCM format).
fn is_compatible_wav(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    if extension.to_lowercase() != "wav" {
        return false;
    }

    let reader = match WavReader::open(path) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let spec = reader.spec();

    // Compatible if: 16kHz, mono, and a PCM format (integer or float)
    spec.sample_rate == TARGET_SAMPLE_RATE
        && spec.channels == 1
        && matches!(
            spec.sample_format,
            hound::SampleFormat::Int | hound::SampleFormat::Float
        )
}

/// Decode an audio file using symphonia, returning 16kHz mono f32 samples.
fn decode_audio(path: &Path) -> Result<Vec<f32>, ConversionError> {
    let file = File::open(path).map_err(|e| ConversionError::DecodingFailed {
        path: path.to_path_buf(),
        detail: format!("Cannot open file: {}", e),
    })?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Provide a hint about the file extension to help symphonia probe
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| ConversionError::DecodingFailed {
            path: path.to_path_buf(),
            detail: format!("Failed to probe audio format: {}", e),
        })?;

    let mut format_reader = probed.format;

    // Get the default audio track
    let track = format_reader
        .default_track()
        .ok_or_else(|| ConversionError::DecodingFailed {
            path: path.to_path_buf(),
            detail: "No audio track found in file".to_string(),
        })?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let source_sample_rate = codec_params.sample_rate.ok_or_else(|| {
        ConversionError::DecodingFailed {
            path: path.to_path_buf(),
            detail: "Cannot determine sample rate".to_string(),
        }
    })?;

    let source_channels = codec_params
        .channels
        .map(|ch| ch.count())
        .unwrap_or(1) as usize;

    // Create a decoder for the track
    let decoder_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &decoder_opts)
        .map_err(|e| ConversionError::DecodingFailed {
            path: path.to_path_buf(),
            detail: format!("Failed to create decoder: {}", e),
        })?;

    // Decode all packets into interleaved f32 samples
    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format_reader.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // End of stream
            }
            Err(_e) => {
                break; // End of stream or non-fatal error
            }
        };

        // Skip packets not belonging to our track
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                continue; // Skip corrupted packets
            }
            Err(e) => {
                return Err(ConversionError::DecodingFailed {
                    path: path.to_path_buf(),
                    detail: format!("Decoding error: {}", e),
                });
            }
        };

        // Convert decoded audio buffer to f32 samples
        let spec = *decoded.spec();
        let num_frames = decoded.frames();
        let num_channels = spec.channels.count();

        let mut sample_buf = SampleBuffer::<f32>::new(
            num_frames as u64,
            symphonia::core::audio::SignalSpec::new(spec.rate, spec.channels),
        );
        sample_buf.copy_interleaved_ref(decoded);

        all_samples.extend_from_slice(sample_buf.samples());
        // Update source_channels from actual decoded data if needed
        let _ = num_channels;
    }

    if all_samples.is_empty() {
        return Err(ConversionError::DecodingFailed {
            path: path.to_path_buf(),
            detail: "No audio samples decoded from file".to_string(),
        });
    }

    // Convert to mono if multi-channel
    let mono_samples = if source_channels > 1 {
        convert_to_mono(&all_samples, source_channels)
    } else {
        all_samples
    };

    // Resample to 16kHz if needed
    let resampled = if source_sample_rate != TARGET_SAMPLE_RATE {
        resample(&mono_samples, source_sample_rate, TARGET_SAMPLE_RATE)?
    } else {
        mono_samples
    };

    Ok(resampled)
}

/// Convert interleaved multi-channel samples to mono by averaging channels.
fn convert_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 || channels == 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample audio using linear interpolation.
fn resample(
    samples: &[f32],
    source_rate: u32,
    target_rate: u32,
) -> Result<Vec<f32>, ConversionError> {
    if source_rate == 0 {
        return Err(ConversionError::ResamplingFailed {
            detail: "Source sample rate is zero".to_string(),
        });
    }

    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let ratio = target_rate as f64 / source_rate as f64;
    let output_len = (samples.len() as f64 * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos.floor() as usize;
        let frac = (src_pos - src_idx as f64) as f32;

        let sample = if src_idx + 1 < samples.len() {
            samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac
        } else if src_idx < samples.len() {
            samples[src_idx]
        } else {
            0.0
        };

        output.push(sample);
    }

    Ok(output)
}

/// Write f32 samples to a WAV file at 16kHz mono.
fn write_wav(path: &Path, samples: &[f32]) -> Result<(), ConversionError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec).map_err(|e| ConversionError::WriteFailed {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
    })?;

    for &sample in samples {
        writer
            .write_sample(sample)
            .map_err(|e| ConversionError::WriteFailed {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;
    }

    writer
        .finalize()
        .map_err(|e| ConversionError::WriteFailed {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?;

    Ok(())
}

/// Create a unique temporary file path for the converted WAV.
fn create_temp_wav_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let filename = format!("klartext_{}_{}.wav", stem, timestamp);
    std::env::temp_dir().join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::fs;
    use tempfile::NamedTempFile;

    /// Helper: create a WAV file with the given spec and samples.
    fn create_wav_file(spec: WavSpec, samples: &[f32]) -> NamedTempFile {
        let tmp = tempfile::Builder::new()
            .suffix(".wav")
            .tempfile()
            .expect("failed to create temp file");
        let mut writer = WavWriter::create(tmp.path(), spec).expect("failed to create WAV writer");
        for &s in samples {
            writer.write_sample(s).expect("failed to write sample");
        }
        writer.finalize().expect("failed to finalize WAV");
        tmp
    }

    #[test]
    fn test_compatible_wav_not_converted() {
        // Create a 16kHz mono WAV file — should be returned as-is
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 / 1600.0).sin()).collect();
        let tmp = create_wav_file(spec, &samples);

        let result = convert_to_wav(tmp.path()).expect("convert_to_wav failed");

        assert!(!result.is_temporary, "compatible WAV should not be marked temporary");
        assert_eq!(
            result.wav_path,
            tmp.path().to_path_buf(),
            "compatible WAV path should match input"
        );
    }

    #[test]
    fn test_incompatible_wav_converted() {
        // Create a 44100Hz stereo WAV file — should be converted
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        // Generate 1 second of stereo silence (interleaved)
        let samples: Vec<f32> = vec![0.0; 44_100 * 2];
        let tmp = create_wav_file(spec, &samples);

        let result = convert_to_wav(tmp.path()).expect("convert_to_wav failed");

        assert!(result.is_temporary, "incompatible WAV should be marked temporary");
        assert!(
            result.wav_path.exists(),
            "converted output file should exist"
        );
        assert_ne!(
            result.wav_path,
            tmp.path().to_path_buf(),
            "converted path should differ from input"
        );

        // Verify the output is 16kHz mono
        let reader = WavReader::open(&result.wav_path).expect("failed to open converted WAV");
        let out_spec = reader.spec();
        assert_eq!(out_spec.sample_rate, 16_000);
        assert_eq!(out_spec.channels, 1);

        // Cleanup
        cleanup_temp_file(&result);
    }

    #[test]
    fn test_cleanup_removes_temp_file() {
        // Create a real temp file and mark it as temporary
        let tmp = NamedTempFile::new().expect("failed to create temp file");
        // Keep the file on disk by persisting it
        let (_file, persisted_path) = tmp.keep().expect("failed to persist temp file");

        let converted = ConvertedAudio {
            wav_path: persisted_path.clone(),
            is_temporary: true,
        };

        assert!(persisted_path.exists(), "file should exist before cleanup");
        cleanup_temp_file(&converted);
        assert!(
            !persisted_path.exists(),
            "temporary file should be deleted after cleanup"
        );
    }

    #[test]
    fn test_cleanup_skips_non_temporary() {
        // Create a real temp file but mark it as non-temporary
        let tmp = NamedTempFile::new().expect("failed to create temp file");
        let (_file, persisted_path) = tmp.keep().expect("failed to persist temp file");

        let converted = ConvertedAudio {
            wav_path: persisted_path.clone(),
            is_temporary: false,
        };

        assert!(persisted_path.exists(), "file should exist before cleanup");
        cleanup_temp_file(&converted);
        assert!(
            persisted_path.exists(),
            "non-temporary file should NOT be deleted after cleanup"
        );

        // Manual cleanup
        fs::remove_file(&persisted_path).ok();
    }

    #[test]
    fn test_convert_to_mono_averages_channels() {
        // Interleaved stereo: [L0, R0, L1, R1, ...]
        let interleaved = vec![1.0, 0.0, 0.5, 0.5, 0.0, 1.0];
        let mono = convert_to_mono(&interleaved, 2);

        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < 1e-6, "frame 0: avg of 1.0 and 0.0 should be 0.5");
        assert!((mono[1] - 0.5).abs() < 1e-6, "frame 1: avg of 0.5 and 0.5 should be 0.5");
        assert!((mono[2] - 0.5).abs() < 1e-6, "frame 2: avg of 0.0 and 1.0 should be 0.5");
    }

    #[test]
    fn test_resample_identity() {
        // Resampling from 16kHz to 16kHz should return (approximately) the same samples
        let samples: Vec<f32> = (0..160).map(|i| (i as f32 / 160.0).sin()).collect();
        let resampled = resample(&samples, 16_000, 16_000).expect("resample failed");

        assert_eq!(
            resampled.len(),
            samples.len(),
            "identity resample should preserve length"
        );
        for (i, (&orig, &res)) in samples.iter().zip(resampled.iter()).enumerate() {
            assert!(
                (orig - res).abs() < 1e-5,
                "sample {} differs: orig={}, resampled={}",
                i,
                orig,
                res
            );
        }
    }
}
