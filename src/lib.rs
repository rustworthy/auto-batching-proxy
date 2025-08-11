#[macro_use]
extern crate tracing;
#[macro_use]
extern crate serde;

mod config;
mod error;
mod services;

use crate::error::Error;
use crate::services::inference::InferenceServiceWorker;
use anyhow::Context;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

pub use config::Config;

pub(crate) type Message = (
    DateTime<Utc>,
    EmbedRequest,
    oneshot::Sender<Result<Vec<Embedding>, Arc<anyhow::Error>>>,
);

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(transparent)]
struct Embedding(Vec<f64>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EmbedRequest {
    pub inputs: Vec<String>,
}

struct AppContext {
    inference_service_chan: mpsc::Sender<Message>,
}

async fn embed(
    State(ctx): State<Arc<AppContext>>,
    Json(embed_req): Json<EmbedRequest>,
) -> Result<Json<Vec<Embedding>>, Error> {
    debug!(
        inputs = ?embed_req.inputs,
        "handler received embded request"
    );
    let (tx, rx) = oneshot::channel();
    ctx.inference_service_chan
        .send((Utc::now(), embed_req, tx))
        .await
        .context("failed to send message to inference service")?;

    let embeddings = rx
        .await
        .context("failed to receive message back from inference worker")??;
    debug!(
        embeddings_count = embeddings.len(),
        "handler received response from inference service worker, sending to end-user"
    );
    Ok(Json(embeddings))
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let addr = SocketAddr::from((config.ip, config.port));
    let listener = TcpListener::bind(addr).await?;
    // TODO: make this configurable?
    let (tx, rx) = mpsc::channel::<Message>(1000);
    let mut worker = InferenceServiceWorker::init(rx, config)?;
    let ctx = Arc::new(AppContext {
        inference_service_chan: tx,
    });
    let router = Router::new()
        .route("/embed", post(embed))
        .with_state(Arc::clone(&ctx));

    info!("Launching application at {:?}", &addr);
    tokio::select! {
        res = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal()) => {
            res.context("htpp server exited")
        },
        Ok(res) = tokio::spawn(async move { worker.run().await }) => {
            res.context("worker exited with error")
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
