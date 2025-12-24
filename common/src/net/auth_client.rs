use std::sync::Arc;

use reqwest::{RequestBuilder, Url};
use uuid::Uuid;

pub type UrlParseError = url::ParseError;

pub type Result<T> = std::result::Result<T, UrlParseError>;

#[derive(Debug, Clone)]
pub struct Client {
    client: reqwest::Client,
    base: Arc<Url>,
}

impl Client {
    pub fn new(base: Url) -> Result<Self> {
        if base.cannot_be_a_base() {
            return Err(UrlParseError::SetHostOnCannotBeABaseUrl);
        }
        Ok(Self {
            client: reqwest::Client::new(),
            base: Arc::new(base),
        })
    }

    pub fn hostname(&self) -> &Url {
        &self.base
    }

    pub fn get(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.get(self.base.join(path)?))
    }

    pub fn post(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.post(self.base.join(path)?))
    }

    pub fn delete(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.delete(self.base.join(path)?))
    }

    pub fn put(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.put(self.base.join(path)?))
    }

    pub fn patch(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.patch(self.base.join(path)?))
    }

    pub fn authenticate(&self, token: Uuid) -> AuthenticatedClient {
        AuthenticatedClient {
            client: self.clone(),
            token,
        }
    }
}

pub struct AuthenticatedClient {
    client: Client,
    token: Uuid,
}

impl AuthenticatedClient {
    pub fn new(token: uuid::Uuid, base: Url) -> Result<Self> {
        Ok(Self {
            client: Client::new(base)?,
            token,
        })
    }

    pub fn hostname(&self) -> &Url {
        self.client.hostname()
    }

    pub fn get(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.get(path)?.bearer_auth(self.token))
    }

    pub fn post(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.post(path)?.bearer_auth(self.token))
    }

    pub fn delete(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.delete(path)?.bearer_auth(self.token))
    }

    pub fn put(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.put(path)?.bearer_auth(self.token))
    }

    pub fn patch(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self.client.patch(path)?.bearer_auth(self.token))
    }

    pub fn without_auth(&self) -> &Client {
        &self.client
    }

    pub fn token(&self) -> uuid::Uuid {
        self.token
    }
}
