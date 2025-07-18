use log::{debug, error, info};
use serenity::all::{
    Colour, CreateEmbed, CreateEmbedFooter, CreateMessage, EditMessage, MessageUpdateEvent,
};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::collections::HashMap;
use std::env;
use std::time::Instant;

// Constants
const WORDLE_APP_ID: u64 = 1211781489931452447;
const PLAYING_TRIGGERS: [&str; 2] = ["is playing", "are playing"];
const FINISHED_TRIGGERS: [&str; 2] = ["was playing", "were playing"];
const EMBED_TITLE: &str = "🧩 Wordle Solved!";
const EMBED_FOOTER: &str = "Time tracked by Matt's third brain.";
const EMBED_COLOR: (u8, u8, u8) = (87, 242, 135); // A nice green color

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Australia::Sydney;

// Struct to store game state and metadata
struct GameState {
    last_start_time: Instant,               // When the current attempt started
    total_active_time: std::time::Duration, // Total time spent actively solving
    completion_msg_id: Option<serenity::model::id::MessageId>, // ID of the completion message if one exists
    created_at: DateTime<Utc>, // When this game was first started (stored in UTC)
}

impl GameState {
    /// Creates a new GameState instance
    fn new() -> Self {
        Self {
            last_start_time: Instant::now(),
            total_active_time: std::time::Duration::ZERO,
            completion_msg_id: None,
            created_at: Utc::now(),
        }
    }

    /// Checks if this game is from the current day in Sydney timezone
    fn is_current(&self) -> bool {
        let now_sydney = Utc::now().with_timezone(&Sydney);
        let created_sydney = self.created_at.with_timezone(&Sydney);
        created_sydney.date() == now_sydney.date()
    }
}

// Struct to store active games
struct WordlePuzzles;

impl TypeMapKey for WordlePuzzles {
    type Value = tokio::sync::Mutex<HashMap<(serenity::model::id::MessageId, String), GameState>>;
}

/// Parse usernames from a message content string.
/// Handles both single user ("User was playing") and multi-user ("User1 and User2 were playing") cases.
fn parse_usernames(content: &str) -> Vec<String> {
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
    let usernames = if before.contains(" and ") {
        before.split(" and ").map(|s| s.trim().to_owned()).collect()
    } else {
        // Single user case
        vec![before.to_owned()]
    };

    info!("Found {} usernames: {:?}", usernames.len(), usernames);
    usernames
}

struct Handler {
    daily_puzzles_channel_name: String,
}

/// Format a duration into a human-readable string
fn format_duration(duration: std::time::Duration) -> String {
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

impl Handler {
    /// Creates an embed for a Wordle completion message
    fn create_completion_embed(
        user_name: &str,
        total_time: std::time::Duration,
        is_update: bool,
    ) -> CreateEmbed {
        let description = if is_update {
            format!(
                "{} finished their Wordle in **{}**! (Updated)",
                user_name,
                format_duration(total_time)
            )
        } else {
            format!(
                "{} finished their Wordle in **{}**!",
                user_name,
                format_duration(total_time)
            )
        };

        CreateEmbed::new()
            .title(EMBED_TITLE)
            .description(description)
            .colour(Colour::from_rgb(
                EMBED_COLOR.0,
                EMBED_COLOR.1,
                EMBED_COLOR.2,
            ))
            .footer(CreateEmbedFooter::new(EMBED_FOOTER))
    }

    /// Validates if a message is from the Wordle app and in the correct channel
    async fn validate_message(
        &self,
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        author_id: serenity::model::id::UserId,
    ) -> Result<(), &'static str> {
        // Check if message is from Wordle app
        if author_id != serenity::model::id::UserId::new(WORDLE_APP_ID) {
            return Err("Not from Wordle app");
        }

        // Check channel name
        let channel_name = channel_id
            .name(&ctx.http)
            .await
            .map_err(|_| "Unable to get channel information")?;

        if channel_name.to_lowercase() != self.daily_puzzles_channel_name.to_lowercase() {
            return Err("Not in daily puzzles channel");
        }

        Ok(())
    }
}

#[async_trait]
impl EventHandler for Handler {
    // Fired when the bot successfully connects to Discord
    async fn ready(&self, _: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
    }

