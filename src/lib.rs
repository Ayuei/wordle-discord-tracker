pub mod detection;

use anyhow::Result;
use log::info;
use opencv::imgcodecs;
use tokio::{fs, io::AsyncWriteExt};

const DATA_DIR: &'static str = "./data";

pub async fn download_image(url: &String) -> Result<String> {
    let file_path = format!("{DATA_DIR}/{}", url.split("/").last().unwrap());
    info!("Downloading image from {url}");
    // Send the HTTP request
    let response = reqwest::get(url).await?.bytes().await?;

    // Create and open the output file
    let mut file = fs::File::create(&file_path).await?;

    // Write the image bytes to the file
    file.write_all(&response).await?;

    info!("Succesfully downloaded image and saved to {file_path}");

    Ok(file_path)
}

#[derive(Debug)]
pub struct Player {
    pub uid: usize,
    pub username: String,
    pub profile_url: String,
    pub downloaded_fp: Option<String>,
    pub completed: bool,
}

impl Player {
    pub fn new(uid: usize, username: String, profile_url: String) -> Player {
        Player {
            uid,
            username,
            profile_url,
            downloaded_fp: None,
            completed: false,
        }
    }

    pub async fn download_profile_picture(&mut self) -> Result<String> {
        match &self.downloaded_fp {
            Some(v) => Ok(v.clone()),
            None => {
                let fp = download_image(&self.profile_url).await?;
                self.downloaded_fp = Some(fp.clone());
                Ok(fp)
            }
        }
    }
}

/// Verify if a specific player has completed their Wordle puzzle
///
/// # Arguments
/// * `player` - The player to verify
/// * `haystack_fp` - Path to the screenshot containing potential completions
///
/// # Returns
/// * `Ok(bool)` - Whether the player has completed their puzzle
pub async fn verify_player_completion(player: &mut Player, haystack_fp: String) -> Result<bool> {
    let haystack = imgcodecs::imread(&haystack_fp, imgcodecs::IMREAD_COLOR_RGB)?;

    // First check if there are any completions in the image
    let needle = imgcodecs::imread("./data/solved.png", imgcodecs::IMREAD_COLOR_RGB)?;
    let completions =
        detection::detect_needle_in_haystack(&needle, &haystack, 30, 0.1, 1.0, 100, 1.0)?;

    if completions.is_empty() {
        println!("No completions found");
        return Ok(false); // No completions found in image
    }

    println!("Found {} completions", completions.len());
    println!("{:?}", completions);

    // Now check if this player's avatar is next to a completion
    let image_path = player.download_profile_picture().await?;
    let needle = imgcodecs::imread(&image_path, imgcodecs::IMREAD_COLOR_RGB)?;

    let found = detection::detect_needle_in_haystack(&needle, &haystack, 1, 0.1, 1.0, 100, 0.84)?;
    println!("Found {:?} avatar", found);

    if found.len() == 1 {
        let x_coord_1 = found[0].0.0.x;
        let x_coord_2 = found[0].0.1.x;

        let center = (x_coord_1 + x_coord_2) / 2;

        // Check if the center of the player's avatar intersects with a completion marker
        let completed = completions.iter().any(|f| {
            println!("{}, {}", f.0.0.x, f.0.1.x);
            (f.0.0.x < center) && (f.0.1.x > center)
        });

        println!("Completed: {completed}, Center: {center}");

        player.completed = completed;
        Ok(completed)
    } else {
        Ok(false)
    }
}

/// Format a duration into a human-readable string
pub fn format_duration(duration: std::time::Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let remaining_seconds_after_hours = total_seconds % 3600;
    let minutes = remaining_seconds_after_hours / 60;
    let seconds = remaining_seconds_after_hours % 60;
    let milliseconds = duration.subsec_millis();

    let mut time_parts = Vec::new();

    if hours > 0 {
        time_parts.push(format!(
            "{} hour{}",
            hours,
            if hours != 1 { "s" } else { "" }
        ));
    }
    if minutes > 0 {
        time_parts.push(format!(
            "{} minute{}",
            minutes,
            if minutes != 1 { "s" } else { "" }
        ));
    }
    // Always include seconds and milliseconds
    time_parts.push(format!(
        "{}.{:03} second{}",
        seconds,
        milliseconds,
        if seconds != 1 { "s" } else { "" }
    ));

    if time_parts.len() == 1 {
        time_parts[0].clone()
    } else {
        let last_part = time_parts.pop().unwrap(); // Safe to unwrap as we always have milliseconds
        format!("{} and {}", time_parts.join(", "), last_part)
    }
}
