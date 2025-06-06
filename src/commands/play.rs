use crate::errors::errors::BeatError;
use crate::messages::messages::to_embed;
use crate::{HttpKey, Queue, QueueKey};
use reqwest::Client;
use serenity::all::{ChannelId, GuildId, Interaction};
use serenity::async_trait;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::client::Context;
use serenity::http::Http;
use serenity::json::Value;
use serenity::model::application::{CommandOptionType, ResolvedOption, ResolvedValue};
use serenity::prelude::TypeMap;
use songbird::input::{Compose, YoutubeDl};
use songbird::{Call, Event, EventContext, EventHandler, Songbird, SongbirdKey, TrackEvent};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use url::Url;

struct TrackErrorNotifier;

struct OnTrackEnd {
    guild_id: GuildId,
    channel_id: ChannelId,
    data: Arc<tokio::sync::RwLock<TypeMap>>,
    http: Arc<Http>,
}

struct OnTrackStart {
    guild_id: GuildId,
    channel_id: ChannelId,
    data: Arc<tokio::sync::RwLock<TypeMap>>,
    http: Arc<Http>,
}

pub fn register() -> CreateCommand {
    CreateCommand::new("play")
        .description("Play a track or add it to the queue")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "track",
                "The track to play or add to the queue",
            )
            .required(true),
        )
}

pub async fn run(
    ctx: &Context,
    interaction: &Interaction,
    options: &[ResolvedOption<'_>],
) -> Result<(), BeatError> {
    let mut should_delete = true;

    if let Some(ResolvedOption {
        value: ResolvedValue::String(url),
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
            } else if let Interaction::Component(component) = interaction {
                Some((
                    component.guild_id.ok_or(BeatError::NoGuild)?,
                    component.channel_id,
                    component.user.id,
                ))
            } else {
                None
            }
        {
            let url = String::from(*url);

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
                        .reset_for_play();
                }
            };

            let do_search = !url.starts_with("http");
            let playlist = url.contains("list=");

            let http_client = {
                let data = ctx.data.read().await;
                data.get::<HttpKey>().cloned().ok_or(BeatError::NoHttp)?
            };

            if playlist {
                let parsed = Url::parse(url.as_str())?;
                let index = parsed
                    .query_pairs()
                    .filter(|(key, _)| key == "index")
                    .last()
                    .map(|(_, value)| value.parse::<usize>().unwrap_or(1))
                    .unwrap_or(1);

                let playlist = ytdl_playlist(url.clone())
                    .await
                    .ok_or(BeatError::Other("Empty playlist"))?
                    .split_off(index - 1);

                for i in 0..playlist.len() {
                    should_delete = insert_track(
                        ctx,
                        interaction,
                        guild_id,
                        channel_id,
                        playlist[i].clone(),
                        manager.get(guild_id).ok_or(BeatError::NoManager)?,
                        false,
                        i == 0,
                        http_client.clone(),
                    )
                    .await
                    // Ignore error in a playlist, keep loading next ones
                    .unwrap_or(true);
                }
            } else {
                should_delete = insert_track(
                    ctx,
                    interaction,
                    guild_id,
                    channel_id,
                    url,
                    manager.get(guild_id).ok_or(BeatError::NoManager)?,
                    do_search,
                    true,
                    http_client.clone(),
                )
                .await
                .unwrap_or(true);
            }
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        if should_delete {
            command.delete_response(ctx).await?;
        }
    }

    Ok(())
}

pub async fn connect_and_handle(
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    to_connect: ChannelId,
    manager: &Arc<Songbird>,
) -> Result<(), BeatError> {
    let lock = manager.join(guild_id, to_connect).await?;
    let copy = lock.clone();
    let mut handler = copy.lock().await;
    handler.remove_all_global_events();
    handler.add_global_event(TrackEvent::Error.into(), TrackErrorNotifier);
    handler.add_global_event(
        TrackEvent::End.into(),
        OnTrackEnd {
            guild_id,
            channel_id: channel_id,
            data: ctx.clone().data,
            http: ctx.clone().http,
        },
    );
    handler.add_global_event(
        TrackEvent::Play.into(),
        OnTrackStart {
            guild_id,
            channel_id: channel_id,
            data: ctx.clone().data,
            http: ctx.clone().http,
        },
    );
    Ok(())
}

pub async fn insert_track(
    ctx: &Context,
    interaction: &Interaction,
    guild_id: GuildId,
    channel_id: ChannelId,
    url: String,
    handler_lock: Arc<Mutex<Call>>,
    do_search: bool,
    should_delete: bool,
    http_client: Client,
) -> Result<bool, BeatError> {
    // let yt_dlp_args = env::var("YT_DLP_ARGS")
    //     .unwrap()
    //     .split(" ")
    //     .map(|str| String::from(str))
    //     .collect::<Vec<String>>();

    let queue_lock = {
        let guard = ctx.data.read().await;
        guard.get::<QueueKey>().ok_or(BeatError::NoQueues)?.clone()
    };

    let mut maybe_queue = queue_lock.write().await;

    if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
        if !existing_queue.stopping {
            let src = if do_search {
                YoutubeDl::new_search(http_client, url.clone()).user_args(vec![
                    "-4".into(),
                    "-f".into(),
                    "\"webm[abr>0]/bestaudio/best\"".into(),
                    "-R".into(),
                    "infinite".into(),
//                    "--extractor-args".into(),
//                    "youtube:player-client=tv".into(),
                ])
            } else {
                YoutubeDl::new(http_client, url.clone()).user_args(vec![
                    "-4".into(),
                    "-f".into(),
                    "\"webm[abr>0]/bestaudio/best\"".into(),
                    "-R".into(),
                    "infinite".into(),
//                    "--extractor-args".into(),
//                    "youtube:player-client=tv".into(),
                ])
            };

            let metadata = src.clone().aux_metadata().await?.clone();

            if let Some(message_id) = existing_queue.message_id {
                existing_queue.queue.push(metadata);

                let _ = ctx
                    .http
                    .edit_message(channel_id, message_id, &to_embed(&existing_queue), vec![])
                    .await?;
            } else {
                existing_queue.queue.push(metadata);

                let message = ctx
                    .http
                    .send_message(channel_id, vec![], &to_embed(&existing_queue))
                    .await?;

                existing_queue.message_id = Some(message.id);
            }

            // Attach an event handler to see notifications of all track errors.
            let mut handler = handler_lock.lock().await;

            handler.enqueue_with_preload(src.into(), Duration::from_secs(10).into());

            if should_delete {
                if let Interaction::Command(command) = interaction {
                    // Delete ephemeral response
                    command.delete_response(ctx).await?;
                    return Ok(false);
                }

                return Ok(should_delete);
            }

            return Ok(should_delete);
        }

        return Err(BeatError::Stopping);
    }

    Ok(should_delete)
}

