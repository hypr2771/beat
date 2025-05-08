use crate::errors::errors::BeatError;
use serde_json::json;
use serenity::all::{Context, CreateCommand, Interaction};
use std::fs;
use std::fs::create_dir_all;

pub fn register() -> CreateCommand {
    CreateCommand::new("list").description("Lists the available playlists")
}

pub async fn run(ctx: &Context, interaction: &Interaction) -> Result<(), BeatError> {
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
        let dir_name = format!("./{}", guild_id);
        create_dir_all(dir_name.clone())?;

        let mut collected: String = String::from("");
        for entry in fs::read_dir(dir_name)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                collected = format!(
                    "{}\n- {}",
                    collected,
                    path.file_name().unwrap().to_str().unwrap().split(".").nth(0).unwrap()
                );
            }
        }

        let body = format!("{}{}", collected, if collected.len() == 0 { "_None_" } else { "" });

        let json = json!({"embeds": [
          {
            "title": format!("**Available playlists**"),
            "description": body
          }
        ]});

        ctx.http.send_message(channel_id, vec![], &json).await?;
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }
    Ok(())
}
