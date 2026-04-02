use crate::events::voice_receiver::{CLIPS_FILE_PATH, RECORDING_FILE_PATH};
use std::path::Path;

#[tokio::test]
async fn test_audio_paths_are_valid() {
    let rec_path = Path::new(RECORDING_FILE_PATH);
    let clips_path = Path::new(CLIPS_FILE_PATH);

    // Ensure directories can be created
    std::fs::create_dir_all(rec_path).expect("Should be able to create recording dir");
    std::fs::create_dir_all(clips_path).expect("Should be able to create clips dir");

    assert!(
        rec_path.exists(),
        "Recording path must exist after creation"
    );
    assert!(clips_path.exists(), "Clips path must exist after creation");

    // Test writing and reading to ensure we have valid paths
    let test_rec_file = rec_path.join("test_write.txt");
    std::fs::write(&test_rec_file, "test_data").expect("Should be able to write to rec path");
    let read_back = std::fs::read_to_string(&test_rec_file).expect("Should be able to read back");
    assert_eq!(read_back, "test_data");

    let test_clip_file = clips_path.join("test_clip.txt");
    std::fs::write(&test_clip_file, "test_clip_data")
        .expect("Should be able to write to clips path");
    let read_clip_back = std::fs::read_to_string(&test_clip_file).expect("Should read clip back");
    assert_eq!(read_clip_back, "test_clip_data");

    // Clean up
    std::fs::remove_file(test_rec_file).unwrap();
    std::fs::remove_file(test_clip_file).unwrap();
}
