// Feature: klartext-rust, Property 6: Queue contains only validated files
//
// **Validates: Requirements 1.2, 2.2**
//
// For any list of file paths submitted for transcription (via drop or dialog), the resulting
// queue SHALL contain exactly those files that pass validation, in their original submission
// order, and no others.

use std::path::PathBuf;

use proptest::prelude::*;

use klartext_rust::queue::filter_valid_audio_paths;

/// Valid audio extensions (lowercase forms; the function handles case-insensitivity).
const VALID_EXTENSIONS: &[&str] = &["wav", "mp3"];

/// Invalid extensions for testing.
const INVALID_EXTENSIONS: &[&str] = &[
    "ogg", "flac", "aac", "txt", "pdf", "exe", "m4a", "wma", "doc", "zip", "png", "jpg",
];

/// Strategy to generate a file path with a valid audio extension (various cases).
fn arb_valid_audio_path() -> impl Strategy<Value = PathBuf> {
    let ext_strategy = prop_oneof![
        Just("wav".to_string()),
        Just("WAV".to_string()),
        Just("Wav".to_string()),
        Just("mp3".to_string()),
        Just("MP3".to_string()),
        Just("Mp3".to_string()),
    ];

    ("[a-zA-Z0-9_]{1,20}", ext_strategy).prop_map(|(name, ext)| {
        PathBuf::from(format!("{}.{}", name, ext))
    })
}

/// Strategy to generate a file path with an invalid extension.
fn arb_invalid_audio_path() -> impl Strategy<Value = PathBuf> {
    let ext_strategy = prop_oneof![
        // Known invalid extensions
        prop::sample::select(INVALID_EXTENSIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        // Random extensions that aren't wav or mp3
        "[a-z]{1,8}".prop_filter("must not be wav or mp3", |ext| {
            !ext.eq_ignore_ascii_case("wav") && !ext.eq_ignore_ascii_case("mp3")
        }),
    ];

    prop_oneof![
        // File with invalid extension
        ("[a-zA-Z0-9_]{1,20}", ext_strategy).prop_map(|(name, ext)| {
            PathBuf::from(format!("{}.{}", name, ext))
        }),
        // File with no extension
        "[a-zA-Z0-9_]{1,20}".prop_map(|name| PathBuf::from(name)),
    ]
}

/// Strategy to generate a mixed list of valid and invalid file paths.
/// Each element is tagged with whether it's expected to be valid.
fn arb_mixed_file_list() -> impl Strategy<Value = Vec<(PathBuf, bool)>> {
    proptest::collection::vec(
        prop_oneof![
            arb_valid_audio_path().prop_map(|p| (p, true)),
            arb_invalid_audio_path().prop_map(|p| (p, false)),
        ],
        0..=30,
    )
}

/// Helper: check if a path has a valid audio extension.
fn is_valid_extension(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VALID_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Property 6: The queue filter returns exactly the valid files in original order.
    /// Given a random list of file paths with mixed valid/invalid extensions,
    /// filter_valid_audio_paths returns only those with valid extensions, preserving order.
    #[test]
    fn queue_contains_only_validated_files(file_list in arb_mixed_file_list()) {
        let paths: Vec<PathBuf> = file_list.iter().map(|(p, _)| p.clone()).collect();
        let expected_valid: Vec<PathBuf> = file_list.iter()
            .filter(|(_, is_valid)| *is_valid)
            .map(|(p, _)| p.clone())
            .collect();
        let expected_rejected_count = file_list.iter()
            .filter(|(_, is_valid)| !*is_valid)
            .count();

        let (valid, rejected) = filter_valid_audio_paths(&paths);

        // Only valid files pass through
        prop_assert_eq!(
            valid.len(),
            expected_valid.len(),
            "Expected {} valid files, got {}. Valid: {:?}, Expected: {:?}",
            expected_valid.len(),
            valid.len(),
            valid,
            expected_valid
        );

        // All rejected files are accounted for
        prop_assert_eq!(
            rejected.len(),
            expected_rejected_count,
            "Expected {} rejected files, got {}",
            expected_rejected_count,
            rejected.len()
        );

        // Order is preserved: valid files appear in the same relative order as input
        for (actual, expected) in valid.iter().zip(expected_valid.iter()) {
            prop_assert_eq!(actual, expected, "Order mismatch: got {:?}, expected {:?}", actual, expected);
        }

        // Every valid file has a valid extension
        for path in &valid {
            prop_assert!(
                is_valid_extension(path),
                "File {:?} in valid list does not have a valid extension",
                path
            );
        }

        // Every rejected file does NOT have a valid extension
        for (path, reason) in &rejected {
            prop_assert!(
                !is_valid_extension(path),
                "File {:?} in rejected list has a valid extension but was rejected with: {}",
                path,
                reason
            );
        }
    }

    /// Property 6 (sub-property): Empty input produces empty output.
    #[test]
    fn empty_input_produces_empty_output(_dummy in 0..1u8) {
        let (valid, rejected) = filter_valid_audio_paths(&[]);
        prop_assert!(valid.is_empty(), "Expected empty valid list for empty input");
        prop_assert!(rejected.is_empty(), "Expected empty rejected list for empty input");
    }

    /// Property 6 (sub-property): All-valid input passes everything through in order.
    #[test]
    fn all_valid_files_pass_through(paths in proptest::collection::vec(arb_valid_audio_path(), 1..=20)) {
        let (valid, rejected) = filter_valid_audio_paths(&paths);

        prop_assert_eq!(valid.len(), paths.len(), "All valid files should pass through");
        prop_assert_eq!(rejected.len(), 0, "No files should be rejected");

        // Order preserved
        for (actual, expected) in valid.iter().zip(paths.iter()) {
            prop_assert_eq!(actual, expected);
        }
    }

    /// Property 6 (sub-property): All-invalid input rejects everything.
    #[test]
    fn all_invalid_files_rejected(paths in proptest::collection::vec(arb_invalid_audio_path(), 1..=20)) {
        let (valid, rejected) = filter_valid_audio_paths(&paths);

        prop_assert_eq!(valid.len(), 0, "No invalid files should pass through");
        prop_assert_eq!(rejected.len(), paths.len(), "All files should be rejected");

        // Each rejected entry has a non-empty reason
        for (path, reason) in &rejected {
            prop_assert!(!reason.is_empty(), "Rejection reason for {:?} should not be empty", path);
        }
    }
}
