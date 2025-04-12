use arboard::Clipboard;
use chrono::{Datelike, Local};
use clap::Parser;
use csv::Writer;
use daemonize::Daemonize;
use directories::BaseDirs;
use image::DynamicImage;
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
    thread,
    time::Duration,
};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the CSV file
    #[arg(short, long, value_name = "FILE")]
    output_dir: Option<String>,

    /// Interval in seconds to check the clipboard
    #[arg(short, long, default_value_t = 1)]
    interval: u64,

    /// run as background daemon
    #[arg(short, long)]
    daemon: bool,

    /// Directory to store clipboard images
    #[arg(short, long, value_name = "DIR")]
    image_dir: Option<PathBuf>,

    /// Directory for completed files ready for server upload
    #[arg(short = 'u', long, value_name = "DIR")]
    upload_dir: Option<PathBuf>,

    /// Enable automatic daily file rotation
    #[arg(short = 'r', long)]
    rotate_daily: bool,
}

#[derive(Debug, Serialize)]
struct ClipboardEntry {
    timestamp: String,
    content_type: String,
    content: String,
    image_path: Option<String>,
}

enum ClipboardContent {
    Text(String),
    Image(DynamicImage, String), //  (Image, filename)
    Other(String),               // Type descriptiion
    Empty,
}

fn get_clipboard_content(
    clipboard: &mut Clipboard,
    image_dir: &PathBuf,
) -> Result<ClipboardContent, Box<dyn std::error::Error>> {
    // first try  textt
    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            return Ok(ClipboardContent::Text(text));
        }
    }

    // then try image
    if let Ok(image) = clipboard.get_image() {
        // Create image directory  if it doesn't exist
        if !image_dir.exists() {
            fs::create_dir_all(image_dir)?;
        }

        // Create filename based on timestamp
        let timestamp = Local::now().format("%Y%m%d%H%M%S").to_string();
        let filename = format!("clipboard_{}.png", timestamp);
        let _path = image_dir.join(&filename);

        // Convert arboard ImageData to DynamicImage
        let width = image.width as u32;
        let height = image.height as u32;
        let bytes_vec = image.bytes.to_vec();

        // Create RgbaImage from raw bytes
        let img_buffer = image::RgbaImage::from_raw(width, height, bytes_vec)
            .ok_or("Failed to create image from clipboard data")?;

        // Convert to RGB image
        let dynamic_img = DynamicImage::ImageRgba8(img_buffer);

        // Return the image
        return Ok(ClipboardContent::Image(dynamic_img, filename));
    }

    // // try to determine if there's something else in clipboard
    // if clipboard.get_html().is_ok() {
    //     return Ok(ClipboardContent::Other("HTML".to_string()));
    // }
    //
    // if clipboard.get_data("application/rtf").is_ok() || clipboard.get_data("text/rtf").is_ok() {
    //     return Ok(ClipboardContent::Other("RTF".to_string()));
    // }

    // Nothing found
    Ok(ClipboardContent::Empty)
}

fn setup_daemon(program_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Get user log directory
    let base_dirs = BaseDirs::new().ok_or("Unable to find base directories")?;
    let log_dir = base_dirs.home_dir().join(".local/share/clipboard-logger");

    // create log directory  if it doesn't exist
    fs::create_dir_all(&log_dir)?;

    let stdout = File::create(log_dir.join("clipboard-logger.out"))?;
    let stderr = File::create(log_dir.join("clipboard-logger.err"))?;

    let username = whoami::username();

    let daemonize = Daemonize::new()
        .pid_file(log_dir.join("clipboard-logger.pid"))
        .chown_pid_file(true)
        .working_directory(&log_dir)
        .user(username.as_str())
        .group(username.as_str())
        .stdout(stdout)
        .stderr(stderr);

    daemonize.start()?;
    println!("Daemon started successfully: {}", program_name);

    // // Check if the daemon is already running
    // let pid_file = log_dir.join("clipboard-logger.pid");
    // if pid_file.exists() {
    //     let pid = fs::read_to_string(&pid_file)?;
    //     println!("Daemon is already running with PID: {}", pid);
    // } else {
    //     println!("Daemon started successfully: {}", program_name);
    // }

    Ok(())
}

