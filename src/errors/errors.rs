use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use serenity::prelude::SerenityError;
use songbird::error::{ControlError, JoinError};
use songbird::input::AudioStreamError;
use url::ParseError;

#[derive(Debug)]
pub enum BeatError {
    Other(&'static str),
    NoSongbird,
    NoGuild,
    NoQueues,
    NoManager,
    NoHttp,
    NoPreviousTrack,
    NoPreviousSourceUrl,
    NoCurrentTrack,
    NoCurrentSourceUrl,
    NoValidCommand,
}

impl Display for BeatError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Other(msg) => f.write_str(msg),
            Self::NoSongbird => f.write_str("No Songbird for that context"),
            Self::NoGuild => f.write_str("No guild ID on that interaction"),
            Self::NoQueues => f.write_str("Queues not initialized"),
            Self::NoManager => f.write_str("No active connection"),
            Self::NoHttp => f.write_str("No HTTP client"),
            Self::NoPreviousTrack => f.write_str("No previous track to load"),
            Self::NoPreviousSourceUrl => f.write_str("Previous track has no source URL"),
            Self::NoCurrentTrack => f.write_str("No current track to load"),
            Self::NoCurrentSourceUrl => f.write_str("Current track has no source URL"),
            Self::NoValidCommand => f.write_str("Not a valid command"),
        }
    }
}

impl Error for BeatError {}

impl From<std::io::Error> for BeatError {
    fn from(_: std::io::Error) -> Self {
        Self::Other("JSON error")
    }
}

impl From<AudioStreamError> for BeatError {
    fn from(_: AudioStreamError) -> Self {
        Self::Other("Audio stream error")
    }
}

impl From<SerenityError> for BeatError {
    fn from(why: SerenityError) -> Self {
        Self::Other("Serenity error")
    }
}

impl From<JoinError> for BeatError {
    fn from(_: JoinError) -> Self {
        Self::Other("Could not join channel")
    }
}

impl From<ParseError> for BeatError {
    fn from(_: ParseError) -> Self {
        Self::Other("Could not parse URL")
    }
}

impl From<ControlError> for BeatError {
    fn from(_: ControlError) -> Self {
        Self::Other("Could not run control")
    }
}