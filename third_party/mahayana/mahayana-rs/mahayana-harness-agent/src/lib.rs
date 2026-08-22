//! Bridges Fabushi's existing `AgentBackend` implementations into the
//! Rust-native Mahayana Harness event log.
//!
//! This preserves the already embedded Codex/Responses engines instead of
//! introducing a second model loop. Harness becomes the orchestration and
//! replay surface while `mahayana-agent` remains the provider boundary.

use mahayana_agent::{
    AgentActivityStatus, AgentBackend, AgentError, AgentEvent, AgentEventSink, AgentMessageRequest,
    ApprovalResolution, SharedAgentEventSink, StartThreadRequest,
};
use mahayana_core::{AgentThreadId, ApprovalDecision, ApprovalId, ConversationId, OperationId};
use mahayana_harness::{HarnessError, HarnessResult, MahayanaHarness};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct AgentThreadBinding {
    pub session_id: String,
    pub conversation_id: ConversationId,
    pub thread_id: AgentThreadId,
    pub last_operation_id: Option<OperationId>,
}

#[derive(Clone)]
pub struct HarnessAgentAdapter {
    harness: MahayanaHarness,
    backend: Arc<dyn AgentBackend>,
    bindings: Arc<Mutex<BTreeMap<String, AgentThreadBinding>>>,
}

impl HarnessAgentAdapter {
    pub fn new(harness: MahayanaHarness, backend: Arc<dyn AgentBackend>) -> Self {
        Self {
            harness,
            backend,
            bindings: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn binding(&self, session_id: &str) -> HarnessResult<Option<AgentThreadBinding>> {
        Ok(self
            .bindings
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .get(session_id)
            .cloned())
    }

    pub async fn ensure_thread(&self, session_id: &str) -> HarnessResult<AgentThreadBinding> {
        if let Some(binding) = self.binding(session_id)? {
            return Ok(binding);
        }
        self.harness
            .snapshot()?
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| HarnessError::SessionNotFound(session_id.to_string()))?;

        let conversation_id = ConversationId::generated("harness");
        let thread_id = self
            .backend
            .start_thread(StartThreadRequest {
                conversation_id: conversation_id.clone(),
            })
            .await
            .map_err(agent_error)?;
        let binding = AgentThreadBinding {
            session_id: session_id.to_string(),
            conversation_id,
            thread_id,
            last_operation_id: None,
        };
        self.bindings
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .insert(session_id.to_string(), binding.clone());
        self.harness.append_session_event(
            session_id,
            "agent/thread-started",
            json!({
                "threadId": binding.thread_id,
                "conversationId": binding.conversation_id,
                "backend": self.backend.name(),
            }),
        )?;
        Ok(binding)
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        text: impl Into<String>,
        client_message_id: Option<String>,
    ) -> HarnessResult<OperationId> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(HarnessError::InvalidConfig(
                "agent message must not be empty".into(),
            ));
        }
        let binding = self.ensure_thread(session_id).await?;
        let operation_id = OperationId::generated("harness-op");
        self.harness.append_session_event(
            session_id,
            "user/message",
            json!({
                "text": text,
                "clientMessageId": client_message_id,
                "operationId": operation_id,
            }),
        )?;
        self.harness.append_session_event(
            session_id,
            "agent/turn-started",
            json!({"operationId": operation_id, "threadId": binding.thread_id}),
        )?;

        {
            let mut bindings = self
                .bindings
                .lock()
                .map_err(|_| HarnessError::StatePoisoned)?;
            if let Some(binding) = bindings.get_mut(session_id) {
                binding.last_operation_id = Some(operation_id.clone());
            }
        }

