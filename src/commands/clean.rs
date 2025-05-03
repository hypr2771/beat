use crate::QueueKey;
use crate::errors::errors::BeatError;
use serde_json::json;
use serenity::all::Interaction;
use serenity::builder::CreateCommand;
use serenity::client::Context;
use tracing::error;

pub fn register() -> CreateCommand {
    CreateCommand::new("clean").description("Removes all previous messages from the bot")
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
        let all_messages = ctx.http.get_messages(channel_id, None, Some(100)).await?;

        let queue_lock = {
            let guard = ctx.data.read().await;
            guard.get::<QueueKey>().ok_or(BeatError::NoQueues)?.clone()
        };

        let mut maybe_queue = queue_lock.write().await;

        let bot_messages = if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
            if let Some(message_id) = existing_queue.message_id {
                all_messages
                    .iter()
                    .filter(|message| {
                        message.author.id == ctx.cache.current_user().id
                            || (message_id != message.id)
                    })
                    .map(|message| message.id.to_string())
                    .collect::<Vec<String>>()
            } else {
                all_messages
                    .iter()
                    .filter(|message| message.author.id == ctx.cache.current_user().id)
                    .map(|message| message.id.to_string())
                    .collect::<Vec<String>>()
            }
        } else {
            all_messages
                .iter()
                .filter(|message| message.author.id == ctx.cache.current_user().id)
                .map(|message| message.id.to_string())
                .collect::<Vec<String>>()
        };

        let json = json!({"messages": bot_messages});

        ctx.http
            .delete_messages(channel_id, &json, Some("Old messages"))
            .await
            .map_err(|error| error!(?error, "Failed to delete all messages"));
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }

    Ok(())
}
