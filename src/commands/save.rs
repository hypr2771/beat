use crate::errors::errors::BeatError;
use crate::QueueKey;
use serenity::all::{
    CommandOptionType, Context, CreateCommand, CreateCommandOption, Interaction, ResolvedOption,
    ResolvedValue,
};
use std::fs::{create_dir_all, File};
use std::io::Write;

pub fn register() -> CreateCommand {
    CreateCommand::new("save")
        .description("Saves the current playlist to replay later using /load command")
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
    if let Some(ResolvedOption {
        value: ResolvedValue::String(name),
        ..
    }) = options.first()
    {
        if let Some(guild_id) = if let Interaction::Command(command) = interaction {
            command.defer_ephemeral(ctx).await?;
            Some(command.guild_id.ok_or(BeatError::NoGuild)?)
        } else {
            None
        } {
            let queue_lock = {
                let guard = ctx.data.read().await;
                guard.get::<QueueKey>().unwrap().clone()
            };

            let mut maybe_queue = queue_lock.write().await;

            if let Some(existing_queue) = maybe_queue.get_mut(&guild_id) {
                println!("Queue exists: {:?}", existing_queue);

                let urls: Vec<String> = existing_queue
                    .queue
                    .iter()
                    .map(|track| track.source_url.clone().unwrap())
                    .collect();

                let urls = urls.join("\n");

                let dir_name = format!("./{}", guild_id);
                let file_name = format!("{}/{}.playlist", dir_name, name);

                create_dir_all(dir_name)?;

                File::create(file_name)?.write_all(urls.as_bytes())?;
            }
        }
    }

    if let Interaction::Command(command) = interaction {
        // Delete ephemeral response
        command.delete_response(ctx).await?;
    }
    Ok(())
}
