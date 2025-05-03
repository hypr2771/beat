use crate::errors::errors::BeatError;
use crate::{HttpKey, Queue, QueueKey};
use reqwest::Client;
use serenity::all::{ChannelId, GuildId, Interaction};
use serenity::async_trait;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::client::Context;
use serenity::http::Http;
use serenity::json::{Value, json};
use serenity::model::application::{CommandOptionType, ResolvedOption, ResolvedValue};
use serenity::prelude::TypeMap;
use songbird::input::{Compose, YoutubeDl};
use songbird::{Call, Event, EventContext, EventHandler, SongbirdKey, TrackEvent};
use std::cmp::{max, min};
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

                for url in ytdl_playlist(url.clone())
                    .await
                    .ok_or(BeatError::Other("Empty playlist"))?
                    .split_off(index - 1)
                {
                    insert_track(
                        ctx,
                        guild_id,
                        channel_id,
                        url,
                        manager.get(guild_id).ok_or(BeatError::NoManager)?,
                        false,
                        http_client.clone(),
                    )
                    .await
                    // Ignore error in a playlist, keep loading next ones
                    .unwrap_or(());
                }
            } else {
                insert_track(
                    ctx,
                    guild_id,
                    channel_id,
                    url,
                    manager.get(guild_id).ok_or(BeatError::NoManager)?,
                    do_search,
                    http_client.clone(),
                )
                .await?;
            }
        }
        if let Interaction::Command(command) = interaction {
            // Delete ephemeral response
            command.delete_response(ctx).await?;
        }
    }

    Ok(())
}

async fn insert_track(
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    url: String,
    handler_lock: Arc<Mutex<Call>>,
    do_search: bool,
    http_client: Client,
) -> Result<(), BeatError> {
    let src = if do_search {
        YoutubeDl::new_ytdl_like("yt-dlp", http_client, url.clone()).user_args(vec![
            "-j".into(),
            "-4".into(),
            "-q".into(),
            "--no-simulate".into(),
            "-f".into(),
            "\"webm[abr>0]/bestaudio/best\"".into(),
            "-R".into(),
            "infinite".into(),
            "--ignore-config".into(),
            "--no-warnings".into(),
            "--extractor-args".into(),
            "youtube:player-client=tv".into(),
            "--cache-dir".into(),
            "./yt-dlp-cache".into(),
        ])
    } else {
        YoutubeDl::new_ytdl_like("yt-dlp", http_client, url.clone()).user_args(vec![
            "-4".into(),
            "-f".into(),
            "\"webm[abr>0]/bestaudio/best\"".into(),
            "-R".into(),
            "infinite".into(),
            "--extractor-args".into(),
            "youtube:player-client=tv".into(),
        ])
    };

    let metadata = src.clone().aux_metadata().await?.clone();

    let queue_lock = {
        let guard = ctx.data.read().await;
        guard.get::<QueueKey>().ok_or(BeatError::NoQueues)?.clone()
    };

    let mut maybe_queue = queue_lock.write().await;

    if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
        if let Some(message_id) = existing_queue.message_id {
            existing_queue.queue.push(metadata);

            let _ = ctx
                .http
                .edit_message(channel_id, message_id, &to_embed(&existing_queue), vec![])
                .await?;
        } else {
            let queue = Queue {
                did_skip: false,
                repeat: false,
                pause: false,
                playing_index: 0,
                message_id: None,
                queue: vec![metadata],
            };

            let message = ctx
                .http
                .send_message(channel_id, vec![], &to_embed(&queue))
                .await?;

            maybe_queue.insert(
                guild_id,
                Queue {
                    message_id: Some(message.id),
                    ..queue
                },
            );
        }
    } else {
        let queue = Queue {
            did_skip: false,
            repeat: false,
            pause: false,
            playing_index: 0,
            message_id: None,
            queue: vec![metadata],
        };

        let message = ctx
            .http
            .send_message(channel_id, vec![], &to_embed(&queue))
            .await?;

        maybe_queue.insert(
            guild_id,
            Queue {
                message_id: Some(message.id),
                ..queue
            },
        );
    }

    // Attach an event handler to see notifications of all track errors.
    let mut handler = handler_lock.lock().await;

    handler.enqueue_with_preload(src.into(), Duration::from_secs(1).into());

    Ok(())
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
                            .await;

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
                        maybe_queue.remove(&self.guild_id);
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

