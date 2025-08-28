use log::{debug, error, info, warn};
use serenity::all::{Colour, CreateEmbed, CreateEmbedFooter, CreateMessage, EditMessage, Presence};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::UserId;
use serenity::prelude::*;
use std::collections::HashMap;
use std::env;
use std::time::Instant;
use wordle_timer_bot::{Player, download_image, format_duration, verify_player_completion};

// Constants
const WORDLE_APP_ID: u64 = 1211781489931452447;
const WORDLE_ACTIVITY_NAME: &str = "Wordle";
const EMBED_TITLE: &str = "🧩 Wordle Solved!";
const EMBED_FOOTER: &str = "Time tracked by Matt's third brain.";
const EMBED_COLOR: (u8, u8, u8) = (87, 242, 135); // A nice green color

use chrono::{DateTime, Utc};
use chrono_tz::Australia::Sydney;

// Struct to store game state and metadata
struct GameState {
    user_id: UserId,                                           // Discord user ID
    last_start_time: Instant,                                  // When the current attempt started
    total_active_time: std::time::Duration,                    // Total time spent actively solving
    completion_msg_id: Option<serenity::model::id::MessageId>, // ID of the completion message if one exists
    created_at: DateTime<Utc>, // When this game was first started (stored in UTC)
    completed: bool,           // Whether the game has been completed
    channel_id: Option<serenity::model::id::ChannelId>, // Channel where completion was detected
}

impl GameState {
    /// Creates a new GameState instance
    fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            last_start_time: Instant::now(),
            total_active_time: std::time::Duration::ZERO,
            completion_msg_id: None,
            created_at: Utc::now(),
            completed: false,
            channel_id: None,
        }
    }

    /// Checks if this game is from the current day in Sydney timezone
    fn is_current(&self) -> bool {
        let now_sydney = Utc::now().with_timezone(&Sydney);
        let created_sydney = self.created_at.with_timezone(&Sydney);
        created_sydney.date_naive() == now_sydney.date_naive()
    }

    /// Updates the total active time and resets the start time
    fn update_active_time(&mut self) {
        self.total_active_time += Instant::now().duration_since(self.last_start_time);
        self.last_start_time = Instant::now();
    }
}

// Struct to store active games
struct WordlePuzzles;

impl TypeMapKey for WordlePuzzles {
    type Value = tokio::sync::Mutex<HashMap<UserId, GameState>>;
}

struct Handler {
    daily_puzzles_channel_name: String,
}

/// Custom error type for member-related operations
#[derive(Debug)]
pub enum MemberError {
    NotFound(String),
    ApiError(String),
    RetryExhausted(String),
}

impl std::fmt::Display for MemberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemberError::NotFound(msg) => write!(f, "Member not found: {}", msg),
            MemberError::ApiError(msg) => write!(f, "API error: {}", msg),
            MemberError::RetryExhausted(msg) => write!(f, "Retry attempts exhausted: {}", msg),
        }
    }
}

impl std::error::Error for MemberError {}

