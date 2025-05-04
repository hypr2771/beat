use crate::Queue;
use serde_json::json;
use serenity::json::Value;
use std::cmp::{max, min};
use std::time::Duration;

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
