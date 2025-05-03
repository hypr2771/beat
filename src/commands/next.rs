use crate::errors::errors::BeatError;
use serenity::all::Interaction;
use serenity::builder::CreateCommand;
use serenity::client::Context;

pub fn register() -> CreateCommand {
    CreateCommand::new("next").description("Jumps to the next song")
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
        // Get Songbird
        let manager = songbird::get(ctx)
            .await
            .ok_or(BeatError::NoSongbird)?
            .clone();
        let handler_lock = manager.get(guild_id).ok_or(BeatError::NoManager)?;

        // Skips the track
        {
            let guard = handler_lock.lock().await;
            let x = guard.queue();
            x.skip()?;
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }

    Ok(())
}
