//! Requires the "client", "standard_framework", and "voice" features be enabled in your
//! Cargo.toml, like so:
//!
//! ```toml
//! [dependencies.serenity]
//! git = "https://github.com/serenity-rs/serenity.git"
//! features = ["client", "standard_framework", "voice"]
//! ```

mod commands;
mod errors;

// This trait adds the `register_songbird` and `register_songbird_with` methods
// to the client builder below, making it easy to install this voice client.
// The voice client can be retrieved in any command using `songbird::get(ctx).await`.
use songbird::SerenityInit;
use std::collections::HashMap;
use std::env;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;

// YtDl requests need an HTTP client to operate -- we'll create and store our own.
use reqwest::Client as HttpClient;

// Import the `Context` to handle commands.
use serenity::client::Context;

use crate::errors::errors::BeatError;
use serenity::all::{Command, GuildId, Interaction, MessageId};
use serenity::{
    async_trait,
    client::{Client, EventHandler},
    model::gateway::Ready,
    prelude::{GatewayIntents, TypeMapKey},
};
use songbird::input::AuxMetadata;
use tokio::sync::RwLock;

struct HttpKey;

impl TypeMapKey for HttpKey {
    type Value = HttpClient;
}

#[derive(Debug)]
struct Queue {
    did_skip: bool,
    pause: bool,
    repeat: bool,
    playing_index: usize,
    message_id: Option<MessageId>,
    queue: Vec<AuxMetadata>,
}

impl Queue {
    pub fn is_last(&self) -> bool {
        self.playing_index + 1 >= self.queue.len()
    }
}

struct QueueKey;

impl TypeMapKey for QueueKey {
    type Value = Arc<RwLock<HashMap<GuildId, Queue>>>;
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let guild_command = vec![
            Command::create_global_command(&ctx.http, commands::play::register()).await,
            Command::create_global_command(&ctx.http, commands::pause::register()).await,
            Command::create_global_command(&ctx.http, commands::stop::register()).await,
            Command::create_global_command(&ctx.http, commands::next::register()).await,
            Command::create_global_command(&ctx.http, commands::prev::register()).await,
            Command::create_global_command(&ctx.http, commands::repeat::register()).await,
            Command::create_global_command(&ctx.http, commands::clean::register()).await,
        ];

        println!("I created the following global slash command: {guild_command:#?}");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let interaction_clone = interaction.clone();

        if let Interaction::Command(command) = interaction_clone {
            match command.data.name.as_str() {
                "play" => commands::play::run(&ctx, &interaction, &command.data.options()).await,
                "pause" => commands::pause::run(&ctx, &interaction).await,
                "stop" => commands::stop::run(&ctx, &interaction).await,
                "next" => commands::next::run(&ctx, &interaction).await,
                "prev" => commands::prev::run(&ctx, &interaction).await,
                "loop" => commands::repeat::run(&ctx, &interaction).await,
                "clean" => commands::clean::run(&ctx, &interaction).await,
                _ => Err(BeatError::NoValidCommand),
            }
            .map_err(|err| {
                eprintln!("{:?}", err);
                ()
            }).unwrap_or(());
        } else if let Interaction::Component(command) = interaction_clone {
            match command.data.custom_id.as_str() {
                "pause" => commands::pause::run(&ctx, &interaction).await,
                "stop" => commands::stop::run(&ctx, &interaction).await,
                "next" => commands::next::run(&ctx, &interaction).await,
                "prev" => commands::prev::run(&ctx, &interaction).await,
                "loop" => commands::repeat::run(&ctx, &interaction).await,
                _ => Err(BeatError::NoValidCommand),
            }
            .map_err(|err| {
                eprintln!("{:?}", err);
                ()
            }).unwrap_or(());
        } else {
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let intents = GatewayIntents::non_privileged() | GatewayIntents::GUILD_MESSAGES;

    // Build our client.
    let mut client = Client::builder(token, intents)
        .event_handler(Handler)
        .type_map_insert::<HttpKey>(HttpClient::builder().local_address(IpAddr::from_str("0.0.0.0").unwrap()).build().unwrap())
        .type_map_insert::<QueueKey>(Arc::new(RwLock::new(HashMap::new())))
        .register_songbird()
        .await
        .expect("Error creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform exponential backoff until
    // it reconnects.
    tokio::spawn(async move {
        let _ = client
            .start()
            .await
            .map_err(|why| println!("Client ended: {:?}", why));
    });

    let _signal_err = tokio::signal::ctrl_c().await;
    println!("Received Ctrl-C, shutting down.");
}
