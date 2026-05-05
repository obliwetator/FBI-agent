use crate::events::voice_receiver::{CLIPS_FILE_PATH, RECORDING_FILE_PATH};
use std::path::Path;

#[tokio::test]
async fn test_audio_paths_are_valid() -> Result<(), Box<dyn std::error::Error>> {
    let rec_path = Path::new(RECORDING_FILE_PATH);
    let clips_path = Path::new(CLIPS_FILE_PATH);

    // Ensure directories can be created
    std::fs::create_dir_all(rec_path)?;
    std::fs::create_dir_all(clips_path)?;

    assert!(
        rec_path.exists(),
        "Recording path must exist after creation"
    );
    assert!(clips_path.exists(), "Clips path must exist after creation");

    // Test writing and reading to ensure we have valid paths
    let test_rec_file = rec_path.join("test_write.txt");
    std::fs::write(&test_rec_file, "test_data")?;
    let read_back = std::fs::read_to_string(&test_rec_file)?;
    assert_eq!(read_back, "test_data");

    let test_clip_file = clips_path.join("test_clip.txt");
    std::fs::write(&test_clip_file, "test_clip_data")?;
    let read_clip_back = std::fs::read_to_string(&test_clip_file)?;
    assert_eq!(read_clip_back, "test_clip_data");

    // Clean up
    std::fs::remove_file(test_rec_file)?;
    std::fs::remove_file(test_clip_file)?;
    Ok(())
}