#[async_trait]
impl EventHandler for TrackErrorNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                println!(
                    "Track {:?} encountered an error: {:?}",
                    handle.uuid(),
                    state.playing
                );
            }
        }

        None
    }
}

#[async_trait]
impl EventHandler for OnTrackEnd {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(_) = ctx {
            let queue_lock = {
                let guard = self.data.read().await;
                guard.get::<QueueKey>().unwrap().clone()
            };

            let mut maybe_queue = queue_lock.write().await;

            if let Some(existing_queue) = maybe_queue.get_mut(&self.guild_id) {
                println!("Queue exists: {:?}", existing_queue);

                if existing_queue.is_last() {
                    println!("Was the last song, should leave voice channel");
                    if let Some(message_id) = existing_queue.message_id {
                        println!("Emptying tracklist for guild {}", self.guild_id);

                        println!(
                            "Deleting message at channel {} with ID {}",
                            self.channel_id, message_id
                        );

                        self.http
                            .delete_message(self.channel_id, message_id, Some("Tracklist ended"))
                            .await
                            .unwrap_or_default();

                        println!("Tracklist removed for guild {:?}", self.guild_id);

                        // Disconnect and clear Songbird for the guild
                        let manager_lock = {
                            let guard = self.data.read().await;
                            guard.get::<SongbirdKey>().unwrap().clone()
                        };

                        // Disconnect and clear Songbird for the guild
                        manager_lock
                            .get(self.guild_id)
                            .unwrap()
                            .lock()
                            .await
                            .queue()
                            .stop();
                        manager_lock.remove(self.guild_id).await.unwrap();

                        println!("Tracklist removed for guild {:?}", self.guild_id);

                        // Remove local data
                        maybe_queue
                            .get_mut(&self.guild_id)
                            .ok_or(BeatError::NoQueue)
                            .unwrap_or(&mut Queue::default())
                            .reset_for_play();
                    }
                } else {
                    println!("Not the last sound, should increment playing index");

                    if let Some(existing_queue) = maybe_queue.get_mut(&self.guild_id) {
                        if !existing_queue.did_skip {
                            existing_queue.playing_index = existing_queue.playing_index + 1;
                        }
                        existing_queue.did_skip = false;
                        existing_queue.repeat = false;
                        existing_queue.pause = false;

                        let manager_lock = {
                            let guard = self.data.read().await;
                            guard.get::<SongbirdKey>().unwrap().clone()
                        };

                        manager_lock
                            .get(self.guild_id)
                            .unwrap()
                            .lock()
                            .await
                            .queue()
                            .resume()
                            .unwrap();

                        println!("Playlist index incremented: {:?}", existing_queue);
                    }
                }
            }
        }
        None
    }
}

#[async_trait]
impl EventHandler for OnTrackStart {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(_) = ctx {
            println!("New track playing, updating the queue");

            let queue_lock = {
                let guard = self.data.write().await;
                guard.get::<QueueKey>().unwrap().clone()
            };

            let mut maybe_queue = queue_lock.write().await;

            if let Some(existing_queue) = maybe_queue.get_mut(&self.guild_id) {
                println!("Queue exists: {:?}", existing_queue);

                if let Some(message_id) = existing_queue.message_id {
                    println!(
                        "Message exists: {:?} in channel {}",
                        message_id, self.channel_id
                    );

                    self.http
                        .edit_message(
                            self.channel_id,
                            message_id,
                            &to_embed(existing_queue),
                            vec![],
                        )
                        .await
                        .unwrap_or_default();

                    println!(
                        "Edited message: {:?} in channel {}",
                        message_id, self.channel_id
                    )
                }
            }
        }
        None
    }
}

pub async fn ytdl_playlist(uri: String) -> Option<Vec<String>> {
    let args = vec![uri.as_str(), "-4", "--flat-playlist", "-j"];

    let mut child = Command::new("yt-dlp")
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let Some(stdout) = &mut child.stdout else {
        return None;
    };

    let reader = BufReader::new(stdout);

    let lines = reader.lines().map_while(Result::ok).map(|line| {
        let entry: Value = serde_json::from_str(&line).unwrap();
        entry
            .get("webpage_url")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    });

    Some(lines.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_works() {
        let src = ytdl_playlist(
            "https://www.youtube.com/playlist?list=PLdrfcI54NmaXlCgSsv7VFsYLUJnhSYgVc".into(),
        )
        .await
        .unwrap();

        println!("{:#?}", src);
    }
}
