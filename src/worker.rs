// Worker thread for background processing.
// Handles model loading, transcription orchestration, and cancellation
// on a dedicated background thread, communicating with the GUI via channels.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use hound::WavReader;
use tracing::{debug, error, info};

use crate::core::converter::{cleanup_temp_file, convert_to_wav};
use crate::core::engine::TranscriptionEngine;
use crate::core::validator::validate_audio_file;
use crate::errors::EngineError;
use crate::models::TranscriptionResult;

/// Commands sent from the GUI thread to the worker thread.
#[derive(Debug)]
pub enum Command {
    /// Load the Parakeet TDT model from the given path.
    LoadModel(PathBuf),
    /// Transcribe the audio file at the given path.
    Transcribe(PathBuf),
    /// Cancel the current transcription.
    Cancel,
    /// Shut down the worker thread.
    Shutdown,
}

/// Status updates sent from the worker thread back to the GUI.
#[derive(Debug)]
pub enum WorkerStatus {
    /// Model loading has started.
    ModelLoading,
    /// Model loaded successfully.
    ModelLoaded,
    /// Model loading failed with the given error message.
    ModelLoadError(String),
    /// Audio is being converted/decoded.
    Converting,
    /// Transcription progress update (0.0 to 1.0).
    TranscriptionProgress(f32),
    /// Transcription completed successfully.
    TranscriptionComplete(TranscriptionResult),
    /// Transcription was cancelled by the user.
    TranscriptionCancelled,
    /// Transcription failed with the given error message.
    TranscriptionError(String),
}

/// Spawn a worker thread that processes commands and sends status updates.
///
/// Returns the thread's `JoinHandle` and a shared `Arc<AtomicBool>` abort flag
/// that can be used to signal cancellation.
pub fn spawn_worker(
    command_rx: Receiver<Command>,
    status_tx: Sender<WorkerStatus>,
) -> (JoinHandle<()>, Arc<AtomicBool>) {
    let abort_flag = Arc::new(AtomicBool::new(false));
    let abort_flag_clone = Arc::clone(&abort_flag);

    let handle = thread::spawn(move || {
        worker_loop(command_rx, status_tx, abort_flag_clone);
    });

    (handle, abort_flag)
}

/// The main worker loop. Receives commands and processes them sequentially.
fn worker_loop(
    command_rx: Receiver<Command>,
    status_tx: Sender<WorkerStatus>,
    abort_flag: Arc<AtomicBool>,
) {
    let mut engine: Option<TranscriptionEngine> = None;

    loop {
        let command = match command_rx.recv() {
            Ok(cmd) => cmd,
            Err(_) => {
                // Channel closed, exit the loop
                debug!("Command channel closed, worker shutting down");
                break;
            }
        };

        match command {
            Command::LoadModel(path) => {
                handle_load_model(&path, &status_tx, &mut engine);
            }
            Command::Transcribe(path) => {
                handle_transcribe(&path, &status_tx, &mut engine, &abort_flag);
            }
            Command::Cancel => {
                handle_cancel(&abort_flag, &status_tx);
            }
            Command::Shutdown => {
                info!("Worker received shutdown command");
                break;
            }
        }
    }

    debug!("Worker thread exiting");
}

/// Handle the LoadModel command: load the Parakeet TDT model and report status.
fn handle_load_model(
    path: &PathBuf,
    status_tx: &Sender<WorkerStatus>,
    engine: &mut Option<TranscriptionEngine>,
) {
    info!("Loading model from: {:?}", path);
    let _ = status_tx.send(WorkerStatus::ModelLoading);

    match TranscriptionEngine::load(path) {
        Ok(eng) => {
            *engine = Some(eng);
            info!("Model loaded successfully");
            let _ = status_tx.send(WorkerStatus::ModelLoaded);
        }
        Err(e) => {
            error!("Failed to load model: {}", e);
            *engine = None;
            let _ = status_tx.send(WorkerStatus::ModelLoadError(e.to_string()));
        }
    }
}

