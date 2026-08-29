use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("config error: {0}")]
    Config(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("model load error: {0}")]
    ModelLoad(String),
    #[error("inference error: {0}")]
    Inference(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetails,
}

#[derive(Debug, Serialize)]
struct ErrorDetails {
    code: String,
    message: String,
}

impl ServiceError {
    fn status_code(&self) -> StatusCode {
        match self {
            ServiceError::Config(_) | ServiceError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServiceError::ModelLoad(_) | ServiceError::Inference(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            ServiceError::Config(_) => "config_error",
            ServiceError::BadRequest(_) => "bad_request",
            ServiceError::ModelLoad(_) => "model_load_error",
            ServiceError::Inference(_) => "inference_error",
            ServiceError::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> String {
        self.to_string()
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorBody {
            error: ErrorDetails {
                code: self.code().to_string(),
                message: self.message(),
            },
        };
        (status, Json(body)).into_response()
    }
}
