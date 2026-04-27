use application::ApplicationError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    BadRequest(String),
    Conflict(String),
    NotFound(String),
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };

        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<ApplicationError> for ApiError {
    fn from(value: ApplicationError) -> Self {
        match value {
            ApplicationError::Validation(message) => Self::BadRequest(message),
            ApplicationError::UnsupportedProvider(provider) => {
                Self::BadRequest(format!("unsupported provider: {provider}"))
            }
            ApplicationError::IdempotencyConflict { scope, key } => Self::Conflict(format!(
                "idempotency key conflict for scope={scope} key={key}"
            )),
            ApplicationError::IntentNotFound(id) => {
                Self::NotFound(format!("payment intent not found: {id}"))
            }
            ApplicationError::Domain(err) => Self::BadRequest(err.to_string()),
            ApplicationError::Persistence(err) => Self::Internal(err.to_string()),
        }
    }
}
