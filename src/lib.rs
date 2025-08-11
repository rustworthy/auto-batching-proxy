#[macro_use]
extern crate tracing;
#[macro_use]
extern crate serde;

mod config;
mod error;

use anyhow::Context;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router, debug_handler};
pub use config::Config;
use secrecy::SecretString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use url::Url;

use crate::error::Error;

type Message = (EmbedRequest, oneshot::Sender<Vec<Embedding>>);

#[allow(unused)]
struct InferenceServiceWorker<T> {
    chan: mpsc::Receiver<T>,
    http_client: reqwest::Client,
    max_wait_time: Duration,
    max_batch_size: usize,
    inference_service_url: Url,
    inference_service_key: Option<SecretString>,
}

impl InferenceServiceWorker<Message> {
    async fn run(&mut self) {
        info!(
            inference_service_url = self.inference_service_url.as_str(),
            max_wait_time = self.max_wait_time.as_millis(),
            max_batch_size = self.max_batch_size,
            "launching inference service worker"
        );
        while let Some(msg) = self.chan.recv().await {
            debug!(inputs = ?msg.0.inputs, "inference worker received embedding request");
            drop(msg)
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(transparent)]
struct Embedding(Vec<f64>);

struct AppContext {
    config: Config,
    http_client: reqwest::Client,
    inference_service_chan: mpsc::Sender<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbedRequest {
    inputs: Vec<String>,
}

#[debug_handler]
async fn embed(
    State(ctx): State<Arc<AppContext>>,
    Json(embed_req): Json<EmbedRequest>,
) -> Result<Json<Vec<Embedding>>, Error> {
    debug!(
        inputs = ?embed_req.inputs,
        "handler received embded request"
    );
    let (tx, _rx) = oneshot::channel();
    ctx.inference_service_chan
        .send((embed_req.clone(), tx))
        .await
        .context("failed to send message to inference service")?;

    let url = ctx
        .config
        .inference_service_url
        .join("/embed")
        .context("Error constucting inference service endpoint path")?;
    debug!(url = %url, "sending request to inference service");
    let embeddings: Vec<Embedding> = ctx
        .http_client
        .post(url)
        .json(&embed_req)
        .send()
        .await
        .context("Error occurred when calling inference service")?
        .json()
        .await
        .context("Error occurred when deserializing response from inference service")?;
    debug!(
        embeddings_count = embeddings.len(),
        "received response from inference service, sending to end-user"
    );
    Ok(Json(embeddings))
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let addr = SocketAddr::from((config.ip, config.port));
    let listener = TcpListener::bind(addr).await?;

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(2_000))
        .build()
        .context("Failed to initialize http client")?;

    let (tx, rx) = mpsc::channel::<(EmbedRequest, oneshot::Sender<Vec<Embedding>>)>(1000);
    let ctx = Arc::new(AppContext {
        config: config.clone(),
        http_client: http_client.clone(),
        inference_service_chan: tx,
    });

    let mut worker = InferenceServiceWorker {
        chan: rx,
        http_client,
        max_wait_time: Duration::from_millis(config.max_wait_time),
        max_batch_size: config.max_batch_size,
        inference_service_url: config.inference_service_url,
        inference_service_key: config.inference_service_key,
    };

    let router = Router::new()
        .route("/embed", post(embed))
        .with_state(Arc::clone(&ctx));

    info!("Launching application at {:?}", &addr);

    tokio::select! {
        res = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal()) => {
            res.context("htpp server exited")
        },
        res = tokio::spawn(async move { worker.run().await }) => {
            res.context("inference worker exited")
        }
    }
}

/// Graceful shutdown signal.
///
/// Source: <https://github.com/davidpdrsn/realworld-axum-sqlx/blob/d03a2885b661c8466de24c507099e0e2d66b55bd/src/http/mod.rs>
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
