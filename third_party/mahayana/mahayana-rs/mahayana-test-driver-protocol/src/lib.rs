//! Versioned, presentation-neutral test-driver protocol for Mahayana.
//!
//! The protocol is intentionally a control plane, not a second product core.
//! Product operations are delegated through [`TestDriverBackend`]; this crate
//! owns only request/response framing, correlation, structured events, bounded
//! diagnostic logs, redaction and driver lifecycle semantics.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TEST_DRIVER_PROTOCOL: &str = "mahayana.test-driver.v1";
pub const TEST_DRIVER_PROTOCOL_VERSION: u32 = 1;
const MAX_EVENTS: usize = 512;
const MAX_LOGS: usize = 512;

pub const REQUIRED_METHODS: &[&str] = &[
    "health",
    "resetProfile",
    "loginTestAccount",
    "marketplace.search",
    "plugin.install",
    "plugin.update",
    "plugin.list",
    "miniapp.open",
    "miniapp.chat",
    "actions.describe",
    "actions.invoke",
    "events.subscribe",
    "logs.query",
    "shutdown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestDriverMethod {
    #[serde(rename = "health")]
    Health,
    #[serde(rename = "resetProfile")]
    ResetProfile,
    #[serde(rename = "loginTestAccount")]
    LoginTestAccount,
    #[serde(rename = "marketplace.search")]
    MarketplaceSearch,
    #[serde(rename = "plugin.install")]
    PluginInstall,
    #[serde(rename = "plugin.update")]
    PluginUpdate,
    #[serde(rename = "plugin.list")]
    PluginList,
    #[serde(rename = "miniapp.open")]
    MiniappOpen,
    #[serde(rename = "miniapp.chat")]
    MiniappChat,
    #[serde(rename = "actions.describe")]
    ActionsDescribe,
    #[serde(rename = "actions.invoke")]
    ActionsInvoke,
    #[serde(rename = "events.subscribe")]
    EventsSubscribe,
    #[serde(rename = "logs.query")]
    LogsQuery,
    #[serde(rename = "shutdown")]
    Shutdown,
}

