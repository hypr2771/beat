use crate::commands::play::{connect_and_handle, insert_track};
use crate::errors::errors::BeatError;
use crate::{HttpKey, QueueKey};
use serenity::all::{
    CommandOptionType, Context, CreateCommand, CreateCommandOption, Interaction, ResolvedOption,
    ResolvedValue,
};
use songbird::SongbirdKey;
use std::fs;
use std::fs::create_dir_all;

pub fn register() -> CreateCommand {
    CreateCommand::new("load")
        .description("Loads the previously saved playlist")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "name",
                "The name of the playlist",
            )
            .required(true)
            .max_length(100)
            .min_length(1),
        )
}

pub async fn run(
    ctx: &Context,
    interaction: &Interaction,
    options: &[ResolvedOption<'_>],
) -> Result<(), BeatError> {
    let mut should_delete = true;

    if let Some(ResolvedOption {
        value: ResolvedValue::String(name),
        ..
    }) = options.first()
    {
        if let Some((guild_id, channel_id, user_id)) =
            if let Interaction::Command(command) = interaction {
                command.defer_ephemeral(ctx).await?;
                Some((
                    command.guild_id.ok_or(BeatError::NoGuild)?,
                    command.channel_id,
                    command.user.id,
                ))
            } else {
                None
            }
        {
            let to_connect = ctx
                .cache
                .guild(guild_id)
                .ok_or(BeatError::Other("Beat has no information about that guild"))?
                .voice_states
                .get(&user_id)
                .and_then(|voice_state| voice_state.channel_id)
                .ok_or(BeatError::Other("User connected to a channel"))?;

            let manager = songbird::get(ctx).await.ok_or(BeatError::NoSongbird)?;

            if let None = manager.get(guild_id) {
                connect_and_handle(ctx, guild_id, channel_id, to_connect, &manager).await?;

                let queue_lock = {
                    let guard = ctx.data.read().await;
                    guard.get::<QueueKey>().unwrap().clone()
                };

                let mut maybe_queue = queue_lock.write().await;

                if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
                    println!(
                        "Queue already exists while joining channel: {:?}, clearing previous state",
                        existing_queue
                    );

                    if let Some(message_id) = existing_queue.message_id {
                        println!(
                            "Deleting message at channel {} with ID {}",
                            channel_id, message_id
                        );

                        ctx.http
                            .delete_message(channel_id, message_id, Some("Dangling message"))
                            .await
                            .unwrap_or_default();
                    }

                    println!("Removing tracklist for guild {:?}", guild_id);

                    // Disconnect and clear Songbird for the guild
                    let manager_lock = {
                        let guard = ctx.data.read().await;
                        guard.get::<SongbirdKey>().unwrap().clone()
                    };

                    // Disconnect and clear Songbird for the guild
                    manager_lock
                        .get(guild_id)
                        .unwrap()
                        .lock()
                        .await
                        .queue()
                        .stop();

                    println!("Tracklist removed for guild {:?}", guild_id);

                    // Remove local data
                    maybe_queue
                        .get_mut(&guild_id)
                        .ok_or(BeatError::NoQueue)?
                        .reset();

                    maybe_queue
                        .get_mut(&guild_id)
                        .ok_or(BeatError::NoQueue)?
                        .stopping = false
                }
            };

            let http_client = {
                let data = ctx.data.read().await;
                data.get::<HttpKey>().cloned().ok_or(BeatError::NoHttp)?
            };

            let dir_name = format!("./{}", guild_id);
            create_dir_all(dir_name.clone())?;
            let file_name = format!("{}/{}.playlist", dir_name, name);
            let content = fs::read_to_string(file_name)?;
            let urls = content.split("\n").collect::<Vec<&str>>();

            for i in 0..urls.len() {
                should_delete = insert_track(
                    ctx,
                    interaction,
                    guild_id,
                    channel_id,
                    String::from(urls[i]),
                    manager.get(guild_id).ok_or(BeatError::NoManager)?,
                    false,
                    i == 0,
                    http_client.clone(),
                )
                .await
                // Ignore error in a playlist, keep loading next ones
                .unwrap_or(false);
            }
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        if (should_delete) {
            command.delete_response(ctx).await?;
        }
    }
    Ok(())
}
