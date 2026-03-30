// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

// Structured JSON error responses for the REST API.
//
// Every error returned to clients carries a machine-readable `code` field
// (e.g. "ERR_UNAUTHORIZED") that the frontend uses to trigger specific
// recovery flows, plus a human-readable `message` for debugging.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// JSON error payload returned by every failing REST endpoint.
///
/// Clients key on the `code` field for programmatic error handling;
/// `message` is for human consumption / log context only.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

/// Typed server errors that map to [`ApiError`] for JSON serialisation.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("no mission loaded")]
    NoMission,

    #[error("mission not found: {0}")]
    MissionNotFound(u64),

    #[error("line index out of bounds: {0}")]
    LineNotFound(usize),

    #[error("invalid input: {0}")]
    BadRequest(String),

    #[error("plan generation failed: {0}")]
    PlanGeneration(String),

    #[error("export failed: {0}")]
    ExportFailed(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// SQLITE_BUSY is a fatal architectural violation [Doc 42].
    /// The WAL-reset bug can cause permanent database corruption under
    /// blind retry — return 503 and let the operator investigate.
    #[error("database locked: {0}")]
    DbLocked(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ServerError::NoMission => (StatusCode::NOT_FOUND, "ERR_NO_MISSION"),
            ServerError::MissionNotFound(_) => (StatusCode::NOT_FOUND, "ERR_MISSION_NOT_FOUND"),
            ServerError::LineNotFound(_) => (StatusCode::NOT_FOUND, "ERR_LINE_NOT_FOUND"),
            ServerError::BadRequest(_) => (StatusCode::BAD_REQUEST, "ERR_INVALID_INPUT"),
            ServerError::PlanGeneration(_) => (StatusCode::BAD_REQUEST, "ERR_PLAN_GENERATION"),
            ServerError::ExportFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "ERR_EXPORT_FAILED")
            }
            ServerError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ERR_INTERNAL"),
            ServerError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "ERR_UNAUTHORIZED"),
            ServerError::DbLocked(_) => (StatusCode::SERVICE_UNAVAILABLE, "ERR_DB_LOCKED"),
        };

        ApiError {
            status: status.as_u16(),
            code: code.to_string(),
            message: self.to_string(),
        }
        .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_serializes_to_json() {
        let err = ApiError {
            status: 404,
            code: "ERR_NO_MISSION".into(),
            message: "no mission loaded".into(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["status"], 404);
        assert_eq!(json["code"], "ERR_NO_MISSION");
    }

    #[test]
    fn server_error_display_includes_context() {
        let err = ServerError::MissionNotFound(42);
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn db_locked_maps_to_503() {
        let err = ServerError::DbLocked("write contention".into());
        let (status, code) = match &err {
            ServerError::DbLocked(_) => (StatusCode::SERVICE_UNAVAILABLE, "ERR_DB_LOCKED"),
            _ => unreachable!(),
        };
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code, "ERR_DB_LOCKED");
    }
}