/// Handle the Transcribe command: validate, convert, transcribe, and report.
fn handle_transcribe(
    path: &PathBuf,
    status_tx: &Sender<WorkerStatus>,
    engine: &mut Option<TranscriptionEngine>,
    abort_flag: &Arc<AtomicBool>,
) {
    // Reset the abort flag before starting a new transcription
    abort_flag.store(false, Ordering::Relaxed);

    let engine = match engine.as_mut() {
        Some(eng) => eng,
        None => {
            let _ = status_tx.send(WorkerStatus::TranscriptionError(
                "No model loaded. Please load a model first.".to_string(),
            ));
            return;
        }
    };

    info!("Starting transcription for: {:?}", path);

    // Step 1: Validate the audio file
    if let Err(e) = validate_audio_file(path) {
        error!("Validation failed for {:?}: {}", path, e);
        let _ = status_tx.send(WorkerStatus::TranscriptionError(format!(
            "Validation failed: {}",
            e
        )));
        return;
    }

    // Step 2: Convert to WAV (16kHz mono) if needed
    let _ = status_tx.send(WorkerStatus::Converting);
    let converted = match convert_to_wav(path) {
        Ok(c) => c,
        Err(e) => {
            error!("Conversion failed for {:?}: {}", path, e);
            let _ = status_tx.send(WorkerStatus::TranscriptionError(format!(
                "Audio conversion failed: {}",
                e
            )));
            return;
        }
    };

    // Step 3: Load WAV samples
    let samples = match load_wav_samples(&converted.wav_path) {
        Ok(s) => s,
        Err(e) => {
            cleanup_temp_file(&converted);
            let _ = status_tx.send(WorkerStatus::TranscriptionError(format!(
                "Failed to read WAV samples: {}",
                e
            )));
            return;
        }
    };

    info!(
        "Audio loaded: {} samples ({:.1} seconds at 16kHz)",
        samples.len(),
        samples.len() as f64 / 16000.0
    );

    // Step 4: Run transcription with progress reporting
    let status_tx_clone = status_tx.clone();
    let progress_cb = move |progress: f32| {
        let _ = status_tx_clone.send(WorkerStatus::TranscriptionProgress(progress));
    };

    let result = engine.transcribe(&samples, &abort_flag, progress_cb);

    // Step 5: Cleanup temporary files
    cleanup_temp_file(&converted);

    // Step 6: Report result
    match result {
        Ok(text) => {
            info!("Transcription completed for: {:?}", path);
            let transcription_result = TranscriptionResult {
                text,
                source_file: path.clone(),
                timestamp: chrono::Utc::now(),
            };
            let _ = status_tx.send(WorkerStatus::TranscriptionComplete(transcription_result));
        }
        Err(EngineError::Cancelled) => {
            info!("Transcription cancelled for: {:?}", path);
            let _ = status_tx.send(WorkerStatus::TranscriptionCancelled);
        }
        Err(e) => {
            error!("Transcription failed for {:?}: {}", path, e);
            let _ = status_tx.send(WorkerStatus::TranscriptionError(format!(
                "Transcription failed: {}",
                e
            )));
        }
    }
}

/// Handle the Cancel command: set the abort flag.
fn handle_cancel(abort_flag: &Arc<AtomicBool>, status_tx: &Sender<WorkerStatus>) {
    info!("Cancel requested");
    abort_flag.store(true, Ordering::Relaxed);
    // Note: The actual TranscriptionCancelled status will be sent when the
    // transcription detects the abort flag and returns EngineError::Cancelled.
    // If no transcription is in progress, we send the cancellation status directly.
    // In practice, the transcription loop checks the flag and will report cancellation.
    let _ = status_tx.send(WorkerStatus::TranscriptionCancelled);
}

