use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: u16,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::DatabaseError(_) => StatusCode::SERVICE_UNAVAILABLE,
        };

        let body = Json(ErrorResponse {
            error: self.to_string(),
            code: status.as_u16(),
        });

        (status, body).into_response()
    }
} 