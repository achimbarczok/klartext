use std::path::{Path, PathBuf};

use crate::core::validator::validate_audio_file;
use crate::models::{QueueStatus, QueuedFile};

/// Valid audio file extensions (lowercase).
const VALID_EXTENSIONS: &[&str] = &["wav", "mp3"];

/// Result of submitting files to the queue.
#[derive(Debug, Clone)]
pub struct QueueResult {
    /// Files that passed validation and were added to the queue.
    pub queued: Vec<QueuedFile>,
    /// Files that failed validation, with the rejection reason.
    pub rejected: Vec<(PathBuf, String)>,
}

/// Manages the transcription file queue.
#[derive(Debug, Clone)]
pub struct FileQueue {
    files: Vec<QueuedFile>,
}

impl FileQueue {
    /// Create a new empty file queue.
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Submit files for transcription. Validates each file and adds valid ones to the queue.
    /// Returns information about which files were queued and which were rejected.
    pub fn submit_files(&mut self, paths: &[PathBuf]) -> QueueResult {
        let mut queued = Vec::new();
        let mut rejected = Vec::new();

        for path in paths {
            match validate_audio_file(path) {
                Ok(_format) => {
                    let file = QueuedFile {
                        path: path.clone(),
                        status: QueueStatus::Pending,
                    };
                    self.files.push(file.clone());
                    queued.push(file);
                }
                Err(e) => {
                    rejected.push((path.clone(), e.to_string()));
                }
            }
        }

        QueueResult { queued, rejected }
    }

    /// Get all files currently in the queue.
    pub fn files(&self) -> &[QueuedFile] {
        &self.files
    }

    /// Get the number of files in the queue.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Update the status of a file in the queue by path.
    pub fn update_status(&mut self, path: &Path, status: QueueStatus) {
        if let Some(file) = self.files.iter_mut().find(|f| f.path == path) {
            file.status = status;
        }
    }

    /// Get the next pending file in the queue.
    pub fn next_pending(&self) -> Option<&QueuedFile> {
        self.files.iter().find(|f| f.status == QueueStatus::Pending)
    }

    /// Clear all files from the queue.
    pub fn clear(&mut self) {
        self.files.clear();
    }
}

