use std::sync::Arc;

use axum::response::IntoResponse;
use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("internal error occurred")]
    Anyhow(#[from] anyhow::Error),

    #[error("internal error occurred")]
    AnyhowArced(#[from] Arc<anyhow::Error>),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Error::Anyhow(e) => {
                error!(error = ?e, "unexpected error occurred");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
            Error::AnyhowArced(e) => {
                error!(error = ?e, "unexpected error occurred");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
