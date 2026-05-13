use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use parakeet_rs::{ParakeetTDT, Transcriber, TimestampMode};

use crate::errors::EngineError;

/// Wraps a loaded Parakeet TDT model and provides transcription capabilities.
pub struct TranscriptionEngine {
    model: ParakeetTDT,
}

impl std::fmt::Debug for TranscriptionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionEngine").finish_non_exhaustive()
    }
}

impl TranscriptionEngine {
    /// Load a Parakeet TDT model from the given directory path.
    ///
    /// The directory should contain:
    /// - encoder-model.onnx (+ encoder-model.onnx.data)
    /// - decoder_joint-model.onnx
    /// - vocab.txt
    ///
    /// Returns `EngineError::ModelLoadFailed` if the model cannot be loaded.
    pub fn load(model_path: &Path) -> Result<Self, EngineError> {
        let path_str = model_path.to_str().unwrap_or(".");

        // Try DirectML (GPU) first, fall back to CPU if unavailable
        let config = parakeet_rs::ExecutionConfig::new()
            .with_execution_provider(parakeet_rs::ExecutionProvider::DirectML);

        tracing::info!("Attempting to load model with DirectML GPU acceleration...");

        let model = match ParakeetTDT::from_pretrained(path_str, Some(config)) {
            Ok(m) => {
                tracing::info!("Model loaded with DirectML GPU acceleration");
                m
            }
            Err(gpu_err) => {
                tracing::warn!("DirectML not available ({}), falling back to CPU", gpu_err);
                ParakeetTDT::from_pretrained(path_str, None)
                    .map_err(|e| EngineError::ModelLoadFailed {
                        path: model_path.to_path_buf(),
                        detail: e.to_string(),
                    })?
            }
        };

        Ok(Self { model })
    }

    /// Maximum chunk duration in samples (4 minutes at 16kHz).
    /// Parakeet TDT has a ~10,000 frame limit which corresponds to roughly 4-5 minutes.
    const MAX_CHUNK_SAMPLES: usize = 16000 * 240; // 4 minutes

    /// Transcribe audio samples (expected: 16kHz mono f32).
    ///
    /// For audio longer than 4 minutes, automatically splits into chunks
    /// and concatenates the results.
    ///
    /// - `abort_flag`: set to `true` from another thread to cancel transcription.
    /// - `progress_cb`: called with progress values in 0.0..1.0 range.
    ///
    /// Returns the transcribed text on success, `EngineError::Cancelled` if aborted,
    /// or `EngineError::TranscriptionFailed` on other errors.
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        abort_flag: &AtomicBool,
        progress_cb: impl Fn(f32) + 'static,
    ) -> Result<String, EngineError> {
        // Check cancellation before starting
        if abort_flag.load(Ordering::Relaxed) {
            return Err(EngineError::Cancelled);
        }

        let total_samples = samples.len();
        tracing::info!(
            "Starting Parakeet TDT transcription: {} samples ({:.1}s)",
            total_samples,
            total_samples as f64 / 16000.0
        );

        // If audio is short enough, process in one shot
        if total_samples <= Self::MAX_CHUNK_SAMPLES {
            return self.transcribe_chunk(samples, abort_flag);
        }

        // Split into chunks for long audio
        let chunks: Vec<&[f32]> = samples.chunks(Self::MAX_CHUNK_SAMPLES).collect();
        let num_chunks = chunks.len();
        tracing::info!("Splitting into {} chunks for processing", num_chunks);

        let mut full_text = String::new();

        for (i, chunk) in chunks.iter().enumerate() {
            // Check cancellation between chunks
            if abort_flag.load(Ordering::Relaxed) {
                return Err(EngineError::Cancelled);
            }

            // Report progress
            let progress = i as f32 / num_chunks as f32;
            progress_cb(progress);

            tracing::info!(
                "Processing chunk {}/{} ({:.1}s)",
                i + 1,
                num_chunks,
                chunk.len() as f64 / 16000.0
            );

            let chunk_text = self.transcribe_chunk(chunk, abort_flag)?;

            if !full_text.is_empty() && !chunk_text.is_empty() {
                full_text.push(' ');
            }
            full_text.push_str(&chunk_text);
        }

        progress_cb(1.0);
        Ok(full_text.trim().to_string())
    }

    /// Transcribe a single chunk of audio.
    fn transcribe_chunk(
        &mut self,
        samples: &[f32],
        abort_flag: &AtomicBool,
    ) -> Result<String, EngineError> {
        let result = self.model.transcribe_samples(
            samples.to_vec(),
            16000,
            1,
            Some(TimestampMode::Sentences),
        ).map_err(|e| {
            if abort_flag.load(Ordering::Relaxed) {
                return EngineError::Cancelled;
            }
            EngineError::TranscriptionFailed {
                detail: e.to_string(),
            }
        })?;

        if abort_flag.load(Ordering::Relaxed) {
            return Err(EngineError::Cancelled);
        }

        Ok(result.text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn test_load_nonexistent_model() {
        let path = PathBuf::from("/nonexistent/path/to/model");
        let result = TranscriptionEngine::load(&path);

        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::ModelLoadFailed {
                path: err_path,
                detail,
            } => {
                assert_eq!(err_path, path);
                assert!(!detail.is_empty(), "Error detail should not be empty");
            }
            other => panic!("Expected ModelLoadFailed, got: {:?}", other),
        }
    }

    #[test]
    fn test_cancellation_flag_mechanism() {
        let abort_flag = Arc::new(AtomicBool::new(false));

        assert!(!abort_flag.load(Ordering::Relaxed));

        let flag_clone = Arc::clone(&abort_flag);
        let handle = std::thread::spawn(move || {
            flag_clone.store(true, Ordering::Relaxed);
        });
        handle.join().unwrap();

        assert!(abort_flag.load(Ordering::Relaxed));

        abort_flag.store(false, Ordering::Relaxed);
        assert!(!abort_flag.load(Ordering::Relaxed));
    }
}