impl Handler {
    /// Attempts to get a guild member with retry logic
    async fn get_member_with_retry(
        ctx: &Context,
        guild_id: Option<serenity::model::id::GuildId>,
        user_id: serenity::model::id::UserId,
    ) -> Result<serenity::model::guild::Member, Box<dyn std::error::Error + Send + Sync>> {
        const MAX_RETRIES: u32 = 3;
        let guild_id = guild_id.ok_or(MemberError::NotFound("No guild ID provided".to_string()))?;

        let mut last_error = None;
        for retry in 0..MAX_RETRIES {
            match guild_id.member(&ctx.http, user_id).await {
                Ok(member) => return Ok(member),
                Err(e) => {
                    warn!(
                        "Failed to get member info (attempt {}/{}): {}",
                        retry + 1,
                        MAX_RETRIES,
                        e
                    );
                    last_error = Some(e);
                    if retry < MAX_RETRIES - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1 << retry)).await;
                    }
                }
            }
        }

        Err(Box::new(MemberError::RetryExhausted(format!(
            "Failed after {} attempts: {:?}",
            MAX_RETRIES, last_error
        ))))
    }

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

    /// Send a new completion message
    async fn send_completion_message(
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        username: &str,
        total_time: std::time::Duration,
        game_state: &mut GameState,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let embed_msg = Self::create_completion_embed(username, total_time, false);
        let sent_msg = channel_id
            .send_message(&ctx.http, CreateMessage::new().embed(embed_msg))
            .await?;

        game_state.completion_msg_id = Some(sent_msg.id);
        info!(
            "Sent completion message for user {} - Time: {:?}",
            username, total_time
        );
        Ok(())
    }

    /// Update an existing completion message
    async fn update_completion_message(
        ctx: &Context,
        channel_id: serenity::model::id::ChannelId,
        message_id: serenity::model::id::MessageId,
        username: &str,
        total_time: std::time::Duration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let embed_msg = Self::create_completion_embed(username, total_time, true);
        let mut message = channel_id.message(&ctx.http, message_id).await?;
        message
            .edit(&ctx.http, EditMessage::new().embed(embed_msg))
            .await?;

        info!(
            "Updated completion message for user {} - Time: {:?}",
            username, total_time
        );
        Ok(())
    }

    /// Verify completion with retry logic
    async fn verify_completion_with_retry(
        player: &mut Player,
        haystack_fp: &str,
        max_retries: u32,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let mut retries = 0;
        let mut last_error = None;

        while retries < max_retries {
            match verify_player_completion(player, haystack_fp.to_string()).await {
                Ok(completed) => return Ok(completed),
                Err(e) => {
                    warn!(
                        "Retry {} failed for user {}: {}",
                        retries + 1,
                        player.username,
                        e
                    );
                    last_error = Some(e);
                    retries += 1;
                    tokio::time::sleep(tokio::time::Duration::from_secs(1 << retries)).await;
                }
            }
        }

        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "Failed to verify completion after {} retries: {:?}",
                max_retries, last_error
            ),
        )))
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

    // Fired when a user's presence is updated
    async fn presence_update(&self, ctx: Context, presence: Presence) {
        let user_id = presence.user.id;

        // Find Wordle activity if it exists
        let wordle_activity = presence.activities.iter().find(|activity| {
            activity.name == WORDLE_ACTIVITY_NAME
                && activity
                    .application_id
                    .map_or(false, |id| id.get() == WORDLE_APP_ID)
        });

        let data_read = ctx.data.read().await;
        let puzzle_lock = data_read
            .get::<WordlePuzzles>()
            .expect("Expected WordlePuzzles in TypeMap")
            .lock();
        let mut puzzle_map = puzzle_lock.await;

        match wordle_activity {
            Some(activity) => {
                // User is playing Wordle
                debug!(
                    "User {} is playing Wordle (state: {:?})",
                    user_id, activity.state
                );

                match puzzle_map.entry(user_id) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        let game_state = entry.get_mut();
                        if !game_state.is_current() {
                            // Reset for new day
                            *game_state = GameState::new(user_id);
                            info!(
                                "Reset game state for new day - User: {} (previous time: {:?})",
                                user_id, game_state.total_active_time
                            );
                        } else {
                            debug!(
                                "Continuing existing game for user {} (current time: {:?})",
                                user_id, game_state.total_active_time
                            );
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        // Start new game tracking
                        entry.insert(GameState::new(user_id));
                        info!("Started tracking new Wordle game for user: {}", user_id);
                    }
                }
            }
            None => {
                // User is not playing Wordle
                if let Some(game_state) = puzzle_map.get_mut(&user_id) {
                    if !game_state.completed {
                        game_state.update_active_time();
                        info!(
                            "User {} stopped playing - Total active time: {:?}",
                            user_id, game_state.total_active_time
                        );
                    } else {
                        debug!(
                            "Ignoring presence update for completed game - User: {}",
                            user_id
                        );
                    }
                } else {
                    debug!("No active game found for user: {}", user_id);
                }
            }
        }
    }

    // Fired when a new message is created
    async fn message(&self, ctx: Context, msg: Message) {
        // Only process messages from Wordle app in the correct channel
        if let Err(why) = self
            .validate_message(&ctx, msg.channel_id, msg.author.id)
            .await
        {
            debug!("Message validation failed: {}", why);
            return;
        }

        // Check for completion message
        if let Some(attachment) = msg.attachments.last() {
            let data_read = ctx.data.read().await;
            let puzzle_lock = data_read
                .get::<WordlePuzzles>()
                .expect("Expected WordlePuzzles in TypeMap")
                .lock();
            let mut puzzle_map = puzzle_lock.await;

            // Download the completion image
            let haystack_fp = match download_image(&attachment.url).await {
                Ok(fp) => fp,
                Err(e) => {
                    error!("Failed to download image: {}", e);
                    return;
                }
            };

            // Check each active player for completion
            for (user_id, game_state) in puzzle_map.iter_mut() {
                // Skip if:
                // 1. Game is already completed
                // 2. Game is not from today
                // 3. User is not currently playing
                if game_state.completed || !game_state.is_current() {
                    debug!(
                        "Skipping user {} - completed: {}, current: {}",
                        user_id,
                        game_state.completed,
                        game_state.is_current()
                    );
                    continue;
                }

                // Get member information with retry
                let member = match Self::get_member_with_retry(&ctx, msg.guild_id, *user_id).await {
                    Ok(member) => member,
                    Err(e) => {
                        warn!("Could not find member info for user {}: {}", user_id, e);
                        continue;
                    }
                };

                // Create player object for verification
                let mut player = Player::new(
                    user_id.get() as usize,
                    member.display_name().to_string(),
                    member.user.default_avatar_url(),
                );

                // Verify if this player has completed (with retry)
                match Self::verify_completion_with_retry(&mut player, &haystack_fp, 5).await {
                    Ok(true) => {
                        info!("Detected completion for user {}", user_id);
                        game_state.completed = true;
                        game_state.channel_id = Some(msg.channel_id);
                        game_state.update_active_time();

                        // Send or update completion message
                        if let Some(msg_id) = game_state.completion_msg_id {
                            // Update existing message
                            if let Err(e) = Self::update_completion_message(
                                &ctx,
                                msg.channel_id,
                                msg_id,
                                &player.username,
                                game_state.total_active_time,
                            )
                            .await
                            {
                                error!("Failed to update completion message: {}", e);
                            }
                        } else {
                            // Send new completion message
                            if let Err(e) = Self::send_completion_message(
                                &ctx,
                                msg.channel_id,
                                &player.username,
                                game_state.total_active_time,
                                game_state,
                            )
                            .await
                            {
                                error!("Failed to send completion message: {}", e);
                            }
                        }
                    }
                    Ok(false) => {
                        debug!("User {} has not completed yet", user_id);
                    }
                    Err(e) => {
                        error!("Failed to verify completion for user {}: {}", user_id, e);
                    }
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
        GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_PRESENCES
            | GatewayIntents::GUILD_MEMBERS,
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
