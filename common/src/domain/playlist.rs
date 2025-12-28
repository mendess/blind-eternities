use http::HeaderName;
use serde::{Deserialize, Serialize};
use std::{ops::Deref, path::Path, str::FromStr, time::Duration};

pub const SONG_META_HEADER: HeaderName = HeaderName::from_static("x-song-meta");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SongId(String);

impl SongId {
    pub const SONG_ID_LEN: usize = 8;

    pub fn generate() -> Self {
        let id = std::iter::repeat_with(|| rand::random_range('0'..='z'))
            .filter(|a| a.is_alphanumeric())
            .take(Self::SONG_ID_LEN)
            .collect::<String>();
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        self
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum SongIdParseError {
    #[error("too long: {0}")]
    TooLong(usize),
    #[error("too short: {0}")]
    TooShort(usize),
    #[error("invalid char at: {0}")]
    InvalidCharAt(usize),
}

impl FromStr for SongId {
    type Err = SongIdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() < Self::SONG_ID_LEN {
            Err(SongIdParseError::TooShort(s.len()))
        } else if s.len() > Self::SONG_ID_LEN {
            Err(SongIdParseError::TooLong(s.len()))
        } else if let Some(pos) = s.chars().position(|c| !c.is_alphanumeric()) {
            Err(SongIdParseError::InvalidCharAt(pos))
        } else {
            Ok(Self(s.to_owned()))
        }
    }
}

impl AsRef<Path> for SongId {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl Deref for SongId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Serialize, Deserialize)]
pub struct SongMetadata {
    pub title: String,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct NavidromeId(String);

impl NavidromeId {
    fn check_str(v: &str) -> Result<(), &'static str> {
        if v.len() != "6x1qYGK5rD73Z8ZxxwtOs4".len() {
            Err("invalid length")
        } else if !v.bytes().all(|c| c.is_ascii_alphanumeric()) {
            Err("invalid char")
        } else {
            Ok(())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NavidromeId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::check_str(&value)?;
        Ok(Self(value))
    }
}

impl<'de> serde::Deserialize<'de> for NavidromeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;
        impl<'v> serde::de::Visitor<'v> for V {
            type Value = NavidromeId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "navidrome_id")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(E::custom)
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v)
            }
        }
        deserializer.deserialize_str(V)
    }
}

impl FromStr for NavidromeId {
    type Err = &'static str;
    fn from_str(v: &str) -> Result<Self, Self::Err> {
        Self::check_str(v)?;
        Ok(Self(v.to_owned()))
    }
}