    // Fired when a new message is created
    async fn message(&self, ctx: Context, msg: Message) {
        // Validate message is from Wordle app and in correct channel
        if let Err(why) = self
            .validate_message(&ctx, msg.channel_id, msg.author.id)
            .await
        {
            info!("Message validation failed: {}", why);
            return;
        }

        let content = msg.content.to_lowercase();
        debug!("{}", content);

        let Some(_guild_id) = msg.guild_id else {
            info!("Missing guild id");
            return;
        };

        // Check if this is a Wordle game start or resume
        if PLAYING_TRIGGERS
            .iter()
            .any(|&trigger| content.contains(trigger))
        {
            let data_read = ctx.data.read().await;
            let puzzle_lock = data_read
                .get::<WordlePuzzles>()
                .expect("Expected WordlePuzzles in TypeMap")
                .lock();

            // Parse all usernames from the message
            let usernames = parse_usernames(&content);

            // Create a timer entry for each user
            let mut puzzle_map = puzzle_lock.await;
            for username in &usernames {
                let mut entry = puzzle_map.entry((msg.id, username.clone()));
                match entry {
                    std::collections::hash_map::Entry::Occupied(ref mut entry) => {
                        // Check if game is from a previous day
                        let is_current = entry.get().is_current();
                        if !is_current {
                            info!("Resetting game from previous day");
                            // Reset game state for new day
                            entry.insert(GameState::new());
                            info!("Previous day's game replaced for user: {}", username);
                        } else {
                            // This is a resume - update total active time and start new attempt
                            let game_state = entry.get_mut();
                            game_state.total_active_time +=
                                Instant::now().duration_since(game_state.last_start_time);
                            game_state.last_start_time = Instant::now();
                            info!("Resumed game for user: {}", username);
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(vacant) => {
                        // This is a new game
                        vacant.insert(GameState::new());
                        info!("Started new game for user: {}", username);
                    }
                }
            }
            drop(puzzle_map);

            info!(
                "Tracking Wordle for message ID: {} with {} users",
                msg.id,
                usernames.len()
            );
        }
    }

    // Fired when an existing message is updated (e.g., edited)
    async fn message_update(
        &self,
        ctx: Context,
        _old_if_available: Option<Message>,
        _new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        debug!("Message update fired");

        // Get author from event
        let author = match event.author {
            Some(v) => v,
            None => {
                info!("Unable to find author of the message update");
                return;
            }
        };

        // Validate message is from Wordle app and in correct channel
        if let Err(why) = self
            .validate_message(&ctx, event.channel_id, author.id)
            .await
        {
            info!("Message validation failed: {}", why);
            return;
        }

        // Get content from event
        let Some(content) = event.content else {
            info!("Unable to find content of the message update");
            return;
        };

        let Some(_guild_id) = event.guild_id else {
            info!("Missing guild id");
            return;
        };

        // Check if this is a game start/resume or completion
        let is_playing = PLAYING_TRIGGERS
            .iter()
            .any(|&trigger| content.contains(trigger));
        let is_finished = FINISHED_TRIGGERS
            .iter()
            .any(|&trigger| content.contains(trigger));

        // Get the shared data
        let data_read = ctx.data.read().await;
        let puzzle_lock = data_read
            .get::<WordlePuzzles>()
            .expect("Expected WordlePuzzles in TypeMap")
            .lock();

        // Parse usernames from content
        let usernames = parse_usernames(&content);
        info!(
            "Message update - Found {} users: {:?}",
            usernames.len(),
            usernames
        );

        let mut puzzle_map = puzzle_lock.await;

        if is_playing {
            info!("Processing game start/resume from message edit");
            // Handle game start/resume
            for username in &usernames {
                let entry = puzzle_map.entry((event.id, username.clone()));
                match entry {
                    std::collections::hash_map::Entry::Occupied(ref mut entry) => {
                        // Check if game is from a previous day
                        let is_current = entry.get().is_current();
                        if !is_current {
                            info!("Resetting game from previous day for {}", username);
                            // Reset game state for new day
                            entry.insert(GameState::new());
                        } else {
                            // This is a resume - update total active time and start new attempt
                            let game_state = entry.get_mut();
                            game_state.total_active_time +=
                                Instant::now().duration_since(game_state.last_start_time);
                            game_state.last_start_time = Instant::now();
                            info!(
                                "Resumed game for {} (total time: {:?})",
                                username, game_state.total_active_time
                            );
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(vacant) => {
                        // This is a new game
                        vacant.insert(GameState::new());
                        info!("Started new game for {}", username);
                    }
                }
            }
        } else if is_finished {
            info!("Processing game completion from message edit");
            // Handle game completion
            for user_name in &usernames {
                if let Some(game_state) = puzzle_map.get_mut(&(event.id, user_name.clone())) {
                    // Add the time from the current attempt
                    let current_attempt_time =
                        Instant::now().duration_since(game_state.last_start_time);
                    let total_time = game_state.total_active_time + current_attempt_time;

                    info!(
                        "User {} completed game - Current attempt: {:?}, Total time: {:?}",
                        user_name, current_attempt_time, total_time
                    );

                    // Send or update completion message
                    if let Some(msg_id) = game_state.completion_msg_id {
                        info!("Updating existing completion message");
                        // Update existing completion message
                        let embed_msg = Self::create_completion_embed(user_name, total_time, true);
                        if let Ok(mut message) = event.channel_id.message(&ctx.http, msg_id).await {
                            if let Err(why) = message
                                .edit(&ctx.http, |m: &mut EditMessage| m.embed(embed_msg))
                                .await
                            {
                                error!("Error updating completion message: {:?}", why);
                            }
                        }
                    } else {
                        info!("Sending new completion message");
                        // Send new completion message
                        let embed_msg = Self::create_completion_embed(user_name, total_time, false);
                        if let Ok(sent_msg) = event
                            .channel_id
                            .send_message(&ctx.http, |m: &mut CreateMessage| m.embed(embed_msg))
                            .await
                        {
                            game_state.completion_msg_id = Some(sent_msg.id);
                            info!("Created new completion message with ID: {:?}", sent_msg.id);
                        }
                    }

                    // Update the game state with final time
                    game_state.total_active_time = total_time;
                } else {
                    info!("No game state found for user {}", user_name);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Load environment variables from .env file
    dotenv::dotenv().expect("Failed to load .env file");
    env_logger::init();

    // Configure the Discord bot token and channel name from environment variables
    let token = env::var("DISCORD_TOKEN").expect("Expected a DISCORD_TOKEN in the environment");
    let daily_puzzles_channel_name =
        env::var("DAILY_PUZZLES_CHANNEL_NAME").unwrap_or_else(|_| "daily-puzzles".to_string()); // Default to "daily-puzzles" if not set

    // Create a new instance of the Discord client
    let mut client = Client::builder(
        &token,
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT,
    )
    .event_handler(Handler {
        daily_puzzles_channel_name,
    })
    .await
    .expect("Error creating client");

    // Initialize the shared data for storing active puzzles
    {
        let mut data = client.data.write().await;
        data.insert::<WordlePuzzles>(Mutex::new(HashMap::new()));
    }

    // Start the client, blocking until it's disconnected
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}
