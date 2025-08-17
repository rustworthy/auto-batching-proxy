use std::sync::Arc;

use axum::response::IntoResponse;
use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("unprecessable entity")]
    Unprocessable(String),

    #[error("internal error occurred")]
    Anyhow(#[from] anyhow::Error),

    #[error("internal error occurred")]
    AnyhowArced(#[from] Arc<anyhow::Error>),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Error::Unprocessable(e) => {
                trace!(error = %e, "unprocessable enitity");
                return (StatusCode::UNPROCESSABLE_ENTITY, e).into_response();
            }
            Error::Anyhow(e) => {
                error!(error = ?e, "unexpected error occurred");
            }
            Error::AnyhowArced(e) => {
                error!(error = ?e, "unexpected error occurred");
            }
        };
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