/// Load f32 samples from a WAV file (expected: 16kHz mono).
fn load_wav_samples(path: &std::path::Path) -> Result<Vec<f32>, String> {
    let reader = WavReader::open(path)
        .map_err(|e| format!("Cannot open WAV file {:?}: {}", path, e))?;

    let spec = reader.spec();
    debug!(
        "WAV spec: channels={}, sample_rate={}, bits_per_sample={}, format={:?}",
        spec.channels, spec.sample_rate, spec.bits_per_sample, spec.sample_format
    );

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader
                .into_samples::<f32>()
                .collect::<Result<Vec<f32>, _>>()
                .map_err(|e| format!("Failed to read float samples: {}", e))?
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .collect::<Result<Vec<i32>, _>>()
                .map_err(|e| format!("Failed to read int samples: {}", e))?
                .into_iter()
                .map(|s| s as f32 / max_val)
                .collect()
        }
    };

    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn test_shutdown_terminates_worker() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (status_tx, _status_rx) = mpsc::channel();

        let (handle, _abort_flag) = spawn_worker(cmd_rx, status_tx);

        // Send shutdown command
        cmd_tx.send(Command::Shutdown).unwrap();

        // Worker should terminate within a reasonable time
        let result = handle.join();
        assert!(result.is_ok(), "Worker thread should terminate cleanly on Shutdown");
    }

    #[test]
    fn test_cancel_sets_abort_flag() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();

        let (handle, abort_flag) = spawn_worker(cmd_rx, status_tx);

        // Initially, abort flag should be false
        assert!(!abort_flag.load(Ordering::Relaxed));

        // Send cancel command
        cmd_tx.send(Command::Cancel).unwrap();

        // Give the worker a moment to process the command
        thread::sleep(Duration::from_millis(50));

        // Abort flag should now be true
        assert!(
            abort_flag.load(Ordering::Relaxed),
            "Cancel command should set the abort flag to true"
        );

        // Should receive a TranscriptionCancelled status
        let status = status_rx.recv_timeout(Duration::from_millis(100));
        assert!(status.is_ok());
        match status.unwrap() {
            WorkerStatus::TranscriptionCancelled => {} // expected
            other => panic!("Expected TranscriptionCancelled, got: {:?}", other),
        }

        // Clean up: shut down the worker
        cmd_tx.send(Command::Shutdown).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn test_channel_close_terminates_worker() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (status_tx, _status_rx) = mpsc::channel();

        let (handle, _abort_flag) = spawn_worker(cmd_rx, status_tx);

        // Drop the sender to close the channel
        drop(cmd_tx);

        // Worker should terminate when channel is closed
        let result = handle.join();
        assert!(result.is_ok(), "Worker thread should terminate when channel closes");
    }

    #[test]
    fn test_transcribe_without_model_sends_error() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();

        let (handle, _abort_flag) = spawn_worker(cmd_rx, status_tx);

        // Send transcribe without loading a model first
        cmd_tx
            .send(Command::Transcribe(PathBuf::from("/some/file.wav")))
            .unwrap();

        // Should receive an error about no model loaded
        let status = status_rx.recv_timeout(Duration::from_millis(200));
        assert!(status.is_ok());
        match status.unwrap() {
            WorkerStatus::TranscriptionError(msg) => {
                assert!(
                    msg.contains("No model loaded"),
                    "Error should mention no model loaded, got: {}",
                    msg
                );
            }
            other => panic!("Expected TranscriptionError, got: {:?}", other),
        }

        // Clean up
        cmd_tx.send(Command::Shutdown).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn test_load_model_nonexistent_sends_error() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (status_tx, status_rx) = mpsc::channel();

        let (handle, _abort_flag) = spawn_worker(cmd_rx, status_tx);

        // Send load model with a nonexistent path
        cmd_tx
            .send(Command::LoadModel(PathBuf::from(
                "/nonexistent/model.bin",
            )))
            .unwrap();

        // Should receive ModelLoading first
        let status1 = status_rx.recv_timeout(Duration::from_millis(200));
        assert!(status1.is_ok());
        match status1.unwrap() {
            WorkerStatus::ModelLoading => {} // expected
            other => panic!("Expected ModelLoading, got: {:?}", other),
        }

        // Then ModelLoadError
        let status2 = status_rx.recv_timeout(Duration::from_millis(200));
        assert!(status2.is_ok());
        match status2.unwrap() {
            WorkerStatus::ModelLoadError(msg) => {
                assert!(!msg.is_empty(), "Error message should not be empty");
            }
            other => panic!("Expected ModelLoadError, got: {:?}", other),
        }

        // Clean up
        cmd_tx.send(Command::Shutdown).unwrap();
        handle.join().unwrap();
    }
}
