pub mod detection;

use anyhow::Result;
use log::info;
use opencv::imgcodecs;
use tokio::{fs, io::AsyncWriteExt};

pub const PLAYING_TRIGGERS: [&str; 2] = ["is playing", "are playing"];
pub const FINISHED_TRIGGERS: [&str; 2] = ["was playing", "were playing"];

const DATA_DIR: &'static str = "./data";

/// Parse usernames from the server by seeing if their profile picture is in the picture.
pub fn parse_usernames(content: &String) -> Vec<String> {
    let content = content.to_lowercase();

    // Try each trigger to find which one matches
    let (before, is_plural) = PLAYING_TRIGGERS
        .iter()
        .find_map(|&trigger| {
            content
                .split_once(trigger)
                .map(|(s, _)| (s.trim(), trigger == "are playing"))
        })
        .or_else(|| {
            FINISHED_TRIGGERS.iter().find_map(|&trigger| {
                content
                    .split_once(trigger)
                    .map(|(s, _)| (s.trim(), trigger == "were playing"))
            })
        })
        .unwrap_or(("", false));

    info!(
        "Parsing usernames from: '{}' (plural: {})",
        before, is_plural
    );

    // If there's " and " in the string, it's a multi-user case
    let mut usernames = if before.contains(" and ") {
        before.split(" and ").map(|s| s.trim().to_owned()).collect()
    } else {
        // Single user case
        vec![before.to_owned()]
    };

    // Check for edge cases like "2 others", "3 others", etc.
    if let Some(last_username) = usernames.last() {
        if last_username.chars().next().unwrap_or(' ').is_numeric()
            && last_username.ends_with(" others")
        {
            // Edge case with pattern like "2 others", "3 others", etc.
            usernames.clear();
        }
    }

    info!("Found {} usernames: {:?}", usernames.len(), usernames);

    usernames
}

async fn download_image(url: &String) -> Result<String> {
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

pub struct Player {
    uid: usize,
    profile_url: String,
}

impl Player {
    pub fn new(uid: usize, profile_url: String) -> Player {
        Player { uid, profile_url }
    }
}

pub async fn find_players_in_image(
    players: Vec<Player>,
    haystack_url: String,
) -> Result<Vec<Player>> {
    let haystack_fp = download_image(&haystack_url).await?;
    let haystack = imgcodecs::imread(&haystack_fp, imgcodecs::IMREAD_COLOR_RGB)?;
    let mut found_players = Vec::new();

    for player in players {
        let image_path = download_image(&player.profile_url).await?;
        let needle = imgcodecs::imread(&image_path, imgcodecs::IMREAD_COLOR_RGB)?;
        let found =
            detection::detect_needle_in_haystack(&needle, &haystack, 1, 0.6, 1.4, 100, 0.95)?;

        if found.len() == 1 {
            found_players.push(player);
        }
    }

    Ok(found_players)
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
