use anyhow::Context;
use chrono::Utc;
use core::panic;
use secrecy::SecretString;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use url::Url;

struct InferenceServiceClient {
    http_client: reqwest::Client,
    embed_endpoint: Url,
}

impl InferenceServiceClient {
    async fn embed(&self, inputs: Vec<String>) -> anyhow::Result<Vec<crate::Embedding>> {
        let embeddings = self
            .http_client
            .post(self.embed_endpoint.clone())
            .json(&crate::EmbedRequest { inputs })
            .send()
            .await
            .context("Error occurred when calling inference service")?
            .json()
            .await
            .context("Error occurred when deserializing response from inference service")?;
        Ok(embeddings)
    }
}

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
    client: InferenceServiceClient,
    chan: mpsc::Receiver<T>,
    config: ServiceWorkerConfig,
    queue: Vec<crate::Message>,
    timeout: Option<Duration>,
}

impl<T> InferenceServiceWorker<T> {
    pub fn init<C>(chan: mpsc::Receiver<T>, config: C) -> anyhow::Result<Self>
    where
        C: Into<ServiceWorkerConfig>,
    {
        let config = config.into();
        let http_client = reqwest::Client::builder()
            // TODO: make this configurable
            .timeout(Duration::from_millis(2_000))
            .build()
            .context("Failed to initialize http client")?;
        let embed_endpoint = config
            .inference_service_url
            .join("/embed")
            .context("Error constucting inference service endpoint path")?;
        let client = InferenceServiceClient {
            http_client,
            embed_endpoint,
        };
        let queue = Vec::with_capacity(config.max_batch_size);

        Ok(Self {
            client,
            chan,
            config,
            queue,
            timeout: None,
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

        loop {
            tokio::select! {
                res = self.chan.recv() => {
                    if let Some(msg) = res {
                        if self.queue.is_empty() {
                            match (Utc::now() - msg.0).to_std() {
                                Ok(elapsed) => {
                                    self.timeout = Some(self.config.max_wait_time - elapsed);
                                },
                                // conversion to std::Duration errors if timedelta < 0
                                Err(_) => {
                                    self.timeout = Some(Duration::ZERO);
                                }
                            }
                        }
                        trace!(inputs = ?msg.1.inputs, "inference worker received embedding request");
                        self.queue.push(msg);
                        if self.queue.len() < self.config.max_batch_size { continue; }
                        trace!("max batch size reached, sending to inference service");

                        self.timeout.take();
                        let batch = std::mem::take(&mut self.queue);
                        process_batch(batch, &self.client).await
                    } else { break; }
                },
                _ = async {
                        let timeout = self.timeout.take().expect("only polled by tokio if cond is true");
                        tokio::time::sleep(timeout).await
                    }, if self.timeout.is_some() => {
                    trace!(
                        batch_size = self.queue.len(),
                        "timeout reached, sending accumulated requests");

                    let batch = std::mem::take(&mut self.queue);
                    process_batch(batch, &self.client).await
                },

            }
        }

        Ok(())
    }
}

fn broadcast_error(e: anyhow::Error, batch: Vec<crate::Message>) {
    let err = Arc::new(e);
    for (_, _, chan) in batch {
        if chan.send(Err(Arc::clone(&err))).is_err() {
            error!("error sending response back to handler, channel closed");
        }
    }
}

async fn process_batch(batch: Vec<crate::Message>, client: &InferenceServiceClient) {
    let inputs: Vec<_> = batch
        .iter()
        // TODO: we can consider mem::take'ing here, but we
        // need to make sure there is a way to store the offset:
        // to be able to link reqests and responses
        .flat_map(|(_, req, _)| req.inputs.clone())
        .collect();

    let embeddings = match client.embed(inputs).await {
        Err(e) => {
            broadcast_error(e, batch);
            return;
        }
        Ok(resp) => resp,
    };
    trace!(
        embeddings_count = embeddings.len(),
        "got embeddings from inference service"
    );

    // TODO: what if the length of embeddings differs from inputs
    let mut offset = 0;
    for (_, req, chan) in batch {
        let limit = req.inputs.len();
        trace!(
            offset,
            limit, "projecting into embeddings to get repsonses for this handler"
        );
        let embeddings = &embeddings[offset..offset + limit];
        if chan.send(Ok(embeddings.to_owned())).is_err() {
            error!("error sending embeddings back to handler, channel closed");
        }
        offset += limit;
    }

    trace!("fanned out results and unset timeout until next request");
}
