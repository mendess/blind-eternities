use reqwest::{Client, RequestBuilder, Url};

pub type UrlParseError = url::ParseError;

pub type Result<T> = std::result::Result<T, UrlParseError>;

pub struct AuthenticatedClient {
    client: Client,
    token: String,
    base: Url,
}

impl AuthenticatedClient {
    pub fn new(token: String, base: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            token,
            base: Url::parse(&base)?,
        })
    }

    pub fn get(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .get(self.base.join(path)?)
            .bearer_auth(&self.token))
    }

    pub fn post(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .post(self.base.join(path)?)
            .bearer_auth(&self.token))
    }

    pub fn delete(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .delete(self.base.join(path)?)
            .bearer_auth(&self.token))
    }

    pub fn put(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .put(self.base.join(path)?)
            .bearer_auth(&self.token))
    }

    pub fn patch(&self, path: &str) -> Result<RequestBuilder> {
        Ok(self
            .client
            .patch(self.base.join(path)?)
            .bearer_auth(&self.token))
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
