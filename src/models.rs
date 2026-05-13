use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// The primary output of a transcription operation.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionResult {
    /// The transcribed text content
    pub text: String,
    /// Path to the original source audio file
    pub source_file: PathBuf,
    /// Timestamp when transcription completed
    pub timestamp: DateTime<Utc>,
}

/// Metadata extracted from a Markdown export header.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionMetadata {
    /// Source filename from the header
    pub source_filename: String,
    /// Timestamp from the header
    pub timestamp: DateTime<Utc>,
}

/// Represents a file in the transcription queue.
#[derive(Debug, Clone)]
pub struct QueuedFile {
    pub path: PathBuf,
    pub status: QueueStatus,
}

/// Status of a file in the transcription queue.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}
