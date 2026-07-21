use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the shared HTTP and WebSocket frame protocol.
pub const PROTOCOL_VERSION: u16 = 1;

/// A request sent over HTTP or an established WebSocket session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A successful or failed response paired with a request ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseFrame {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl ResponseFrame {
    /// Creates a successful response frame.
    #[must_use]
    pub fn success(id: impl Into<String>, payload: Value) -> Self {
        Self {
            id: id.into(),
            ok: true,
            payload: Some(payload),
            error: None,
        }
    }

    /// Creates a failed response frame.
    #[must_use]
    pub fn failure(id: impl Into<String>, error: ProtocolError) -> Self {
        Self {
            id: id.into(),
            ok: false,
            payload: None,
            error: Some(error),
        }
    }
}

/// A server-originated event delivered to subscribed WebSocket clients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventFrame {
    pub event: String,
    #[serde(default)]
    pub payload: Value,
}

impl EventFrame {
    /// Creates an event frame with an explicit payload.
    #[must_use]
    pub fn new(event: impl Into<String>, payload: Value) -> Self {
        Self {
            event: event.into(),
            payload,
        }
    }
}

/// A machine-readable protocol error.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

impl ProtocolError {
    /// Creates a protocol error with a stable code and user-facing message.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Initial WebSocket handshake parameters.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectParams {
    #[serde(default)]
    pub protocol_version: u16,
    #[serde(default)]
    pub token: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProtocolError, ResponseFrame};

    #[test]
    fn response_frames_round_trip_through_json() {
        let response = ResponseFrame::failure(
            "request-1",
            ProtocolError::new("unknown_method", "Method is not supported."),
        );
        let encoded = serde_json::to_string(&response).unwrap();
        let decoded: ResponseFrame = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, response);
        assert!(ResponseFrame::success("request-2", json!({"ok": true})).ok);
    }
}