impl TestDriverMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::ResetProfile => "resetProfile",
            Self::LoginTestAccount => "loginTestAccount",
            Self::MarketplaceSearch => "marketplace.search",
            Self::PluginInstall => "plugin.install",
            Self::PluginUpdate => "plugin.update",
            Self::PluginList => "plugin.list",
            Self::MiniappOpen => "miniapp.open",
            Self::MiniappChat => "miniapp.chat",
            Self::ActionsDescribe => "actions.describe",
            Self::ActionsInvoke => "actions.invoke",
            Self::EventsSubscribe => "events.subscribe",
            Self::LogsQuery => "logs.query",
            Self::Shutdown => "shutdown",
        }
    }

    pub const fn is_control_plane(self) -> bool {
        matches!(
            self,
            Self::Health | Self::EventsSubscribe | Self::LogsQuery | Self::Shutdown
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDriverRequest {
    pub protocol: String,
    pub request_id: String,
    #[serde(default)]
    pub correlation_id: Option<String>,
    pub method: TestDriverMethod,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDriverResponse {
    pub protocol: String,
    pub request_id: String,
    pub correlation_id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub error: Option<TestDriverError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDriverEvent {
    pub protocol: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub correlation_id: String,
    pub kind: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDriverLogEntry {
    pub protocol: String,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub correlation_id: String,
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct TestDriverError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

impl TestDriverError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Value::Null,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = redact_value(details);
        self
    }
}

/// Product-backed operations required by the test-driver contract.
///
/// Implementations must call the same Mahayana core/product services used by
/// normal application surfaces. A backend must never synthesize marketplace,
/// plugin, conversation or action success solely for automation.
pub trait TestDriverBackend {
    fn backend_name(&self) -> &'static str;

    fn execute(
        &mut self,
        method: TestDriverMethod,
        params: Value,
        correlation_id: &str,
    ) -> Result<Value, TestDriverError>;
}

pub struct TestDriverSession<B> {
    backend: B,
    events: VecDeque<TestDriverEvent>,
    logs: VecDeque<TestDriverLogEntry>,
    next_event_sequence: u64,
    next_log_sequence: u64,
    shutdown_requested: bool,
}

impl<B: TestDriverBackend> TestDriverSession<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            events: VecDeque::new(),
            logs: VecDeque::new(),
            next_event_sequence: 1,
            next_log_sequence: 1,
            shutdown_requested: false,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn execute_json_line(&mut self, input: &str) -> String {
        let raw = match serde_json::from_str::<Value>(input) {
            Ok(value) => value,
            Err(error) => {
                return serialize_response(TestDriverResponse {
                    protocol: TEST_DRIVER_PROTOCOL.into(),
                    request_id: String::new(),
                    correlation_id: String::new(),
                    ok: false,
                    result: Value::Null,
                    error: Some(TestDriverError::new("invalid_json", error.to_string())),
                });
            }
        };

        let request_id = raw
            .get("requestId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let correlation_id = raw
            .get("correlationId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&request_id)
            .to_string();

        let request = match serde_json::from_value::<TestDriverRequest>(raw) {
            Ok(request) => request,
            Err(error) => {
                return serialize_response(TestDriverResponse {
                    protocol: TEST_DRIVER_PROTOCOL.into(),
                    request_id,
                    correlation_id,
                    ok: false,
                    result: Value::Null,
                    error: Some(TestDriverError::new("invalid_request", error.to_string())),
                });
            }
        };
        serialize_response(self.execute(request))
    }

    pub fn execute(&mut self, request: TestDriverRequest) -> TestDriverResponse {
        let request_id = request.request_id.trim().to_string();
        let correlation_id = request
            .correlation_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&request_id)
            .to_string();

        let validation = if request.protocol != TEST_DRIVER_PROTOCOL {
            Some(TestDriverError::new(
                "unsupported_protocol",
                format!(
                    "expected {TEST_DRIVER_PROTOCOL}, received {}",
                    request.protocol
                ),
            ))
        } else if request_id.is_empty() {
            Some(TestDriverError::new(
                "invalid_request",
                "requestId must be non-empty",
            ))
        } else if self.shutdown_requested && request.method != TestDriverMethod::Health {
            Some(TestDriverError::new(
                "driver_shutdown",
                "test driver is shutting down",
            ))
        } else {
            None
        };

        if let Some(error) = validation {
            return TestDriverResponse {
                protocol: TEST_DRIVER_PROTOCOL.into(),
                request_id,
                correlation_id,
                ok: false,
                result: Value::Null,
                error: Some(error),
            };
        }

        let method = request.method;
        let params = request.params;
        self.push_event(
            &correlation_id,
            "request.started",
            json!({"requestId": request_id, "method": method.as_str()}),
        );
        self.push_log(
            &correlation_id,
            "info",
            "test-driver request started",
            json!({"requestId": request_id, "method": method.as_str(), "params": params}),
        );

        let outcome = match method {
            TestDriverMethod::Health => Ok(json!({
                "protocol": TEST_DRIVER_PROTOCOL,
                "version": TEST_DRIVER_PROTOCOL_VERSION,
                "backend": self.backend.backend_name(),
                "debugOnly": true,
                "shutdownRequested": self.shutdown_requested,
                "supportedMethods": REQUIRED_METHODS,
            })),
            TestDriverMethod::EventsSubscribe => Ok(self.query_events(&params)),
            TestDriverMethod::LogsQuery => Ok(self.query_logs(&params)),
            TestDriverMethod::Shutdown => {
                self.shutdown_requested = true;
                Ok(json!({"shutdownRequested": true}))
            }
            business_method => {
                self.backend
                    .execute(business_method, params.clone(), &correlation_id)
            }
        };

        match outcome {
            Ok(result) => {
                self.push_event(
                    &correlation_id,
                    "request.completed",
                    json!({"requestId": request_id, "method": method.as_str()}),
                );
                self.push_log(
                    &correlation_id,
                    "info",
                    "test-driver request completed",
                    json!({"requestId": request_id, "method": method.as_str()}),
                );
                TestDriverResponse {
                    protocol: TEST_DRIVER_PROTOCOL.into(),
                    request_id,
                    correlation_id,
                    ok: true,
                    result: redact_value(result),
                    error: None,
                }
            }
            Err(error) => {
                let safe_error = TestDriverError {
                    code: error.code,
                    message: redact_text(&error.message),
                    details: redact_value(error.details),
                };
                self.push_event(
                    &correlation_id,
                    "request.failed",
                    json!({
                        "requestId": request_id,
                        "method": method.as_str(),
                        "errorCode": safe_error.code,
                    }),
                );
                self.push_log(
                    &correlation_id,
                    "error",
                    "test-driver request failed",
                    json!({
                        "requestId": request_id,
                        "method": method.as_str(),
                        "errorCode": safe_error.code,
                    }),
                );
                TestDriverResponse {
                    protocol: TEST_DRIVER_PROTOCOL.into(),
                    request_id,
                    correlation_id,
                    ok: false,
                    result: Value::Null,
                    error: Some(safe_error),
                }
            }
        }
    }

    fn query_events(&self, params: &Value) -> Value {
        let after = params
            .get("afterSequence")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let limit = bounded_limit(params, 100);
        let events = self
            .events
            .iter()
            .filter(|event| event.sequence > after)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        json!({
            "events": events,
            "lastSequence": self.events.back().map(|event| event.sequence).unwrap_or(0),
        })
    }

    fn query_logs(&self, params: &Value) -> Value {
        let after = params
            .get("afterSequence")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let requested_correlation = params.get("correlationId").and_then(Value::as_str);
        let limit = bounded_limit(params, 100);
        let entries = self
            .logs
            .iter()
            .filter(|entry| entry.sequence > after)
            .filter(|entry| {
                requested_correlation
                    .map(|value| entry.correlation_id == value)
                    .unwrap_or(true)
            })
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        json!({
            "entries": entries,
            "lastSequence": self.logs.back().map(|entry| entry.sequence).unwrap_or(0),
        })
    }

    fn push_event(&mut self, correlation_id: &str, kind: &str, data: Value) {
        let event = TestDriverEvent {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            sequence: self.next_event_sequence,
            timestamp_ms: now_ms(),
            correlation_id: correlation_id.into(),
            kind: kind.into(),
            data: redact_value(data),
        };
        self.next_event_sequence = self.next_event_sequence.saturating_add(1);
        self.events.push_back(event);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    fn push_log(&mut self, correlation_id: &str, level: &str, message: &str, fields: Value) {
        let entry = TestDriverLogEntry {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            sequence: self.next_log_sequence,
            timestamp_ms: now_ms(),
            correlation_id: correlation_id.into(),
            level: level.into(),
            message: redact_text(message),
            fields: redact_value(fields),
        };
        self.next_log_sequence = self.next_log_sequence.saturating_add(1);
        self.logs.push_back(entry);
        while self.logs.len() > MAX_LOGS {
            self.logs.pop_front();
        }
    }
}

fn serialize_response(response: TestDriverResponse) -> String {
    serde_json::to_string(&response).unwrap_or_else(|_| {
        format!(
            "{{\"protocol\":\"{TEST_DRIVER_PROTOCOL}\",\"requestId\":\"\",\"correlationId\":\"\",\"ok\":false,\"result\":null,\"error\":{{\"code\":\"serialization_error\",\"message\":\"response serialization failed\",\"details\":null}}}}"
        )
    })
}

fn bounded_limit(params: &Value, default: usize) -> usize {
    params
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
        .clamp(1, 500)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "cookie",
        "email",
        "header",
        "password",
        "secret",
        "sessiontoken",
        "token",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

pub fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    *value = redact_value(value.take());
                }
            }
            Value::Object(map)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        other => other,
    }
}

fn redact_text(text: &str) -> String {
    // Free-form strings are never inspected for or echoed from raw request
    // payloads. Backend errors may still contain accidental bearer material;
    // suppress the common credential prefix instead of persisting it.
    if text.to_ascii_lowercase().contains("bearer ") {
        "[REDACTED CREDENTIAL MATERIAL]".into()
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[derive(Default)]
    struct EchoBackend {
        calls: Vec<(TestDriverMethod, String, Value)>,
    }

    impl TestDriverBackend for EchoBackend {
        fn backend_name(&self) -> &'static str {
            "echo-test-backend"
        }

        fn execute(
            &mut self,
            method: TestDriverMethod,
            params: Value,
            correlation_id: &str,
        ) -> Result<Value, TestDriverError> {
            self.calls
                .push((method, correlation_id.to_string(), params.clone()));
            Ok(json!({"method": method.as_str(), "params": params}))
        }
    }

    fn request(method: TestDriverMethod, params: Value) -> TestDriverRequest {
        TestDriverRequest {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            request_id: "req-1".into(),
            correlation_id: Some("corr-1".into()),
            method,
            params,
        }
    }

    #[test]
    fn health_advertises_complete_required_contract() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let response = session.execute(request(TestDriverMethod::Health, json!({})));
        assert!(response.ok);
        assert_eq!(response.result["protocol"], TEST_DRIVER_PROTOCOL);
        assert_eq!(response.result["version"], TEST_DRIVER_PROTOCOL_VERSION);
        assert_eq!(
            response.result["supportedMethods"],
            serde_json::to_value(REQUIRED_METHODS).unwrap()
        );
    }

    #[test]
    fn business_operation_is_delegated_with_same_correlation_id() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let response = session.execute(request(
            TestDriverMethod::MarketplaceSearch,
            json!({"query": "全球法布施"}),
        ));
        assert!(response.ok);
        assert_eq!(response.correlation_id, "corr-1");
        assert_eq!(session.backend().calls.len(), 1);
        assert_eq!(
            session.backend().calls[0].0,
            TestDriverMethod::MarketplaceSearch
        );
        assert_eq!(session.backend().calls[0].1, "corr-1");
    }

    #[test]
    fn event_stream_links_start_and_terminal_event_by_correlation_id() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let _ = session.execute(request(TestDriverMethod::PluginList, json!({})));
        let response = session.execute(TestDriverRequest {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            request_id: "events-1".into(),
            correlation_id: Some("events-corr".into()),
            method: TestDriverMethod::EventsSubscribe,
            params: json!({"afterSequence": 0}),
        });
        let events = response.result["events"].as_array().unwrap();
        assert!(events.iter().any(|event| {
            event["correlationId"] == "corr-1" && event["kind"] == "request.started"
        }));
        assert!(events.iter().any(|event| {
            event["correlationId"] == "corr-1" && event["kind"] == "request.completed"
        }));
    }

    #[test]
    fn diagnostic_logs_redact_credentials_and_email_fields() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let _ = session.execute(request(
            TestDriverMethod::LoginTestAccount,
            json!({
                "token": "top-secret-token",
                "email": "person@example.com",
                "nested": {"authorization": "Bearer abc", "safe": "visible"}
            }),
        ));
        let response = session.execute(TestDriverRequest {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            request_id: "logs-1".into(),
            correlation_id: Some("logs-corr".into()),
            method: TestDriverMethod::LogsQuery,
            params: json!({"correlationId": "corr-1"}),
        });
        let serialized = serde_json::to_string(&response.result).unwrap();
        assert!(!serialized.contains("top-secret-token"));
        assert!(!serialized.contains("person@example.com"));
        assert!(!serialized.contains("Bearer abc"));
        assert!(serialized.contains("[REDACTED]"));
        assert!(serialized.contains("visible"));
    }

    #[test]
    fn protocol_mismatch_fails_closed_without_backend_execution() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let mut bad = request(TestDriverMethod::PluginList, json!({}));
        bad.protocol = "mahayana.test-driver.v999".into();
        let response = session.execute(bad);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "unsupported_protocol");
        assert!(session.backend().calls.is_empty());
    }

    #[test]
    fn shutdown_prevents_later_business_operations() {
        let mut session = TestDriverSession::new(EchoBackend::default());
        let response = session.execute(request(TestDriverMethod::Shutdown, json!({})));
        assert!(response.ok);
        assert!(session.shutdown_requested());
        let later = session.execute(TestDriverRequest {
            protocol: TEST_DRIVER_PROTOCOL.into(),
            request_id: "req-2".into(),
            correlation_id: None,
            method: TestDriverMethod::PluginList,
            params: json!({}),
        });
        assert!(!later.ok);
        assert_eq!(later.error.unwrap().code, "driver_shutdown");
    }
}
