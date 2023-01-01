use std::{collections::HashMap, fmt, str::FromStr};

use common::domain::Hostname;

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Destination {
    #[serde(default)]
    pub username: Option<String>,
    pub hostname: Hostname,
}

impl FromStr for Destination {
    type Err = <Hostname as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('@') {
            Some((username, hostname)) => Ok(Destination {
                hostname: hostname.parse()?,
                username: Some(username.parse::<Hostname>()?.into_string()),
            }),
            None => Ok(Destination {
                hostname: s.parse()?,
                username: None,
            }),
        }
    }
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(u) => write!(f, "{}@{}", u, self.hostname),
            None => write!(f, "{}", self.hostname),
        }
    }
}

impl Destination {
    pub fn resolve_alias<'s>(
        &'s self,
        aliases: &'s HashMap<String, Destination>,
    ) -> (String, &'s Hostname) {
        match aliases.get(self.hostname.as_ref()) {
            Some(d) => {
                tracing::debug!("resolving alias {} as {}", self.hostname, d);
                (
                    self.username
                        .clone()
                        .or_else(|| d.username.clone())
                        .unwrap_or_else(whoami::username),
                    &d.hostname,
                )
            }
            None => (
                self.username.clone().unwrap_or_else(whoami::username),
                &self.hostname,
            ),
        }
    }
}
