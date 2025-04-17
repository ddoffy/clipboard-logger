// reading csv file
//timestamp,content_type,content,image_path

use clap::Parser;
use image::DynamicImage;
use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the CSV file
    #[arg(short, long, value_name = "FILE")]
    pub output_dir: Option<String>,

    /// Interval in seconds to check the clipboard
    #[arg(short, long, default_value_t = 1)]
    pub interval: u64,

    /// Directory to store clipboard images
    #[arg(long, value_name = "DIR")]
    pub image_dir: Option<PathBuf>,

    /// Directory for completed files ready for server upload
    #[arg(short = 'u', long, value_name = "DIR")]
    pub upload_dir: Option<PathBuf>,

    /// Enable automatic daily file rotation
    #[arg(short = 'r', long)]
    pub rotate_daily: bool,

    #[arg(short, long, default_value_t = false)]
    pub sync: bool,
}

#[derive(Debug, Serialize)]
struct ClipboardRequest {
    pub title: Option<String>,
    pub content_type: Option<String>,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ClipboardEntry {
    pub timestamp: String,
    pub content_type: String,
    pub content: String,
    pub image_path: Option<String>,
}

pub enum ClipboardContent {
    Text(String),
    Image(DynamicImage, String), //  (Image, filename)
    Other(String),               // Type descriptiion
    Empty,
}

pub fn read_csv_file<P: AsRef<Path>>(path: P) -> Result<Vec<ClipboardEntry>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;

    // csv file contnet in the format with double quotes
    // add settings to read the csv file
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .double_quote(true)
        .from_reader(contents.as_bytes());

    // Skip the header line
    let headers = reader.headers()?;
    if headers.len() != 4 {
        return Err("Invalid CSV file format".into());
    }

    // Read the CSV file and parse the entries
    let mut result = Vec::new();

    for record in reader.records() {
        let record = record?;
        if record.len() != 4 {
            continue;
        }
        let timestamp = record[0].to_string();
        let content_type = record[1].to_string();
        let content = record[2].to_string();
        let image_path = record[3].to_string();

        result.push(ClipboardEntry {
            timestamp,
            content_type,
            content,
            image_path: if image_path.is_empty() {
                None
            } else {
                Some(image_path)
            },
        });
    }

    Ok(result)
}

pub fn sync_clipboard(clipboards: Vec<ClipboardEntry>) -> Result<(), Box<dyn Error>> {
    // read endpoint to upload clipboard fromm environment variable
    let upload_endpoint = std::env::var("CLIPBOARD_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:8000/api/v1/upload".to_string());
    let clipboard_endpoint = std::env::var("CLIPBOARD_API_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:8000/api/v1/clipboard".to_string());

    // read token from environment variable
    let token = std::env::var("CLIPBOARD_API_KEY").unwrap_or_else(|_| "your_token".to_string());

    // Sync the clipboard entries to the server
    for entry in clipboards {
        // Here you can implement the logic to sync each entry to the server
        // For example, you can send a POST request to your server with the entry data
        println!("Syncing entry: {:?}", entry);

        // Create the request body
        let mut request_body = ClipboardRequest {
            title: Some(entry.timestamp.clone()),
            content_type: Some(entry.content_type.clone()),
            content: entry.content.clone(),
        };

        // If the entry has an image, you can upload it separately
        if let Some(image_path) = entry.image_path {
            // Check if the image path is valid
            let image_path = Path::new(&image_path);

            if image_path.exists() {
                // Upload the image
                println!("Uploading image: {:?}", image_path);
                // Implement your image upload logic here
                let server_resp = upload_image(&image_path, &upload_endpoint);

                if server_resp.is_err() {
                    println!("Failed to upload image: {:?}", server_resp);
                    continue;
                }

                let server_path = server_resp.unwrap();

                // Append image path to the request body's content
                request_body
                    .content
                    .push_str(&format!("\nImage uploaded to: {}", server_path));
            } else {
                println!("Image not found: {:?}", image_path);
            }
        }

        // Send the request to the server
        let response = send_clipboard_to_server(&clipboard_endpoint, &token, &request_body);
        if response.is_err() {
            println!("Failed to sync clipboard entry: {:?}", response);
            continue;
        }
    }
    Ok(())
}

fn send_clipboard_to_server(
    endpoint: &str,
    token: &str,
    request_body: &ClipboardRequest,
) -> Result<(), Box<dyn Error>> {
    let client = reqwest::blocking::Client::new();

    // Send the request to the server
    let response = client
        .post(endpoint)
        .header("X-API-KEY", token)
        .json(request_body)
        .send()?;

    if response.status().is_success() {
        println!("Successfully synced clipboard entry");
        Ok(())
    } else {
        let status = response.status();
        let error_body = response
            .text()
            .unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!(
            "Failed to sync clipboard entry: {} - {}",
            status, error_body
        )
        .into())
    }
}

fn upload_image(image_path: &Path, endpoint: &str) -> Result<String, Box<dyn Error>> {
    let client = reqwest::blocking::Client::new();

    // Create the multipart part for the file
    let file_part = reqwest::blocking::multipart::Part::file(image_path)?.mime_str("image/*")?; // Optionally set a MIME type

    // Create the form and add the file part
    let form = reqwest::blocking::multipart::Form::new().part("file", file_part); // "file" is the field name the server expects

    let response = client.post(endpoint).multipart(form).send()?;

    if response.status().is_success() {
        // Parse the response body as JSON into a generic Value
        let json_response: Value = response.json()?;

        // Access the "file_url" field from the parsed JSON
        // Use .get() and .and_then() for safe access and type checking
        let server_path = json_response
            .get("file_url")
            .and_then(Value::as_str)
            .map(String::from) // Convert &str to String
            .ok_or("Failed to extract 'file_url' from response or it's not a string")?; // Provide error if missing/wrong type

        Ok(server_path)
    } else {
        // Include response body in error if possible for more context
        let status = response.status();
        let error_body = response
            .text()
            .unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!("Failed to upload image: {} - {}", status, error_body).into())
    }
}
