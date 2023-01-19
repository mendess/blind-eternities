use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{Database, Decode};
use std::{
    convert::TryFrom,
    fmt::{self, Display},
    str::FromStr,
};

static HOSTNAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^([a-zA-Z0-9]{1,63}\.)*([a-zA-Z0-9]{1,63})$"#).unwrap());

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(try_from = "String")]
pub struct Hostname(String);

impl Hostname {
    pub fn into_string(self) -> String {
        self.0
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct HostnameParseError {
    pub value: String,
    pub reason: HostnameParseErrorReason,
}

impl Display for HostnameParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum HostnameParseErrorReason {
    #[error("invalid chars")]
    InvalidChars,
    #[error("too long (max is 253 chars)")]
    TooLong,
}

impl TryFrom<String> for Hostname {
    type Error = HostnameParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if (1..=253).contains(&value.len()) {
            if HOSTNAME.is_match(&value) {
                Ok(Hostname(value))
            } else {
                Err(HostnameParseError {
                    value,
                    reason: HostnameParseErrorReason::InvalidChars,
                })
            }
        } else {
            Err(HostnameParseError {
                value,
                reason: HostnameParseErrorReason::TooLong,
            })
        }
    }
}

impl TryFrom<&str> for Hostname {
    type Error = <Hostname as TryFrom<String>>::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for Hostname {
    type Err = <Hostname as TryFrom<String>>::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl AsRef<str> for Hostname {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Hostname {
    pub fn from_this_host() -> Self {
        let hostname = whoami::hostname();
        if hostname == "localhost" {
            Self(std::env::var("HOSTNAME").unwrap_or(hostname))
        } else {
            Self(hostname)
        }
    }
}

//TODO: delete?
impl<'r, DB: Database> Decode<'r, DB> for Hostname
where
    String: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let v = <String as Decode<DB>>::decode(value)?;
        Ok(Hostname::try_from(v)?)
    }
}

#[cfg(test)]
pub mod tests {
    use std::ops::RangeInclusive;

    use super::*;
    use fake::Dummy;
    use proptest::prelude::*;
    use rand::{distributions::Uniform, prelude::Distribution, seq::SliceRandom, Rng};

    pub struct FakeHostname;

    impl Dummy<FakeHostname> for Hostname {
        fn dummy_with_rng<R: Rng + ?Sized>(_: &FakeHostname, rng: &mut R) -> Self {
            const RANGES: [RangeInclusive<char>; 3] = ['a'..='z', 'A'..='Z', '0'..='9'];
            Hostname::try_from(
                std::iter::repeat_with(|| {
                    Uniform::from(RANGES.choose(rng).unwrap().clone()).sample(rng)
                })
                .take(10)
                .collect::<String>(),
            )
            .unwrap()
        }
    }

    proptest! {
        #[test]
        fn valid(s in r#"([a-zA-Z0-9]{1,6}\.)*([a-zA-Z0-9]{1,6})"#) {
            prop_assert_eq!(Hostname::try_from(s.clone()), Ok(Hostname(s)));
        }

        #[test]
        fn contains_bad_chars(s in r#"([_+)({}\[\]$#%^&*!@]){1,100}"#) {
            prop_assert_eq!(
                Hostname::try_from(s.clone()),
                Err(HostnameParseError { reason: HostnameParseErrorReason::InvalidChars, value: s })
            );
        }

        #[test]
        fn too_long(s in "[a-z]{400}") {
            prop_assert_eq!(
                Hostname::try_from(s.clone()),
                Err(HostnameParseError { reason: HostnameParseErrorReason::TooLong, value: s })
            );
        }
    }
}