impl Default for FileQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a file extension is a valid audio extension.
///
/// Returns true if the extension (case-insensitive) is one of the supported formats.
pub fn has_valid_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VALID_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Filter a list of paths, keeping only those with valid audio extensions.
///
/// This is the pure logic portion of queue filtering (extension check only).
/// It does not perform file I/O (no readability or header checks).
/// Returns (valid_paths_in_order, rejected_with_reasons).
pub fn filter_valid_audio_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let mut valid = Vec::new();
    let mut rejected = Vec::new();

    for path in paths {
        if has_valid_audio_extension(path) {
            valid.push(path.clone());
        } else {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_string();
            let reason = if extension.is_empty() {
                "No file extension provided. Supported formats: .wav, .mp3".to_string()
            } else {
                format!(
                    "Unsupported file extension '.{}'. Supported formats: .wav, .mp3",
                    extension
                )
            };
            rejected.push((path.clone(), reason));
        }
    }

    (valid, rejected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_valid_wav() -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".wav").unwrap();
        file.write_all(b"RIFF\x00\x00\x00\x00WAVEfmt ").unwrap();
        file
    }

    fn create_valid_mp3() -> NamedTempFile {
        let mut file = NamedTempFile::with_suffix(".mp3").unwrap();
        file.write_all(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        file
    }

    #[test]
    fn test_submit_valid_files() {
        let wav = create_valid_wav();
        let mp3 = create_valid_mp3();

        let mut queue = FileQueue::new();
        let result = queue.submit_files(&[wav.path().to_path_buf(), mp3.path().to_path_buf()]);

        assert_eq!(result.queued.len(), 2);
        assert_eq!(result.rejected.len(), 0);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_submit_invalid_extension() {
        let file = NamedTempFile::with_suffix(".ogg").unwrap();

        let mut queue = FileQueue::new();
        let result = queue.submit_files(&[file.path().to_path_buf()]);

        assert_eq!(result.queued.len(), 0);
        assert_eq!(result.rejected.len(), 1);
        assert!(result.rejected[0].1.contains("Unsupported"));
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_submit_mixed_files_preserves_order() {
        let wav = create_valid_wav();
        let ogg = NamedTempFile::with_suffix(".ogg").unwrap();
        let mp3 = create_valid_mp3();

        let mut queue = FileQueue::new();
        let result = queue.submit_files(&[
            wav.path().to_path_buf(),
            ogg.path().to_path_buf(),
            mp3.path().to_path_buf(),
        ]);

        assert_eq!(result.queued.len(), 2);
        assert_eq!(result.rejected.len(), 1);
        // Order preserved
        assert_eq!(result.queued[0].path, wav.path());
        assert_eq!(result.queued[1].path, mp3.path());
    }

    #[test]
    fn test_queue_status_tracking() {
        let wav = create_valid_wav();

        let mut queue = FileQueue::new();
        queue.submit_files(&[wav.path().to_path_buf()]);

        assert_eq!(queue.files()[0].status, QueueStatus::Pending);

        queue.update_status(wav.path(), QueueStatus::InProgress);
        assert_eq!(queue.files()[0].status, QueueStatus::InProgress);

        queue.update_status(wav.path(), QueueStatus::Completed);
        assert_eq!(queue.files()[0].status, QueueStatus::Completed);
    }

    #[test]
    fn test_next_pending() {
        let wav1 = create_valid_wav();
        let wav2 = create_valid_wav();

        let mut queue = FileQueue::new();
        queue.submit_files(&[wav1.path().to_path_buf(), wav2.path().to_path_buf()]);

        let next = queue.next_pending().unwrap();
        assert_eq!(next.path, wav1.path());

        queue.update_status(wav1.path(), QueueStatus::Completed);
        let next = queue.next_pending().unwrap();
        assert_eq!(next.path, wav2.path());
    }

    #[test]
    fn test_filter_valid_audio_paths_basic() {
        let paths = vec![
            PathBuf::from("song.wav"),
            PathBuf::from("podcast.mp3"),
            PathBuf::from("video.mp4"),
            PathBuf::from("document.txt"),
        ];

        let (valid, rejected) = filter_valid_audio_paths(&paths);

        assert_eq!(valid.len(), 2);
        assert_eq!(valid[0], PathBuf::from("song.wav"));
        assert_eq!(valid[1], PathBuf::from("podcast.mp3"));
        assert_eq!(rejected.len(), 2);
    }

    #[test]
    fn test_filter_valid_audio_paths_case_insensitive() {
        let paths = vec![
            PathBuf::from("file.WAV"),
            PathBuf::from("file.Mp3"),
            PathBuf::from("file.WaV"),
        ];

        let (valid, rejected) = filter_valid_audio_paths(&paths);

        assert_eq!(valid.len(), 3);
        assert_eq!(rejected.len(), 0);
    }

    #[test]
    fn test_filter_valid_audio_paths_no_extension() {
        let paths = vec![PathBuf::from("noextension")];

        let (valid, rejected) = filter_valid_audio_paths(&paths);

        assert_eq!(valid.len(), 0);
        assert_eq!(rejected.len(), 1);
        assert!(rejected[0].1.contains("No file extension"));
    }

    #[test]
    fn test_filter_preserves_order() {
        let paths = vec![
            PathBuf::from("third.mp3"),
            PathBuf::from("invalid.ogg"),
            PathBuf::from("first.wav"),
            PathBuf::from("second.mp3"),
        ];

        let (valid, _rejected) = filter_valid_audio_paths(&paths);

        assert_eq!(valid.len(), 3);
        assert_eq!(valid[0], PathBuf::from("third.mp3"));
        assert_eq!(valid[1], PathBuf::from("first.wav"));
        assert_eq!(valid[2], PathBuf::from("second.mp3"));
    }

    #[test]
    fn test_nonexistent_file_rejected_by_submit() {
        let mut queue = FileQueue::new();
        let result = queue.submit_files(&[PathBuf::from("/nonexistent/file.wav")]);

        assert_eq!(result.queued.len(), 0);
        assert_eq!(result.rejected.len(), 1);
        // Rejected because file is not readable
        assert!(result.rejected[0].1.contains("not readable") || result.rejected[0].1.contains("File not readable"));
    }
}
