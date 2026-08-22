use async_trait::async_trait;
use mahayana_core::{ApprovalDecision, BuildProfile};
use mahayana_harness::{HarnessError, MahayanaHarness, ToolDefinition};
use mahayana_tool_host::{ToolCapabilities, ToolError, ToolHost, ToolRequest, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;

struct EchoHost;

#[async_trait]
impl ToolHost for EchoHost {
    async fn execute(&self, request: ToolRequest) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            content: request.arguments,
            is_error: false,
        })
    }

    fn capabilities(&self) -> ToolCapabilities {
        ToolCapabilities::for_profile(BuildProfile::DesktopFull)
    }
}

fn tool() -> ToolDefinition {
    ToolDefinition {
        name: "guarded.write".into(),
        description: "approval scope test".into(),
        input_schema: Value::Null,
        read_only: false,
        requires_approval: true,
        tags: Vec::new(),
    }
}

fn request() -> ToolRequest {
    ToolRequest {
        name: "guarded.write".into(),
        arguments: json!({"value": 1}),
    }
}

#[tokio::test]
async fn approval_lifetimes_are_scoped() {
    let harness = MahayanaHarness::new(BuildProfile::DesktopFull);
    let a = harness.create_session("a").unwrap();
    let b = harness.create_session("b").unwrap();
    harness.register_tool(tool(), Arc::new(EchoHost)).unwrap();

    let id = match harness.execute_tool(Some(&a.id), request()).await {
        Err(HarnessError::ApprovalRequired(id)) => id,
        other => panic!("expected approval request, got {other:?}"),
    };
    harness
        .resolve_approval(&id, ApprovalDecision::Accept)
        .unwrap();
    assert!(harness.execute_tool(Some(&a.id), request()).await.is_ok());
    assert!(matches!(
        harness.execute_tool(Some(&a.id), request()).await,
        Err(HarnessError::ApprovalRequired(_))
    ));

    let id = match harness.execute_tool(Some(&a.id), request()).await {
        Err(HarnessError::ApprovalRequired(id)) => id,
        other => panic!("expected approval request, got {other:?}"),
    };
    harness
        .resolve_approval(&id, ApprovalDecision::AcceptForSession)
        .unwrap();
    assert!(harness.execute_tool(Some(&a.id), request()).await.is_ok());
    assert!(harness.execute_tool(Some(&a.id), request()).await.is_ok());
    assert!(matches!(
        harness.execute_tool(Some(&b.id), request()).await,
        Err(HarnessError::ApprovalRequired(_))
    ));
}

#[tokio::test]
async fn session_approval_requires_session() {
    let harness = MahayanaHarness::new(BuildProfile::DesktopFull);
    harness.register_tool(tool(), Arc::new(EchoHost)).unwrap();
    let id = match harness.execute_tool(None, request()).await {
        Err(HarnessError::ApprovalRequired(id)) => id,
        other => panic!("expected approval request, got {other:?}"),
    };
    assert!(matches!(
        harness.resolve_approval(&id, ApprovalDecision::AcceptForSession),
        Err(HarnessError::InvalidConfig(_))
    ));
}
