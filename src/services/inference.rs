use anyhow::Context;
use secrecy::SecretString;
use std::time::Duration;
use tokio::sync::mpsc;
use url::Url;

pub(crate) struct ServiceWorkerConfig {
    max_wait_time: Duration,
    max_batch_size: usize,
    inference_service_url: Url,
    #[allow(unused)]
    inference_service_key: Option<SecretString>,
}

impl From<crate::Config> for ServiceWorkerConfig {
    fn from(value: crate::Config) -> Self {
        Self {
            max_wait_time: Duration::from_millis(value.max_wait_time),
            max_batch_size: value.max_batch_size,
            inference_service_url: value.inference_service_url,
            inference_service_key: value.inference_service_key,
        }
    }
}

pub(crate) struct InferenceServiceWorker<T> {
    client: reqwest::Client,
    embed_endpoint: Url,
    chan: mpsc::Receiver<T>,
    config: ServiceWorkerConfig,
}
impl<T> InferenceServiceWorker<T> {
    pub fn init<C>(chan: mpsc::Receiver<T>, config: C) -> anyhow::Result<Self>
    where
        C: Into<ServiceWorkerConfig>,
    {
        let config = config.into();
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(2_000))
            .build()
            .context("Failed to initialize http client")?;
        let embed_endpoint = config
            .inference_service_url
            .join("/embed")
            .context("Error constucting inference service endpoint path")?;
        Ok(Self {
            client: http_client,
            embed_endpoint,
            chan,
            config,
        })
    }
}

impl InferenceServiceWorker<crate::Message> {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        info!(
            inference_service_url = self.config.inference_service_url.as_str(),
            max_wait_time = self.config.max_wait_time.as_millis(),
            max_batch_size = self.config.max_batch_size,
            "launching inference service worker"
        );
        while let Some(msg) = self.chan.recv().await {
            debug!(inputs = ?msg.0.inputs, "inference worker received embedding request");
            debug!("sending request to inference service");
            let resp = match self
                .client
                .post(self.embed_endpoint.clone())
                .json(&msg.0)
                .send()
                .await
                .context("Error occurred when calling inference service")
            {
                Err(_e) => {
                    continue;
                }
                Ok(resp) => resp,
            };
            match resp
                .json()
                .await
                .context("Error occurred when deserializing response from inference service")
            {
                Err(_e) => continue,
                Ok(embeddings) => {
                    if msg.1.send(embeddings).is_err() {
                        error!("error sending embeddings back to handler, channel closed");
                    }
                }
            }
        }
        Ok(())
    }
}
