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
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

use crate::error::Error;

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(transparent)]
struct Embedding(Vec<f64>);

struct AppContext {
    config: Config,
    http_client: reqwest::Client,
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
        inputs_count = embed_req.inputs.len(),
        "received embded request"
    );
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

pub fn api(config: Config) -> anyhow::Result<Router> {
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(2_000))
        .build()
        .context("Faield to initialize http client")?;
    let ctx = Arc::new(AppContext {
        config,
        http_client,
    });
    let router = Router::new()
        .route("/embed", post(embed))
        .with_state(Arc::clone(&ctx));
    Ok(router)
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let addr = SocketAddr::from((config.ip, config.port));
    let listener = TcpListener::bind(addr).await?;
    let app = api(config)?;
    info!("Launching application at {:?}", &addr);
    Ok(axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?)
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
