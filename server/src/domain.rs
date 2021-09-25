use once_cell::sync::Lazy;
use regex::Regex;
use std::convert::TryFrom;

static HOSTNAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^([a-zA-Z0-9]{1,63}\.)*([a-zA-Z0-9]{1,63})$"#).unwrap());

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String")]
pub struct Hostname(String);

impl TryFrom<String> for Hostname {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        dbg!(&value);
        if (1..=253).contains(&value.len()) {
            if HOSTNAME.is_match(&value) {
                Ok(Hostname(value))
            } else {
                Err("invalid chars")
            }
        } else {
            Err("too long")
        }
    }
}

impl AsRef<str> for Hostname {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn valid(s in r#"([a-zA-Z0-9]{1,6}\.)*([a-zA-Z0-9]{1,6})"#) {
            prop_assert_eq!(Hostname::try_from(s.clone()), Ok(Hostname(s)));
        }

        #[test]
        fn contains_bad_chars(s in r#"([_+)({}\[\]$#%^&*!@]){1,100}"#) {
            prop_assert_eq!(Hostname::try_from(s), Err("invalid chars"));
        }

        #[test]
        fn too_long(s in "[a-z]{400}") {
            prop_assert_eq!(Hostname::try_from(s), Err("too long"));
        }
    }
}