pub(crate) fn to_embed(queue: &Queue) -> Value {
    let whole_queue = queue.queue.clone();
    let loop_mode = if queue.repeat { 3 } else { 2 };
    let (pause_mode, pause_button) = if queue.pause {
        (3, "▶️")
    } else {
        (1, "⏸️")
    };
    let current_track = whole_queue.get(queue.playing_index).unwrap().clone();
    let title = current_track.title.unwrap();
    let artist = current_track.artist.unwrap();
    let duration = readable_duration(current_track.duration.unwrap());
    let link = current_track.source_url.unwrap();
    let thumbnail = current_track.thumbnail.unwrap();
    let (played, to_play) = whole_queue.split_at(queue.playing_index);

    let time_to_play = readable_duration(
        to_play
            .iter()
            .map(|played| played.duration.unwrap())
            .fold(Duration::from_secs(0), |acc, duration| acc + duration),
    );
    let time_elapsed = played
        .iter()
        .map(|played| played.duration.unwrap())
        .fold(Duration::from_secs(0), |acc, duration| acc + duration);
    let total_time = whole_queue
        .iter()
        .map(|played| played.duration.unwrap())
        .fold(Duration::from_secs(0), |acc, duration| acc + duration);

    let elapsed_over_total = readable_elapsed(time_elapsed, total_time);

    let short_queue: Vec<String> = queue
        .queue
        .iter()
        .map(|track| {
            format!(
                "{} ({}) - {}",
                track.title.clone().unwrap(),
                readable_duration(track.duration.unwrap()),
                track.artist.clone().unwrap()
            )
        })
        .collect();

    let short = get_short_playlist(queue.playing_index, &short_queue, 2).join("\n");

    let json = json!({
      "embeds": [
        {
          "author": {
            "name": "🔊 Now playing"
          },
          "title": format!("**{} ({}) - {}**", title, duration, artist),
          "description": short,
          "url": link,
          "thumbnail": {
            "url": thumbnail,
          },
          "footer": {
            "text": format!("{} of {} tracks - {} ({} left)", queue.playing_index + 1, whole_queue.len(), elapsed_over_total, time_to_play),
          }
        }
      ],
      "components": [
        {
          "type": 1,
          "components": [
            {
              "type": 2,
              "emoji": {
                "name": "⏮"
              },
              "style": 2,
              "custom_id": "prev"
            },
            {
              "type": 2,
              "emoji": {
                "name": "⏹"
              },
              "style": 4,
              "custom_id": "stop"
            },
            {
              "type": 2,
              "emoji": {
                "name": pause_button
              },
              "style": pause_mode,
              "custom_id": "pause"
            },
            {
              "type": 2,
              "emoji": {
                "name": "⏭"
              },
              "style": 2,
              "custom_id": "next"
            },
            {
              "type": 2,
              "emoji": {
                "name": "🔁"
              },
              "style": loop_mode,
              "custom_id": "loop"
            }
          ]
        }
      ]
    });

    json
}

fn readable_duration(duration: Duration) -> String {
    let seconds = duration.as_secs() % 60;
    let minutes = (duration.as_secs() / 60) % 60;
    let hours = (duration.as_secs() / 60) / 60;

    let mut parts = Vec::new();

    if hours > 0 {
        parts.push(format!("{:0>2}", hours));
    }
    if minutes > 0 {
        parts.push(format!("{:0>2}", minutes));
    }
    parts.push(format!("{:0>2}", seconds));

    parts.join(":")
}

fn readable_elapsed(elapsed: Duration, total: Duration) -> String {
    let seconds_total = total.as_secs() % 60;
    let minutes_total = (total.as_secs() / 60) % 60;
    let hours_total = (total.as_secs() / 60) / 60;
    let seconds_elapsed = elapsed.as_secs() % 60;
    let minutes_elapsed = (elapsed.as_secs() / 60) % 60;
    let hours_elapsed = (elapsed.as_secs() / 60) / 60;

    let mut parts_total = Vec::new();
    let mut parts_elapsed = Vec::new();

    if hours_total > 0 {
        parts_total.push(format!("{:0>2}", hours_total));
        parts_elapsed.push(format!("{:0>2}", hours_elapsed));
    }
    if minutes_total > 0 || hours_total > 0 {
        parts_total.push(format!("{:0>2}", minutes_total));
        parts_elapsed.push(format!("{:0>2}", minutes_elapsed));
    }
    parts_total.push(format!("{:0>2}", seconds_total));
    parts_elapsed.push(format!("{:0>2}", seconds_elapsed));

    format!("{}/{}", parts_elapsed.join(":"), parts_total.join(":"))
}

pub fn get_short_playlist<'a>(index: usize, data: &'a [String], split: usize) -> Vec<String> {
    let len = data.len();
    if len == 0 {
        return vec![];
    }

    let mut result: Vec<String> = Vec::new();

    // If there is less to display than the minimum, display all
    if data.len() <= split * 2 + 1 {
        for i in 0..data.len() {
            if i == index {
                result.push(format!("▶️ {}. {}", i + 1, data[i]));
            } else {
                result.push(format!("- {}. {}", i + 1, data[i]));
            }
        }

        return result;
    }

    // [ - - - b = 2 - i - a = 2 - - - ]
    // [ i - - - a'=b+a= 4 - - - - - - ]

    let mut before = split;
    let mut after = split;

    // Readjust before
    if (index.checked_sub(before)).is_none() {
        after = before - index + after;
        before = index;
    }

    // Readjust after
    if (index + after) >= data.len() - 1 {
        before = after - (data.len() - 1 - index) + before;
        after = data.len() - 1 - index;
    }

    // Pick elements before the index
    if index > 0 && before > 0 {
        result.push(format!("- {}. {}", 1, &data[0]));

        for i in max(index - before, 1)..index {
            result.push(format!("- {}. {}", i + 1, &data[i]));
        }
    }

    // Pick the element at the index if not 0
    result.push(format!("▶️ {}. {}", index + 1, &data[index]));

    // Pick elements before the index
    if index < data.len() - 1 && after < data.len() - 1 {
        for i in index + 1..min(index + after + 1, data.len() - 1) {
            result.push(format!("- {}. {}", i + 1, &data[i]));
        }

        result.push(format!("- {}. {}", data.len(), &data[data.len() - 1]));
    }

    result
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
            "https://www.youtube.com/watch?v=BcoPKWzLjrE&list=RDEMatZpVWj6E8vgE-V4-cj6lQ&index=2"
                .into(),
        )
        .await
        .unwrap();

        println!("{:#?}", src);
    }
}
