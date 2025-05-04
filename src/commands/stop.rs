use crate::QueueKey;
use crate::errors::errors::BeatError;
use serenity::all::{Interaction, MessageId};
use serenity::builder::CreateCommand;
use serenity::client::Context;

pub fn register() -> CreateCommand {
    CreateCommand::new("stop").description("Stops and disconnects Beat")
}

pub async fn run(ctx: &Context, interaction: &Interaction) -> Result<(), BeatError> {
    if let Some((guild_id, channel_id)) = if let Interaction::Command(command) = interaction {
        command.defer_ephemeral(ctx).await?;
        Some((
            command.guild_id.ok_or(BeatError::NoGuild)?,
            command.channel_id,
        ))
    } else if let Interaction::Component(component) = interaction {
        component.defer_ephemeral(ctx).await?;
        component.delete_response(ctx).await?;
        Some((
            component.guild_id.ok_or(BeatError::NoGuild)?,
            component.channel_id,
        ))
    } else {
        None
    } {
        let queue_lock = {
            let guard = ctx.data.write().await;
            guard.get::<QueueKey>().ok_or(BeatError::NoQueues)?.clone()
        };

        let mut maybe_queue = queue_lock.write().await;

        // Delete the queue message
        if let Some(queue) = maybe_queue.get(&guild_id) {
            if let Some(message_id) = queue.message_id {
                ctx.http
                    .delete_message(channel_id, MessageId::from(message_id), None)
                    .await
                    .unwrap_or_default();
            }
        }

        // Delete Beat data for the guild
        maybe_queue.remove(&guild_id);

        // Disconnect and clear Songbird for the guild
        let manager = songbird::get(ctx)
            .await
            .ok_or(BeatError::NoSongbird)?
            .clone();
        manager.remove(guild_id).await?;
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }

    Ok(())
}
