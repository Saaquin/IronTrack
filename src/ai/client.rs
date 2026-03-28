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

/*
 * Claude API client using reqwest::blocking.
 *
 * Talks to POST https://api.anthropic.com/v1/messages with the required
 * headers (x-api-key, anthropic-version, content-type). The API key is
 * read from the ANTHROPIC_API_KEY environment variable at construction
 * time. No async -- this runs on the main CLI thread.
 */

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AiError;

use super::prompts;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_MAX_TOKENS: u32 = 4096;

// ---------------------------------------------------------------------------
// Request types (serialised to JSON and sent to the Claude API)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

// ---------------------------------------------------------------------------
// Response types (deserialised from the Claude API JSON body)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
    #[allow(dead_code)]
    usage: Usage,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[allow(dead_code)]
    input_tokens: u32,
    #[allow(dead_code)]
    output_tokens: u32,
}

/// Anthropic error response envelope.
#[derive(Deserialize)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: String,
}

// ---------------------------------------------------------------------------
// NlPlan extraction result
// ---------------------------------------------------------------------------

/// Structured parameters extracted from a natural language mission description.
///
/// All fields except the bounding box are optional -- Claude omits fields the
/// user did not mention, and IronTrack applies defaults.
#[derive(Debug, Deserialize)]
pub struct NlPlanParams {
    pub min_lat: Option<f64>,
    pub min_lon: Option<f64>,
    pub max_lat: Option<f64>,
    pub max_lon: Option<f64>,
    pub sensor: Option<String>,
    pub gsd_cm: Option<f64>,
    pub side_lap: Option<f64>,
    pub end_lap: Option<f64>,
    pub azimuth: Option<f64>,
    pub terrain: Option<bool>,
    pub mission_type: Option<String>,
    pub altitude_msl: Option<f64>,
    pub datum: Option<String>,
    // Custom sensor fields
    pub focal_length_mm: Option<f64>,
    pub sensor_width_mm: Option<f64>,
    pub sensor_height_mm: Option<f64>,
    pub image_width_px: Option<u32>,
    pub image_height_px: Option<u32>,
    // LiDAR fields
    pub lidar_prr: Option<f64>,
    pub lidar_scan_rate: Option<f64>,
    pub lidar_fov: Option<f64>,
    pub target_density: Option<f64>,
    // Error case -- Claude may return this when coordinates are missing
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// ClaudeClient
// ---------------------------------------------------------------------------

/// Blocking HTTP client for the Anthropic Claude Messages API.
pub struct ClaudeClient {
    http: reqwest::blocking::Client,
    api_key: String,
    model: String,
}

impl ClaudeClient {
    /// Construct a client by reading `ANTHROPIC_API_KEY` from the environment.
    pub fn from_env() -> Result<Self, AiError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| AiError::MissingApiKey)?;
        if api_key.trim().is_empty() {
            return Err(AiError::MissingApiKey);
        }

        let http = reqwest::blocking::Client::builder()
            .user_agent(format!("irontrack/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| AiError::Http(e.to_string()))?;

        Ok(Self {
            http,
            api_key,
            model: DEFAULT_MODEL.to_string(),
        })
    }

    /// Send a single user message with an optional system prompt and return
    /// all text blocks from the response concatenated together.
    pub fn send(&self, system: Option<&str>, user_message: &str) -> Result<String, AiError> {
        let request_body = MessagesRequest {
            model: &self.model,
            max_tokens: DEFAULT_MAX_TOKENS,
            system,
            messages: vec![Message {
                role: "user",
                content: user_message,
            }],
        };

        let body_bytes = serde_json::to_vec(&request_body)
            .map_err(|e| AiError::Http(format!("request serialization failed: {e}")))?;

        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .body(body_bytes)
            .send()
            .map_err(|e| AiError::Http(e.to_string()))?;

        let status = response.status();
        let response_bytes = response
            .bytes()
            .map_err(|e| AiError::Http(format!("failed to read response body: {e}")))?;

        if !status.is_success() {
            let message = serde_json::from_slice::<ApiErrorResponse>(&response_bytes)
                .map(|e| e.error.message)
                .unwrap_or_else(|_| String::from_utf8_lossy(&response_bytes).into_owned());
            return Err(AiError::Api {
                status: status.as_u16(),
                message,
            });
        }

        let parsed: MessagesResponse = serde_json::from_slice(&response_bytes)
            .map_err(|e| AiError::ResponseParse(format!("{e}")))?;

        let text: String = parsed
            .content
            .into_iter()
            .filter(|b| b.block_type == "text")
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            return Err(AiError::ResponseParse("no text block in response".into()));
        }
        Ok(text)
    }

    /// Send a natural language mission description and parse the structured
    /// JSON response into `NlPlanParams`.
    pub fn extract_plan_params(&self, description: &str) -> Result<NlPlanParams, AiError> {
        let raw = self.send(Some(prompts::NL_PLAN_SYSTEM_PROMPT), description)?;
        let json_str = strip_code_fences(&raw);

        let params: NlPlanParams = serde_json::from_str(json_str).map_err(|e| {
            AiError::ExtractionFailed(format!("JSON parse failed: {e}\nRaw response:\n{raw}"))
        })?;

        if let Some(ref err_msg) = params.error {
            return Err(AiError::ExtractionFailed(err_msg.clone()));
        }

        Ok(params)
    }
}

/// Strip optional markdown code fences from the response. The system prompt
/// asks for raw JSON, but this is a safety net in case the model wraps it.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let stripped = stripped.strip_suffix("```").unwrap_or(stripped);
    stripped.trim()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_fences_raw_json() {
        let input = r#"{"min_lat": 51.0}"#;
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_code_fences_with_json_fence() {
        let input = "```json\n{\"min_lat\": 51.0}\n```";
        assert_eq!(strip_code_fences(input), r#"{"min_lat": 51.0}"#);
    }

    #[test]
    fn strip_code_fences_with_plain_fence() {
        let input = "```\n{\"min_lat\": 51.0}\n```";
        assert_eq!(strip_code_fences(input), r#"{"min_lat": 51.0}"#);
    }

    #[test]
    fn nl_plan_params_partial_deserialize() {
        let json = r#"{"min_lat":51.0,"min_lon":-1.0,"max_lat":51.1,"max_lon":-0.9}"#;
        let params: NlPlanParams = serde_json::from_str(json).expect("should parse");
        assert!((params.min_lat.unwrap() - 51.0).abs() < f64::EPSILON);
        assert!(params.sensor.is_none());
        assert!(params.gsd_cm.is_none());
    }

    #[test]
    fn nl_plan_params_error_field() {
        let json = r#"{"error":"Please provide coordinates."}"#;
        let params: NlPlanParams = serde_json::from_str(json).expect("should parse");
        assert!(params.error.is_some());
    }

    #[test]
    fn from_env_missing_key() {
        // Temporarily unset the key (test isolation)
        let orig = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = ClaudeClient::from_env();
        // Restore
        if let Some(key) = orig {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
        assert!(result.is_err());
    }
}
