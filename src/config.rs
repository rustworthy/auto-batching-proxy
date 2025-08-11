use anyhow::Context as _;
use figment::{Figment, providers::Env};
use secrecy::SecretString;
use std::net::IpAddr;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub max_wait_time: u64,
    pub max_batch_size: usize,
    pub inference_service_url: Url,
    pub inference_service_key: Option<SecretString>,
    pub ip: IpAddr,
    pub port: u16,
}

impl Config {
    pub fn try_build() -> anyhow::Result<Self> {
        let config = Figment::new()
            .merge(Env::raw())
            .extract()
            .context("Failed to build configuration")?;
        Ok(config)
    }
}
