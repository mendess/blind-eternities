use core::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MusicSession([u8; 6]);

impl MusicSession {
    pub fn gen() -> MusicSession {
        MusicSession(thread_rng().gen())
    }
}

impl FromStr for MusicSession {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.as_bytes()
            .try_into()
            .map(Self)
            .map_err(|_| format!("invalid music session id: {s}"))
    }
}

impl fmt::Display for MusicSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MusicSession {
    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl Serialize for MusicSession {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MusicSession {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = MusicSession;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a 6 character ascii string")
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

        deserializer.deserialize_str(Visitor)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExpiresAt {
    pub expires_at: Option<DateTime<Utc>>,
}
