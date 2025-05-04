use crate::errors::errors::BeatError;
use crate::{HttpKey, QueueKey};
use serenity::all::Interaction;
use serenity::builder::CreateCommand;
use serenity::client::Context;
use songbird::input::YoutubeDl;
use std::env;
use std::time::Duration;

pub fn register() -> CreateCommand {
    CreateCommand::new("prev").description("Plays the previous song")
}

pub async fn run(ctx: &Context, interaction: &Interaction) -> Result<(), BeatError> {
    if let Some(guild_id) = if let Interaction::Command(command) = interaction {
        command.defer_ephemeral(ctx).await?;
        Some(command.guild_id.ok_or(BeatError::NoGuild)?)
    } else if let Interaction::Component(component) = interaction {
        component.defer_ephemeral(ctx).await?;
        component.delete_response(ctx).await?;
        Some(component.guild_id.ok_or(BeatError::NoGuild)?)
    } else {
        None
    } {
        let queue_lock = {
            let guard = ctx.data.read().await;
            guard.get::<QueueKey>().ok_or(BeatError::NoQueues)?.clone()
        };

        let mut maybe_queue = queue_lock.write().await;

        if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
            let yt_dlp_args = env::var("YT_DLP_ARGS")
                .unwrap()
                .split(" ")
                .map(|str| String::from(str))
                .collect::<Vec<String>>();

            if existing_queue.playing_index >= 1 {
                let current_metadata = existing_queue
                    .queue
                    .get(existing_queue.playing_index)
                    .ok_or(BeatError::NoCurrentTrack)?;

                existing_queue.playing_index = existing_queue.playing_index.saturating_sub(1);

                let previous_metadata = existing_queue
                    .queue
                    .get(existing_queue.playing_index)
                    .ok_or(BeatError::NoPreviousTrack)?;

                let manager = songbird::get(ctx).await.ok_or(BeatError::NoSongbird)?;
                let handler_lock = manager.get(guild_id).ok_or(BeatError::NoManager)?;

                let http_client = {
                    let data = ctx.data.read().await;
                    data.get::<HttpKey>().cloned().ok_or(BeatError::NoHttp)?
                };

                let src_previous = {
                    YoutubeDl::new(
                        http_client.clone(),
                        previous_metadata
                            .clone()
                            .source_url
                            .ok_or(BeatError::NoPreviousSourceUrl)?,
                    )
                    .user_args(yt_dlp_args.clone())
                };

                let src = {
                    YoutubeDl::new(
                        http_client.clone(),
                        current_metadata
                            .clone()
                            .source_url
                            .ok_or(BeatError::NoCurrentSourceUrl)?,
                    )
                    .user_args(yt_dlp_args)
                };

                // Skips the track
                {
                    let mut handle = handler_lock.lock().await;

                    // Place the previous track at the end
                    handle
                        .enqueue_with_preload(src_previous.into(), Duration::from_secs(15).into());
                    // Place the current track at the end
                    handle.enqueue_with_preload(src.into(), Duration::from_secs(15).into());

                    handle.queue().modify_queue(|queue| {
                        // Get the current track
                        let current = queue.pop_back().expect("Just pushed, can not fail");
                        // Get the previous track
                        let previous = queue.pop_back().expect("Just pushed, can not fail");

                        // Put the current at the beginning
                        queue.insert(1, current);
                        // Put the previous before the current one
                        queue.insert(1, previous);
                    });

                    // Skips the current track which is outdated, to play the previous one
                    if existing_queue.playing_index.checked_sub(1).is_none() {
                        existing_queue.playing_index = 0;
                        existing_queue.did_skip = true;
                    } else {
                        existing_queue.playing_index = existing_queue.playing_index - 1;
                    }
                    handle.queue().skip().expect("Just pushed, can not fail");
                }
            } else {
                let current_metadata = existing_queue
                    .queue
                    .get(existing_queue.playing_index)
                    .ok_or(BeatError::NoCurrentTrack)?;

                let http_client = {
                    let data = ctx.data.read().await;
                    data.get::<HttpKey>().cloned().ok_or(BeatError::NoHttp)?
                };

                let src = {
                    YoutubeDl::new(
                        http_client.clone(),
                        current_metadata
                            .clone()
                            .source_url
                            .ok_or(BeatError::NoCurrentSourceUrl)?,
                    )
                    .user_args(yt_dlp_args)
                };

                let manager = songbird::get(ctx).await.ok_or(BeatError::NoSongbird)?;
                let handler_lock = manager.get(guild_id).ok_or(BeatError::NoManager)?;

                {
                    let mut handle = handler_lock.lock().await;

                    // Place the current track at the end
                    handle.enqueue_with_preload(src.into(), Duration::from_secs(15).into());

                    handle.queue().modify_queue(|queue| {
                        // Get the current track
                        let current = queue.pop_back().expect("Just pushed, can not fail");

                        // Put the current at the beginning
                        queue.insert(1, current);
                    });

                    // Skips the current track which is outdated, to play the previous one
                    if existing_queue.playing_index.checked_sub(1).is_none() {
                        existing_queue.playing_index = 0;
                        existing_queue.did_skip = true;
                    } else {
                        existing_queue.playing_index = existing_queue.playing_index - 1;
                    }
                    handle.queue().skip().expect("Just pushed, can not fail");
                }
            }
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }

    Ok(())
}
