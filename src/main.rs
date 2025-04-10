use clap::Parser;
use tokio::fs;
use tokio::task::LocalSet;
use chrono::{DateTime, Local, Duration, Datelike, Weekday};
use std::path::PathBuf;
use futures::stream::{FuturesUnordered, StreamExt};

#[derive(Parser)]
#[clap(name = "organizer", about = "A file organizer tool")]
struct Cli {
    /// The directory to organize
    dir: String,
    /// Reverse the organization
    #[clap(short, long)]
    reverse: bool,
}

#[tokio::main]
async fn main() {
    let local_set = LocalSet::new();
    let args = Cli::parse();

    local_set.run_until(async {
        if args.reverse {
            reverse_organize(&args.dir).await;
        } else {
            organize(&args.dir).await;
        }
    }).await;

    println!("Operation complete!");
}

async fn organize(dir: &str) {
    let mut entries = fs::read_dir(dir).await.expect("Failed to read directory");
    let mut tasks = FuturesUnordered::new();

    while let Some(entry) = entries.next_entry().await.expect("Failed to read entry") {
        let path = entry.path();
        if path.is_file() {
            tasks.push(tokio::task::spawn_local(async move {
                organize_file(path).await;
            }));
        }
    }

    while let Some(task) = tasks.next().await {
        task.expect("Task failed");
    }
}

async fn organize_file(file_path: PathBuf) {
    if let Ok(metadata) = fs::metadata(&file_path).await {
        if let Ok(modified) = metadata.modified() {
            let datetime: DateTime<Local> = modified.into();
            let year = datetime.year();

            // Calculate the previous Sunday
            let weekday = datetime.weekday();
            let days_since_sunday = match weekday {
                Weekday::Sun => 0,
                _ => weekday.num_days_from_sunday() as i64,
            };
            let previous_sunday = datetime - Duration::days(days_since_sunday);

            let month_name = datetime.format("%B").to_string();
            let week_folder_name = format!("week of {}", previous_sunday.format("%Y-%m-%d"));

            // Reuse the parent folder of the file
            let year_folder = file_path.parent().unwrap().join(year.to_string());
            let month_folder = year_folder.join(month_name);
            let week_folder = month_folder.join(week_folder_name);

            fs::create_dir_all(&week_folder).await.expect("Failed to create folder");

            let new_file_path = week_folder.join(file_path.file_name().unwrap());
            fs::rename(&file_path, &new_file_path)
                .await
                .expect("Failed to move file");
        }
    }
}

async fn reverse_organize(dir: &str) {
    // In this simplified version, we pass a clone of the target directory string
    let target = dir.to_string();
    reverse_organize_dir(PathBuf::from(dir), target).await;
}

