use clap::Parser;
use tokio::fs;
use tokio::task::LocalSet;
use tokio::sync::Mutex;
use std::path::PathBuf;
use chrono::{Datelike, NaiveDateTime, Local, Duration};
use std::sync::Arc;

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

    let mut tasks = Vec::new();

    while let Some(entry) = entries.next_entry().await.expect("Failed to read entry") {
        let path = entry.path();
        if path.is_file() {
            let task = tokio::task::spawn_local(async move {
                organize_file(path).await;
            });
            tasks.push(task);
        }
    }

    for task in tasks {
        task.await.expect("Task failed");
    }
}

async fn organize_file(file_path: PathBuf) {
    if let Ok(metadata) = fs::metadata(&file_path).await {
        if let Ok(modified) = metadata.modified() {
            #[allow(deprecated)]
            let datetime = NaiveDateTime::from_timestamp(
                modified.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
                0,
            );
            let year = datetime.year();
            let month = datetime.month();

            // Calculate the previous Sunday
            let weekday = datetime.weekday().num_days_from_sunday();
            let previous_sunday = datetime - Duration::days(weekday.into());

            #[allow(deprecated)]
            let month_name = chrono::TimeZone::ymd(&Local, year, month, 1).format("%B").to_string();
            let week_folder_name = format!("week of {}", previous_sunday.format("%Y-%m-%d"));

            let year_folder = file_path.parent().unwrap().join(format!("{}", year));
            let month_folder = year_folder.join(month_name);
            let week_folder = month_folder.join(week_folder_name);

            fs::create_dir_all(&week_folder).await.expect("Failed to create folder");

            let new_file_path = week_folder.join(file_path.file_name().unwrap());
            fs::rename(&file_path, &new_file_path).await.expect("Failed to move file");
        }
    }
}

async fn reverse_organize(dir: &str) {
    let dir_clone = Arc::new(Mutex::new(dir.to_string()));
    let mut tasks = Vec::new();

    let mut entries = fs::read_dir(dir).await.expect("Failed to read directory");

    while let Some(entry) = entries.next_entry().await.expect("Failed to read entry") {
        let path = entry.path();
        if path.is_dir() {
            let dir_clone = Arc::clone(&dir_clone);
            let task = tokio::task::spawn_local(async move {
                reverse_organize_dir(path, dir_clone).await;
            });
            tasks.push(task);
        }
    }

    for task in tasks {
        task.await.expect("Task failed");
    }
}

async fn reverse_organize_dir(current_dir: PathBuf, target_dir: Arc<Mutex<String>>) {
    let mut entries = fs::read_dir(&current_dir).await.expect("Failed to read directory");

    let mut tasks = Vec::new();

    while let Some(entry) = entries.next_entry().await.expect("Failed to read entry") {
        let path = entry.path();
        if path.is_file() {
            let target_dir_clone = Arc::clone(&target_dir);
            let task = tokio::task::spawn_local(async move {
                let new_file_path = {
                    let target_dir = target_dir_clone.lock().await;
                    PathBuf::from(&*target_dir).join(path.file_name().unwrap())
                };
                fs::rename(&path, &new_file_path).await.expect("Failed to move file");
            });
            tasks.push(task);
        } else if path.is_dir() {
            let target_dir_clone = Arc::clone(&target_dir);
            let task = tokio::task::spawn_local(async move {
                reverse_organize_dir(path, target_dir_clone).await;
            });
            tasks.push(task);
        }
    }

    for task in tasks {
        task.await.expect("Task failed");
    }
}
