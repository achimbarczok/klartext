use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors arising from audio file validation.
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Unsupported file extension '{extension}' for '{path}'")]
    UnsupportedExtension { path: PathBuf, extension: String },

    #[error("File not readable '{path}': {source}")]
    FileNotReadable { path: PathBuf, source: io::Error },

    #[error("Invalid audio header in '{path}': {detail}")]
    InvalidAudioHeader { path: PathBuf, detail: String },
}

/// Errors arising from audio format conversion.
#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("Decoding failed for '{path}': {detail}")]
    DecodingFailed { path: PathBuf, detail: String },

    #[error("Resampling failed: {detail}")]
    ResamplingFailed { detail: String },

    #[error("Write failed for '{path}': {source}")]
    WriteFailed { path: PathBuf, source: io::Error },
}

/// Errors arising from the transcription engine.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Model loading failed from '{path}': {detail}")]
    ModelLoadFailed { path: PathBuf, detail: String },

    #[error("Transcription failed: {detail}")]
    TranscriptionFailed { detail: String },

    #[error("Transcription cancelled by user")]
    Cancelled,
}

/// Errors arising from file export operations.
#[derive(Error, Debug)]
pub enum ExportError {
    #[error("Export failed to '{path}': {source}")]
    IoError { path: PathBuf, source: io::Error },
}

/// Top-level application error type.
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Validation failed for '{path}': {detail}")]
    Validation { path: PathBuf, detail: String },

    #[error("Audio conversion failed for '{path}': {detail}")]
    Conversion { path: PathBuf, detail: String },

    #[error("Model loading failed from '{path}': {detail}")]
    ModelLoad { path: PathBuf, detail: String },

    #[error("Transcription failed: {detail}")]
    Transcription { detail: String },

    #[error("Export failed to '{path}': {source}")]
    Export { path: PathBuf, source: io::Error },

    #[error("Transcription cancelled by user")]
    Cancelled,
}