// Simpel image hash function to compare images
fn calculate_image_hash(img: &DynamicImage) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Convert to grayscale and resize to a small size for quick comparision
    let small_img = img.resize(16, 16, image::imageops::FilterType::Lanczos3);
    let gray_img = small_img.grayscale();

    // Calculate hash based on pixel values
    let mut hasher = DefaultHasher::new();
    for pixel in gray_img.as_bytes() {
        pixel.hash(&mut hasher);
    }

    hasher.finish()
}

fn get_daily_csv_path(base_dir: &PathBuf) -> PathBuf {
    let now = Local::now();
    let data_str = now.format("%Y-%m-%d").to_string();
    base_dir.join(format!("clipboard_{}.csv", data_str))
}

fn rotate_csv_file(
    current_csv_path: &PathBuf,
    upload_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    // Only rotate if the file exists
    if current_csv_path.exists() {
        // Create the upload directory if it doesn't exist
        if !upload_dir.exists() {
            fs::create_dir_all(upload_dir)?;
        }

        //  Get yesterday's date
        let yesterday = Local::now()
            .checked_sub_signed(chrono::Duration::days(1))
            .ok_or("Failed to calculate yesterday's date")?;
        let yesterday_str = yesterday.format("%Y-%m-%d").to_string();

        //  Construct the file for yesterrday's file
        let csv_filename = format!("clipboard_{}.csv", yesterday_str);
        let yesterday_path = current_csv_path.with_file_name(&csv_filename);

        // If yesterday's file exists, move it to the upload directory
        if yesterday_path.exists() {
            let upload_path = upload_dir.join(&csv_filename);
            fs::rename(&yesterday_path, &upload_path)?;
            println!(
                "Rotated file {} to upload direcotry: {}",
                yesterday_path.display(),
                upload_path.display()
            );
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Handle daemon mode
    if args.daemon {
        setup_daemon("clipboard-logger")?;
    }

    // Determine base output directory
    let base_dir = match args.output_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            if let Some(base_dirs) = BaseDirs::new() {
                base_dirs.data_local_dir().join("clipboard-logger/data")
            } else {
                PathBuf::from("clipboard_data")
            }
        }
    };

    // Ensure base directory exists
    fs::create_dir_all(&base_dir)?;

    // Determine upload directory
    let upload_dir = match args.upload_dir {
        Some(path) => PathBuf::from(path),
        None => {
            if let Some(base_dirs) = BaseDirs::new() {
                base_dirs.data_local_dir().join("clipboard-logger/upload")
            } else {
                PathBuf::from("clipboard_upload")
            }
        }
    };

    // Ensure upload directory eixsts
    fs::create_dir_all(&upload_dir)?;

    // Determine image directory
    let image_dir = match args.image_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            if let Some(base_dirs) = BaseDirs::new() {
                base_dirs.data_local_dir().join("clipboard-logger/images")
            } else {
                PathBuf::from("clipboard_images")
            }
        }
    };

    // Ensure upload directory exists
    fs::create_dir_all(&image_dir)?;

    // Get today's CSV path
    let mut current_csv_path = get_daily_csv_path(&base_dir);

    // Get current date for day change detection
    let mut current_date = Local::now().day();

    // Check if the file exists to determine if we need headers
    let file_exists = current_csv_path.exists();

    println!("Starting clipboard logger service...");

    // Open the file for writing (create if it doesn't exist, append if it does)
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(&current_csv_path)?;

    // Create a CSV Writer
    let mut wtr = Writer::from_writer(file);

    // If it's a new file, write the header
    if !file_exists {
        wtr.write_record(&["timestamp", "content_type", "content", "image_path"])?;
        wtr.flush()?;
    }

    // Create a clipboard context
    let mut clipboard = Clipboard::new()?;
    let mut last_text_content = String::new();
    let mut last_image_hash: Option<u64> = None;

    println!(
        "Clipboard logger is running. Logging to {}",
        current_csv_path.display()
    );
    println!("Image directory: {}", image_dir.display());
    println!("Upload directory: {}", upload_dir.display());
    println!("Checking clipboard every {} seconds", args.interval);

    if args.rotate_daily {
        println!("Daily rotation enabled - files will be moved to the upload directory");
    }

    if !args.daemon {
        println!("Press Ctrl+C to stop the service.");
    }

    // Main loop - check clipboard every second
    loop {
        // Check if day has changed
        let now = Local::now();
        let today = now.day();

        if today != current_date {
            // Day has changed, rotate files if enabled
            if args.rotate_daily {
                // Flush and close current writer
                wtr.flush()?;
                drop(wtr);

                // Rotate files
                rotate_csv_file(&current_csv_path, &upload_dir)?;
            }

            // Update current date
            current_date = today;

            // Create new file for today
            current_csv_path = get_daily_csv_path(&base_dir);
            let new_file_exists = current_csv_path.exists();

            // Open new file
            let new_file = OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&current_csv_path)?;

            // Create new CSV writer
            wtr = Writer::from_writer(new_file);

            // Writer headers if it's a new file
            if !new_file_exists {
                wtr.write_record(&["timestamp", "content_type", "content", "image_path"])?;
                wtr.flush()?;
            }

            if !args.daemon {
                println!("Day changed. New log file: {}", current_csv_path.display());
            }
        }

        // Get current clipboard content
        match get_clipboard_content(&mut clipboard, &image_dir) {
            Ok(content) => {
                match content {
                    ClipboardContent::Text(text) => {
                        // Only log if the content has changed
                        if !text.is_empty() && text != last_text_content {
                            // get current timestamp
                            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                            // create a new ClipboardEntry
                            let entry = ClipboardEntry {
                                timestamp,
                                content_type: "text".to_string(),
                                content: text.clone(),
                                image_path: None,
                            };

                            // Write to CSV
                            wtr.serialize(&entry)?;
                            wtr.flush()?;

                            if !args.daemon {
                                println!("Logged new text content at {}", Local::now());
                            }

                            // Update last_content
                            last_text_content = text;
                            last_image_hash = None;
                        }
                    }
                    ClipboardContent::Image(image, filename) => {
                        // Calculate image hash for comparison
                        let image_hash = calculate_image_hash(&image);

                        // Only log if the content has changed
                        if last_image_hash != Some(image_hash) {
                            // Save the image
                            let path = image_dir.join(&filename);
                            image.save(&path)?;

                            // get current timestamp
                            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                            // create a new ClipboardEntry
                            let entry = ClipboardEntry {
                                timestamp,
                                content_type: "image".to_string(),
                                content: format!("Image: {}", filename),
                                image_path: Some(path.to_string_lossy().to_string()),
                            };

                            // Write to CSV
                            wtr.serialize(&entry)?;
                            wtr.flush()?;

                            if !args.daemon {
                                println!("Logged new image content at {}", Local::now());
                            }

                            // Update last_content
                            last_text_content = String::new();
                            last_image_hash = Some(image_hash);
                        }
                    }
                    ClipboardContent::Other(content_type) => {
                        // Log other content types if needed
                        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        let entry = ClipboardEntry {
                            timestamp,
                            content_type: content_type.clone(),
                            content: format!("[Unsupported content: {}]", content_type),
                            image_path: None,
                        };

                        // Write to CSV
                        wtr.serialize(&entry)?;
                        wtr.flush()?;

                        if !args.daemon {
                            println!("Logged other content at {}", Local::now());
                        }

                        // Update last_content
                        last_text_content = String::new();
                        last_image_hash = None;
                    }
                    ClipboardContent::Empty => {
                        // Do nothing for empty content
                        if !args.daemon {
                            println!("Clipboard is empty at {}", Local::now());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading clipboard: {}", e);
            }
        }

        // Wait for the interval before checking again
        thread::sleep(Duration::from_secs(args.interval));
    }
}
