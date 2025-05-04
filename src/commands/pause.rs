use crate::QueueKey;
use crate::errors::errors::BeatError;
use crate::messages::messages::to_embed;
use serenity::all::Interaction;
use serenity::builder::CreateCommand;
use serenity::client::Context;

pub fn register() -> CreateCommand {
    CreateCommand::new("pause").description("Toggle pause")
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
        if let Some(queue) = maybe_queue.get_mut(&guild_id) {
            queue.pause = !queue.pause;

            // Get Songbird
            let manager = songbird::get(ctx)
                .await
                .ok_or(BeatError::NoSongbird)?
                .clone();
            let handler_lock = manager.get(guild_id).ok_or(BeatError::NoManager)?;

            // Enable loop
            if queue.pause {
                handler_lock.lock().await.queue().pause()?;
            } else {
                handler_lock.lock().await.queue().resume()?;
            }

            if let Some(message_id) = queue.message_id {
                ctx.http
                    .edit_message(channel_id, message_id, &to_embed(queue), vec![])
                    .await?;
            }
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }

    Ok(())
}