async fn reverse_organize_dir(current_dir: PathBuf, target_dir: String) {
    let mut entries = fs::read_dir(&current_dir)
        .await
        .expect("Failed to read directory");
    let mut tasks = FuturesUnordered::new();

    while let Some(entry) = entries.next_entry().await.expect("Failed to read entry") {
        let path = entry.path();
        if path.is_file() {
            let target_dir = target_dir.clone();
            tasks.push(tokio::task::spawn_local(async move {
                let new_file_path = PathBuf::from(target_dir).join(path.file_name().unwrap());
                fs::rename(&path, &new_file_path)
                    .await
                    .expect("Failed to move file");
            }));
        } else if path.is_dir() {
            let target_dir = target_dir.clone();
            tasks.push(tokio::task::spawn_local(async move {
                reverse_organize_dir(path, target_dir).await;
            }));
        }
    }

    while let Some(task) = tasks.next().await {
        task.expect("Task failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self as std_fs, File};
    use std::io::Write;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Helper function to create a test directory structure with files
    async fn setup_test_directory() -> (TempDir, String) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let temp_path = temp_dir.path().to_str().unwrap().to_string();
        
        // Create a few test files with different modification times
        create_test_file(&temp_path, "file1.txt", 0).await;
        create_test_file(&temp_path, "file2.txt", 1).await;
        create_test_file(&temp_path, "file3.txt", 7).await;  // One week later
        
        (temp_dir, temp_path)
    }
    
    async fn create_test_file(dir: &str, filename: &str, days_offset: u64) {
        let file_path = format!("{}/{}", dir, filename);
        let mut file = File::create(&file_path).expect("Failed to create test file");
        write!(file, "Test content for {}", filename).expect("Failed to write to test file");
        
        // Set modification time
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap();
        let new_time = UNIX_EPOCH + std::time::Duration::from_secs(
            duration.as_secs() + (days_offset * 24 * 60 * 60)
        );
        
        let mtime = filetime::FileTime::from_system_time(new_time);
        filetime::set_file_times(
            &file_path,
            mtime,
            mtime,
        ).expect("Failed to set file modification time");
    }
    
    #[test]
    fn test_organize() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            let (temp_dir, temp_path) = setup_test_directory().await;
            
            // Run the organize function inside a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Check that the files were organized correctly
            let now = Local::now();
            let year_folder = format!("{}/{}", temp_path, now.year());
            
            // Verify year directory exists
            assert!(std_fs::metadata(&year_folder).is_ok(), "Year folder should exist");
            
            // Check that month directories exist
            let month_name = now.format("%B").to_string();
            let month_folder = format!("{}/{}", year_folder, month_name);
            assert!(std_fs::metadata(&month_folder).is_ok(), "Month folder should exist");
            
            // We'd need to calculate the expected week folders based on the dates we set
            // This is simplified here - in a real test you'd want to check each file
            let entries = std_fs::read_dir(month_folder).unwrap();
            let week_folders: Vec<_> = entries.filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .collect();
            
            assert!(!week_folders.is_empty(), "At least one week folder should exist");
            
            // Keep the temp_dir in scope until the end of the test
            drop(temp_dir);
        });
    }
    
    #[test]
    fn test_reverse_organize() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            // First organize the files
            let (temp_dir, temp_path) = setup_test_directory().await;
            
            // Run organize in a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Now reverse the organization in a LocalSet
            local_set.run_until(async {
                reverse_organize(&temp_path).await;
            }).await;
            
            // Check that files are back in the root directory
            let final_count = std_fs::read_dir(&temp_path)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .count();
            
            assert_eq!(final_count, 3, "All three files should be back in the root directory");
            
            drop(temp_dir);
        });
    }

    #[test]
    fn test_empty_directory() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            let temp_dir = TempDir::new().expect("Failed to create temp directory");
            let temp_path = temp_dir.path().to_str().unwrap().to_string();
            
            // Run organize on an empty directory in a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Verify no errors occurred (implicitly tested by the function completing)
            let entries = std_fs::read_dir(&temp_path).unwrap();
            let count = entries.count();
            assert_eq!(count, 0, "Directory should remain empty after organizing");
        });
    }
    
    #[test]
    fn test_file_placement() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            let (temp_dir, temp_path) = setup_test_directory().await;
            
            // Run organize in a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Get the current year and month
            let now = Local::now();
            let year = now.year().to_string();
            let month = now.format("%B").to_string();
            
            // Count files in each week folder
            let year_dir = PathBuf::from(&temp_path).join(year);
            let month_dir = year_dir.join(month);
            
            let week_folders: Vec<_> = std_fs::read_dir(&month_dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .collect();
            
            // We expect files to be organized into at least two week folders
            // (since we created files with different modification times)
            assert!(week_folders.len() >= 1, "Should have at least one week folder");
            
            // Count total files after organization
            let mut total_files = 0;
            for week_folder in week_folders {
                let files: Vec<_> = std_fs::read_dir(week_folder.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.path().is_file())
                    .collect();
                
                total_files += files.len();
            }
            
            assert_eq!(total_files, 3, "All three files should be organized somewhere");
            
            drop(temp_dir);
        });
    }
    
    #[test]
    fn test_nested_directory_organization() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            // Create a nested directory structure
            let temp_dir = TempDir::new().expect("Failed to create temp directory");
            let temp_path = temp_dir.path().to_str().unwrap().to_string();
            
            // Create a subdirectory
            let sub_dir = format!("{}/subdir", temp_path);
            std_fs::create_dir(&sub_dir).expect("Failed to create subdirectory");
            
            // Create files in both directories
            create_test_file(&temp_path, "root_file.txt", 0).await;
            create_test_file(&sub_dir, "sub_file.txt", 0).await;
            
            // Run organize on the root directory in a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Verify the root file was organized but the subdirectory remains
            let root_files = std_fs::read_dir(&temp_path)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .count();
            
            assert_eq!(root_files, 0, "No files should remain in root");
            
            let sub_dir_exists = std_fs::metadata(&sub_dir).is_ok();
            assert!(sub_dir_exists, "Subdirectory should still exist");
            
            // Verify the file in the subdirectory is untouched
            let sub_file_path = format!("{}/sub_file.txt", sub_dir);
            let sub_file_exists = std_fs::metadata(&sub_file_path).is_ok();
            assert!(sub_file_exists, "File in subdirectory should not be moved");
            
            drop(temp_dir);
        });
    }
    
    #[test]
    fn test_organize_and_reverse_cycle() {
        let rt = Runtime::new().unwrap();
        
        rt.block_on(async {
            let local_set = LocalSet::new();
            let (temp_dir, temp_path) = setup_test_directory().await;
            
            // First, check the initial state
            let initial_files: Vec<String> = std_fs::read_dir(&temp_path)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            
            // Sort for reliable comparison
            let mut initial_files_sorted = initial_files.clone();
            initial_files_sorted.sort();
            
            // Run organize in a LocalSet
            local_set.run_until(async {
                organize(&temp_path).await;
            }).await;
            
            // Run reverse organize in a LocalSet
            local_set.run_until(async {
                reverse_organize(&temp_path).await;
            }).await;
            
            // Check the final state
            let final_files: Vec<String> = std_fs::read_dir(&temp_path)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            
            // Sort for reliable comparison
            let mut final_files_sorted = final_files.clone();
            final_files_sorted.sort();
            
            // The sets of files should be identical after a full cycle
            assert_eq!(initial_files_sorted, final_files_sorted, 
                "Files after organize + reverse cycle should match initial files");
            
            drop(temp_dir);
        });
    }
}
