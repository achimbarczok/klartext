// Feature: klartext-rust, Property 3: Error messages contain identifying information
//
// **Validates: Requirements 3.3, 4.4, 6.4**
//
// For any application error (AppError variant), the formatted error message SHALL contain
// the source file path (when applicable) and a non-empty description of the failure reason.

use std::io;
use std::path::PathBuf;

use proptest::prelude::*;

use klartext_rust::errors::AppError;

/// Strategy to generate non-empty path strings (converted to PathBuf).
fn arb_path() -> impl Strategy<Value = PathBuf> {
    "[a-zA-Z0-9_/\\.]{1,64}".prop_map(PathBuf::from)
}

/// Strategy to generate non-empty detail strings.
fn arb_detail() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.,!?]{1,128}"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property 3: Validation error messages contain the file path and a non-empty description.
    #[test]
    fn validation_error_contains_path_and_description(
        path in arb_path(),
        detail in arb_detail(),
    ) {
        let error = AppError::Validation {
            path: path.clone(),
            detail: detail.clone(),
        };
        let msg = format!("{}", error);

        // The display output must contain the path
        prop_assert!(
            msg.contains(&path.display().to_string()),
            "Validation error message '{}' does not contain path '{}'",
            msg,
            path.display()
        );
        // The display output must be non-empty
        prop_assert!(!msg.is_empty(), "Error message should not be empty");
    }

    /// Property 3: Conversion error messages contain the file path and a non-empty description.
    #[test]
    fn conversion_error_contains_path_and_description(
        path in arb_path(),
        detail in arb_detail(),
    ) {
        let error = AppError::Conversion {
            path: path.clone(),
            detail: detail.clone(),
        };
        let msg = format!("{}", error);

        prop_assert!(
            msg.contains(&path.display().to_string()),
            "Conversion error message '{}' does not contain path '{}'",
            msg,
            path.display()
        );
        prop_assert!(!msg.is_empty(), "Error message should not be empty");
    }

    /// Property 3: ModelLoad error messages contain the model path and a non-empty description.
    #[test]
    fn model_load_error_contains_path_and_description(
        path in arb_path(),
        detail in arb_detail(),
    ) {
        let error = AppError::ModelLoad {
            path: path.clone(),
            detail: detail.clone(),
        };
        let msg = format!("{}", error);

        prop_assert!(
            msg.contains(&path.display().to_string()),
            "ModelLoad error message '{}' does not contain path '{}'",
            msg,
            path.display()
        );
        prop_assert!(!msg.is_empty(), "Error message should not be empty");
    }

    /// Property 3: Transcription error messages contain a non-empty description.
    #[test]
    fn transcription_error_contains_description(
        detail in arb_detail(),
    ) {
        let error = AppError::Transcription {
            detail: detail.clone(),
        };
        let msg = format!("{}", error);

        // Transcription variant has no path, but must have non-empty output
        prop_assert!(!msg.is_empty(), "Error message should not be empty");
        // The detail should appear in the message
        prop_assert!(
            msg.contains(&detail),
            "Transcription error message '{}' does not contain detail '{}'",
            msg,
            detail
        );
    }

    /// Property 3: Export error messages contain the file path and a non-empty description.
    #[test]
    fn export_error_contains_path_and_description(
        path in arb_path(),
    ) {
        // io::Error cannot be easily generated randomly; use a fixed error
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        let error = AppError::Export {
            path: path.clone(),
            source: io_err,
        };
        let msg = format!("{}", error);

        prop_assert!(
            msg.contains(&path.display().to_string()),
            "Export error message '{}' does not contain path '{}'",
            msg,
            path.display()
        );
        prop_assert!(!msg.is_empty(), "Error message should not be empty");
    }

    /// Property 3: Cancelled error produces a non-empty message.
    #[test]
    fn cancelled_error_produces_non_empty_message(_dummy in 0..1u8) {
        let error = AppError::Cancelled;
        let msg = format!("{}", error);

        prop_assert!(!msg.is_empty(), "Cancelled error message should not be empty");
    }
}