        let sink: SharedAgentEventSink = Arc::new(HarnessAgentEventSink {
            harness: self.harness.clone(),
            session_id: session_id.to_string(),
            operation_id: operation_id.clone(),
        });
        let result = self
            .backend
            .send_message(
                AgentMessageRequest {
                    thread_id: binding.thread_id,
                    conversation_id: binding.conversation_id,
                    operation_id: operation_id.clone(),
                    text,
                    client_message_id,
                },
                sink,
            )
            .await;
        match result {
            Ok(()) => {
                self.harness.append_session_event(
                    session_id,
                    "agent/turn-completed",
                    json!({"operationId": operation_id}),
                )?;
                Ok(operation_id)
            }
            Err(error) => {
                self.harness.append_session_event(
                    session_id,
                    "agent/turn-failed",
                    json!({"operationId": operation_id, "error": error.to_string()}),
                )?;
                Err(agent_error(error))
            }
        }
    }

    pub async fn interrupt(&self, session_id: &str) -> HarnessResult<()> {
        let binding = self
            .binding(session_id)?
            .ok_or_else(|| HarnessError::SessionNotFound(session_id.to_string()))?;
        let operation_id = binding
            .last_operation_id
            .ok_or_else(|| HarnessError::ServiceNotFound("active agent operation".into()))?;
        self.backend
            .interrupt(&operation_id)
            .await
            .map_err(agent_error)?;
        self.harness.append_session_event(
            session_id,
            "agent/turn-interrupted",
            json!({"operationId": operation_id}),
        )?;
        Ok(())
    }

    pub async fn resolve_backend_approval(
        &self,
        session_id: &str,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        payload: Value,
    ) -> HarnessResult<()> {
        self.backend
            .resolve_approval(ApprovalResolution {
                approval_id: approval_id.clone(),
                decision,
                payload: payload.clone(),
            })
            .await
            .map_err(agent_error)?;
        self.harness.append_session_event(
            session_id,
            "agent/approval-resolved",
            json!({
                "approvalId": approval_id,
                "decision": decision,
                "payload": payload,
            }),
        )?;
        Ok(())
    }
}

struct HarnessAgentEventSink {
    harness: MahayanaHarness,
    session_id: String,
    operation_id: OperationId,
}

impl AgentEventSink for HarnessAgentEventSink {
    fn emit(&self, event: AgentEvent) -> Result<(), AgentError> {
        let (kind, payload) = match event {
            AgentEvent::MessageDelta { delta } => (
                "assistant/delta",
                json!({"delta": delta, "operationId": self.operation_id}),
            ),
            AgentEvent::MessageCompleted { message } => (
                "assistant/message",
                json!({
                    "text": message.text,
                    "message": message,
                    "operationId": self.operation_id,
                }),
            ),
            AgentEvent::TokenUsageUpdated { usage } => (
                "agent/token-usage",
                json!({"usage": usage, "operationId": self.operation_id}),
            ),
            AgentEvent::ToolProgress { message } => (
                "tool/progress",
                json!({"message": message, "operationId": self.operation_id}),
            ),
            AgentEvent::Activity { activity } => (
                "agent/activity",
                json!({
                    "stepId": activity.step_id,
                    "kind": activity.kind,
                    "title": activity.title,
                    "detail": activity.detail,
                    "status": activity_status(activity.status),
                    "metadata": activity.metadata,
                    "operationId": self.operation_id,
                }),
            ),
            AgentEvent::ApprovalRequested {
                approval_id,
                title,
                details,
            } => (
                "agent/approval-requested",
                json!({
                    "approvalId": approval_id,
                    "title": title,
                    "details": details,
                    "operationId": self.operation_id,
                }),
            ),
        };
        self.harness
            .append_session_event(&self.session_id, kind, payload)
            .map(|_| ())
            .map_err(|error| AgentError::Backend(error.to_string()))
    }
}

fn activity_status(status: AgentActivityStatus) -> &'static str {
    match status {
        AgentActivityStatus::Running => "running",
        AgentActivityStatus::Completed => "completed",
        AgentActivityStatus::Failed => "failed",
    }
}

fn agent_error(error: AgentError) -> HarnessError {
    HarnessError::ToolExecution(format!("agent backend: {error}"))
}
