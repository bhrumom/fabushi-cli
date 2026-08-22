//! Rust-native composable agent harness for Mahayana.
//!
//! The design intentionally mirrors the capability-oriented architecture of
//! modern agent harnesses without embedding a JavaScript runtime. Everything
//! is expressed as Rust services, registries, event streams, and replaceable
//! providers so Electron, mobile, headless, and CLI surfaces share one core.

use async_trait::async_trait;
use mahayana_core::{ApprovalDecision, BuildProfile, ConversationId};
use mahayana_tool_host::{ToolHost, ToolRequest, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub type HarnessResult<T> = Result<T, HarnessError>;

#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("service already registered: {0}")]
    DuplicateService(String),
    #[error("service not found: {0}")]
    ServiceNotFound(String),
    #[error("tool already registered: {0}")]
    DuplicateTool(String),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("plugin already mounted: {0}")]
    DuplicatePlugin(String),
    #[error("plugin not mounted: {0}")]
    PluginNotFound(String),
    #[error("approval required: {0}")]
    ApprovalRequired(String),
    #[error("approval not found: {0}")]
    ApprovalNotFound(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("job not found: {0}")]
    JobNotFound(String),
    #[error("goal not found: {0}")]
    GoalNotFound(String),
    #[error("workflow not found: {0}")]
    WorkflowNotFound(String),
    #[error("profile not found: {0}")]
    ProfileNotFound(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("tool execution failed: {0}")]
    ToolExecution(String),
    #[error("state mutex is poisoned")]
    StatePoisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventScope {
    Session,
    Agent,
    Capability,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEvent {
    pub id: String,
    pub sequence: u64,
    pub timestamp_ms: i64,
    pub scope: EventScope,
    pub kind: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub events: Vec<HarnessEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub tool: String,
    pub reason: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
    pub preset: String,
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRecord {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub result_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub steps: Vec<Value>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDefinition {
    pub id: String,
    #[serde(default)]
    pub bundles: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub overlay: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub build_profile: BuildProfile,
    pub profile: String,
    pub services: Vec<String>,
    pub plugins: Vec<PluginManifest>,
    pub tools: Vec<ToolDefinition>,
    pub sessions: Vec<SessionRecord>,
    pub agents: Vec<AgentDescriptor>,
    pub approvals: Vec<ApprovalRequest>,
    pub goals: Vec<GoalRecord>,
    pub jobs: Vec<JobRecord>,
    pub workflows: Vec<WorkflowRecord>,
    pub event_sequence: u64,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    async fn stream(&self, request: LlmRequest, sink: Arc<dyn LlmStreamSink>) -> HarnessResult<()>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub metadata: Value,
}

#[async_trait]
pub trait LlmStreamSink: Send + Sync {
    async fn delta(&self, text: &str) -> HarnessResult<()>;
    async fn complete(&self, metadata: Value) -> HarnessResult<()>;
}

#[async_trait]
pub trait StorageProvider: Send + Sync {
    async fn get(&self, namespace: &str, key: &str) -> HarnessResult<Option<Vec<u8>>>;
    async fn put(&self, namespace: &str, key: &str, bytes: &[u8]) -> HarnessResult<()>;
    async fn delete(&self, namespace: &str, key: &str) -> HarnessResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterceptorDecision {
    Continue,
    Reject(String),
}

#[async_trait]
pub trait ToolInterceptor: Send + Sync {
    async fn before_execute(
        &self,
        definition: &ToolDefinition,
        request: &ToolRequest,
    ) -> HarnessResult<InterceptorDecision>;

    async fn after_execute(
        &self,
        definition: &ToolDefinition,
        request: &ToolRequest,
        result: &ToolResult,
    ) -> HarnessResult<()>;
}

#[derive(Clone)]
struct RegisteredTool {
    definition: ToolDefinition,
    host: Arc<dyn ToolHost>,
}

struct HarnessState {
    build_profile: BuildProfile,
    active_profile: String,
    profiles: BTreeMap<String, ProfileDefinition>,
    services: BTreeSet<String>,
    plugins: BTreeMap<String, PluginManifest>,
    tools: BTreeMap<String, RegisteredTool>,
    sessions: BTreeMap<String, SessionRecord>,
    agents: BTreeMap<String, AgentDescriptor>,
    approvals: BTreeMap<String, ApprovalRequest>,
    approved_tools: BTreeSet<String>,
    goals: BTreeMap<String, GoalRecord>,
    jobs: BTreeMap<String, JobRecord>,
    workflows: BTreeMap<String, WorkflowRecord>,
    events: VecDeque<HarnessEvent>,
    event_sequence: u64,
    interceptors: Vec<Arc<dyn ToolInterceptor>>,
    llm_providers: BTreeMap<String, Arc<dyn LlmProvider>>,
    storage_providers: BTreeMap<String, Arc<dyn StorageProvider>>,
}

impl HarnessState {
    fn new(build_profile: BuildProfile) -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".into(),
            ProfileDefinition {
                id: "default".into(),
                bundles: vec!["mahayana-base".into()],
                plugins: Vec::new(),
                overlay: Value::Null,
            },
        );
        Self {
            build_profile,
            active_profile: "default".into(),
            profiles,
            services: BTreeSet::new(),
            plugins: BTreeMap::new(),
            tools: BTreeMap::new(),
            sessions: BTreeMap::new(),
            agents: BTreeMap::new(),
            approvals: BTreeMap::new(),
            approved_tools: BTreeSet::new(),
            goals: BTreeMap::new(),
            jobs: BTreeMap::new(),
            workflows: BTreeMap::new(),
            events: VecDeque::new(),
            event_sequence: 0,
            interceptors: Vec::new(),
            llm_providers: BTreeMap::new(),
            storage_providers: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct MahayanaHarness {
    state: Arc<Mutex<HarnessState>>,
}

impl MahayanaHarness {
    pub fn new(build_profile: BuildProfile) -> Self {
        let harness = Self {
            state: Arc::new(Mutex::new(HarnessState::new(build_profile))),
        };
        let _ = harness.emit(
            EventScope::Runtime,
            "runtime/ready",
            None,
            None,
            Value::Null,
        );
        harness
    }

    pub fn register_service(&self, name: impl Into<String>) -> HarnessResult<()> {
        let name = required(name.into(), "service")?;
        let mut state = self.state()?;
        if !state.services.insert(name.clone()) {
            return Err(HarnessError::DuplicateService(name));
        }
        drop(state);
        self.emit(
            EventScope::Capability,
            "service/registered",
            None,
            None,
            serde_json::json!({"service": name}),
        )?;
        Ok(())
    }

    pub fn unregister_service(&self, name: &str) -> HarnessResult<()> {
        let mut state = self.state()?;
        if !state.services.remove(name) {
            return Err(HarnessError::ServiceNotFound(name.into()));
        }
        drop(state);
        self.emit(
            EventScope::Capability,
            "service/unregistered",
            None,
            None,
            serde_json::json!({"service": name}),
        )?;
        Ok(())
    }

    pub fn register_profile(&self, profile: ProfileDefinition) -> HarnessResult<()> {
        let mut state = self.state()?;
        state
            .profiles
            .insert(required(profile.id.clone(), "profile id")?, profile);
        Ok(())
    }

    pub fn activate_profile(&self, profile_id: &str) -> HarnessResult<()> {
        let mut state = self.state()?;
        if !state.profiles.contains_key(profile_id) {
            return Err(HarnessError::ProfileNotFound(profile_id.into()));
        }
        state.active_profile = profile_id.into();
        drop(state);
        self.emit(
            EventScope::Runtime,
            "profile/activated",
            None,
            None,
            serde_json::json!({"profile": profile_id}),
        )?;
        Ok(())
    }

    pub fn mount_plugin(&self, manifest: PluginManifest) -> HarnessResult<()> {
        let id = required(manifest.id.clone(), "plugin id")?;
        let mut state = self.state()?;
        if state.plugins.contains_key(&id) {
            return Err(HarnessError::DuplicatePlugin(id));
        }
        state.plugins.insert(id.clone(), manifest);
        drop(state);
        self.emit(
            EventScope::Capability,
            "plugin/mounted",
            None,
            None,
            serde_json::json!({"pluginId": id}),
        )?;
        Ok(())
    }

    pub fn unmount_plugin(&self, plugin_id: &str) -> HarnessResult<()> {
        let mut state = self.state()?;
        if state.plugins.remove(plugin_id).is_none() {
            return Err(HarnessError::PluginNotFound(plugin_id.into()));
        }
        drop(state);
        self.emit(
            EventScope::Capability,
            "plugin/unmounted",
            None,
            None,
            serde_json::json!({"pluginId": plugin_id}),
        )?;
        Ok(())
    }

    pub fn register_tool(
        &self,
        definition: ToolDefinition,
        host: Arc<dyn ToolHost>,
    ) -> HarnessResult<()> {
        let name = required(definition.name.clone(), "tool name")?;
        let mut state = self.state()?;
        if state.tools.contains_key(&name) {
            return Err(HarnessError::DuplicateTool(name));
        }
        state
            .tools
            .insert(name.clone(), RegisteredTool { definition, host });
        drop(state);
        self.emit(
            EventScope::Capability,
            "tool/registered",
            None,
            None,
            serde_json::json!({"tool": name}),
        )?;
        Ok(())
    }

    pub fn add_tool_interceptor(&self, interceptor: Arc<dyn ToolInterceptor>) -> HarnessResult<()> {
        self.state()?.interceptors.push(interceptor);
        Ok(())
    }

    pub fn register_llm_provider(&self, provider: Arc<dyn LlmProvider>) -> HarnessResult<()> {
        let id = required(provider.id().to_string(), "llm provider id")?;
        self.state()?.llm_providers.insert(id.clone(), provider);
        self.emit(
            EventScope::Capability,
            "llm/registered",
            None,
            None,
            serde_json::json!({"provider": id}),
        )?;
        Ok(())
    }

    pub fn register_storage_provider(
        &self,
        id: impl Into<String>,
        provider: Arc<dyn StorageProvider>,
    ) -> HarnessResult<()> {
        let id = required(id.into(), "storage provider id")?;
        self.state()?.storage_providers.insert(id.clone(), provider);
        self.emit(
            EventScope::Capability,
            "storage/registered",
            None,
            None,
            serde_json::json!({"provider": id}),
        )?;
        Ok(())
    }

    pub fn create_session(&self, title: impl Into<String>) -> HarnessResult<SessionRecord> {
        let now = now_ms();
        let session = SessionRecord {
            id: format!("session:{}", Uuid::new_v4()),
            title: required(title.into(), "session title")?,
            created_at_ms: now,
            updated_at_ms: now,
            parent_session_id: None,
            events: Vec::new(),
        };
        self.state()?
            .sessions
            .insert(session.id.clone(), session.clone());
        self.emit(
            EventScope::Session,
            "session/created",
            Some(&session.id),
            None,
            serde_json::json!({"title": session.title}),
        )?;
        Ok(session)
    }

    pub fn ensure_session_for_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> HarnessResult<SessionRecord> {
        let id = format!("conversation:{}", conversation_id.as_str());
        if let Some(existing) = self.state()?.sessions.get(&id).cloned() {
            return Ok(existing);
        }
        let now = now_ms();
        let session = SessionRecord {
            id: id.clone(),
            title: conversation_id.as_str().into(),
            created_at_ms: now,
            updated_at_ms: now,
            parent_session_id: None,
            events: Vec::new(),
        };
        self.state()?.sessions.insert(id.clone(), session.clone());
        self.emit(
            EventScope::Session,
            "session/created",
            Some(&id),
            None,
            serde_json::json!({"conversationId": conversation_id}),
        )?;
        Ok(session)
    }

    pub fn fork_session(&self, source_session_id: &str) -> HarnessResult<SessionRecord> {
        let source = self
            .state()?
            .sessions
            .get(source_session_id)
            .cloned()
            .ok_or_else(|| HarnessError::SessionNotFound(source_session_id.into()))?;
        let now = now_ms();
        let mut child = source.clone();
        child.id = format!("session:{}", Uuid::new_v4());
        child.parent_session_id = Some(source.id.clone());
        child.created_at_ms = now;
        child.updated_at_ms = now;
        self.state()?
            .sessions
            .insert(child.id.clone(), child.clone());
        self.emit(
            EventScope::Session,
            "session/forked",
            Some(&child.id),
            None,
            serde_json::json!({"sourceSessionId": source_session_id}),
        )?;
        Ok(child)
    }

    pub fn spawn_agent(
        &self,
        name: impl Into<String>,
        preset: impl Into<String>,
        session_id: impl Into<String>,
    ) -> HarnessResult<AgentDescriptor> {
        let session_id = required(session_id.into(), "session id")?;
        if !self.state()?.sessions.contains_key(&session_id) {
            return Err(HarnessError::SessionNotFound(session_id));
        }
        let agent = AgentDescriptor {
            id: format!("agent:{}", Uuid::new_v4()),
            name: required(name.into(), "agent name")?,
            preset: required(preset.into(), "agent preset")?,
            session_id: session_id.clone(),
            status: "idle".into(),
        };
        self.state()?.agents.insert(agent.id.clone(), agent.clone());
        self.emit(
            EventScope::Agent,
            "agent/spawned",
            Some(&session_id),
            Some(&agent.id),
            serde_json::json!({"preset": agent.preset}),
        )?;
        Ok(agent)
    }

    pub fn set_goal(&self, session_id: &str, text: impl Into<String>) -> HarnessResult<GoalRecord> {
        if !self.state()?.sessions.contains_key(session_id) {
            return Err(HarnessError::SessionNotFound(session_id.into()));
        }
        let now = now_ms();
        let goal = GoalRecord {
            id: format!("goal:{}", Uuid::new_v4()),
            session_id: session_id.into(),
            text: required(text.into(), "goal")?,
            status: "active".into(),
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.state()?.goals.insert(goal.id.clone(), goal.clone());
        self.emit(
            EventScope::Agent,
            "goal/created",
            Some(session_id),
            None,
            serde_json::json!({"goalId": goal.id, "text": goal.text}),
        )?;
        Ok(goal)
    }

    pub fn update_goal_status(
        &self,
        goal_id: &str,
        status: impl Into<String>,
    ) -> HarnessResult<GoalRecord> {
        let mut state = self.state()?;
        let goal = state
            .goals
            .get_mut(goal_id)
            .ok_or_else(|| HarnessError::GoalNotFound(goal_id.into()))?;
        goal.status = required(status.into(), "goal status")?;
        goal.updated_at_ms = now_ms();
        let result = goal.clone();
        drop(state);
        self.emit(
            EventScope::Agent,
            "goal/updated",
            Some(&result.session_id),
            None,
            serde_json::json!({"goalId": result.id, "status": result.status}),
        )?;
        Ok(result)
    }

    pub fn create_job(&self, kind: impl Into<String>) -> HarnessResult<JobRecord> {
        let now = now_ms();
        let job = JobRecord {
            id: format!("job:{}", Uuid::new_v4()),
            kind: required(kind.into(), "job kind")?,
            status: "queued".into(),
            created_at_ms: now,
            updated_at_ms: now,
            result_ref: None,
        };
        self.state()?.jobs.insert(job.id.clone(), job.clone());
        self.emit(
            EventScope::Runtime,
            "job/created",
            None,
            None,
            serde_json::json!({"jobId": job.id, "kind": job.kind}),
        )?;
        Ok(job)
    }

    pub fn update_job(
        &self,
        job_id: &str,
        status: impl Into<String>,
        result_ref: Option<String>,
    ) -> HarnessResult<JobRecord> {
        let mut state = self.state()?;
        let job = state
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| HarnessError::JobNotFound(job_id.into()))?;
        job.status = required(status.into(), "job status")?;
        job.result_ref = result_ref;
        job.updated_at_ms = now_ms();
        let result = job.clone();
        drop(state);
        self.emit(
            EventScope::Runtime,
            "job/updated",
            None,
            None,
            serde_json::json!({"jobId": result.id, "status": result.status}),
        )?;
        Ok(result)
    }

    pub fn register_workflow(&self, workflow: WorkflowRecord) -> HarnessResult<()> {
        let id = required(workflow.id.clone(), "workflow id")?;
        self.state()?.workflows.insert(id.clone(), workflow);
        self.emit(
            EventScope::Capability,
            "workflow/registered",
            None,
            None,
            serde_json::json!({"workflowId": id}),
        )?;
        Ok(())
    }

    pub async fn execute_tool(
        &self,
        session_id: Option<&str>,
        request: ToolRequest,
    ) -> HarnessResult<ToolResult> {
        let (registered, interceptors, approved) = {
            let state = self.state()?;
            let registered = state
                .tools
                .get(&request.name)
                .cloned()
                .ok_or_else(|| HarnessError::ToolNotFound(request.name.clone()))?;
            let approved = state.approved_tools.contains(&request.name);
            (registered, state.interceptors.clone(), approved)
        };

        if registered.definition.requires_approval && !registered.definition.read_only && !approved
        {
            let approval = ApprovalRequest {
                id: format!("approval:{}", Uuid::new_v4()),
                tool: request.name.clone(),
                reason: format!("{} requires explicit approval", request.name),
                created_at_ms: now_ms(),
                session_id: session_id.map(ToOwned::to_owned),
            };
            self.state()?
                .approvals
                .insert(approval.id.clone(), approval.clone());
            self.emit(
                EventScope::Capability,
                "tool/approval-required",
                session_id,
                None,
                serde_json::to_value(&approval).unwrap_or(Value::Null),
            )?;
            return Err(HarnessError::ApprovalRequired(approval.id));
        }

        for interceptor in &interceptors {
            match interceptor
                .before_execute(&registered.definition, &request)
                .await?
            {
                InterceptorDecision::Continue => {}
                InterceptorDecision::Reject(reason) => {
                    return Err(HarnessError::ToolExecution(reason));
                }
            }
        }

        self.emit(
            EventScope::Session,
            "tool/call",
            session_id,
            None,
            serde_json::json!({"tool": request.name, "arguments": request.arguments}),
        )?;
        let result = registered
            .host
            .execute(request.clone())
            .await
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        for interceptor in &interceptors {
            interceptor
                .after_execute(&registered.definition, &request, &result)
                .await?;
        }
        self.emit(EventScope::Session, "tool/result", session_id, None, serde_json::json!({"tool": request.name, "isError": result.is_error, "content": result.content}))?;
        Ok(result)
    }

    pub fn resolve_approval(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> HarnessResult<()> {
        let mut state = self.state()?;
        let approval = state
            .approvals
            .remove(approval_id)
            .ok_or_else(|| HarnessError::ApprovalNotFound(approval_id.into()))?;
        if matches!(
            decision,
            ApprovalDecision::Accept | ApprovalDecision::AcceptForSession
        ) {
            state.approved_tools.insert(approval.tool.clone());
        }
        drop(state);
        self.emit(EventScope::Capability, "tool/approval-resolved", approval.session_id.as_deref(), None, serde_json::json!({"approvalId": approval_id, "tool": approval.tool, "decision": decision}))?;
        Ok(())
    }

    pub fn append_session_event(
        &self,
        session_id: &str,
        kind: impl Into<String>,
        payload: Value,
    ) -> HarnessResult<HarnessEvent> {
        self.emit(
            EventScope::Session,
            &kind.into(),
            Some(session_id),
            None,
            payload,
        )
    }

    pub fn derive_messages(&self, session_id: &str) -> HarnessResult<Vec<LlmMessage>> {
        let state = self.state()?;
        let session = state
            .sessions
            .get(session_id)
            .ok_or_else(|| HarnessError::SessionNotFound(session_id.into()))?;
        let messages = session
            .events
            .iter()
            .filter_map(|event| {
                let role = match event.kind.as_str() {
                    "user/message" => "user",
                    "assistant/message" => "assistant",
                    "system/message" => "system",
                    _ => return None,
                };
                event
                    .payload
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| LlmMessage {
                        role: role.into(),
                        content: text.into(),
                    })
            })
            .collect();
        Ok(messages)
    }

    pub fn transcript(&self, session_id: &str) -> HarnessResult<String> {
        let messages = self.derive_messages(session_id)?;
        Ok(messages
            .into_iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn snapshot(&self) -> HarnessResult<RuntimeSnapshot> {
        let state = self.state()?;
        Ok(RuntimeSnapshot {
            build_profile: state.build_profile,
            profile: state.active_profile.clone(),
            services: state.services.iter().cloned().collect(),
            plugins: state.plugins.values().cloned().collect(),
            tools: state
                .tools
                .values()
                .map(|tool| tool.definition.clone())
                .collect(),
            sessions: state.sessions.values().cloned().collect(),
            agents: state.agents.values().cloned().collect(),
            approvals: state.approvals.values().cloned().collect(),
            goals: state.goals.values().cloned().collect(),
            jobs: state.jobs.values().cloned().collect(),
            workflows: state.workflows.values().cloned().collect(),
            event_sequence: state.event_sequence,
        })
    }

    pub fn dump_config(&self) -> HarnessResult<Value> {
        let state = self.state()?;
        let profile = state
            .profiles
            .get(&state.active_profile)
            .ok_or_else(|| HarnessError::ProfileNotFound(state.active_profile.clone()))?;
        Ok(serde_json::json!({
            "profile": profile,
            "services": state.services,
            "plugins": state.plugins,
            "tools": state.tools.values().map(|tool| &tool.definition).collect::<Vec<_>>(),
        }))
    }

    pub fn poll_event(&self) -> HarnessResult<Option<HarnessEvent>> {
        Ok(self.state()?.events.pop_front())
    }

    pub fn content_address(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("sha256:{:x}", hasher.finalize())
    }

    fn emit(
        &self,
        scope: EventScope,
        kind: &str,
        session_id: Option<&str>,
        agent_id: Option<&str>,
        payload: Value,
    ) -> HarnessResult<HarnessEvent> {
        let mut state = self.state()?;
        state.event_sequence = state.event_sequence.saturating_add(1);
        let event = HarnessEvent {
            id: format!("event:{}", Uuid::new_v4()),
            sequence: state.event_sequence,
            timestamp_ms: now_ms(),
            scope,
            kind: kind.into(),
            session_id: session_id.map(ToOwned::to_owned),
            agent_id: agent_id.map(ToOwned::to_owned),
            payload,
        };
        if let Some(session_id) = session_id
            && let Some(session) = state.sessions.get_mut(session_id)
        {
            session.updated_at_ms = event.timestamp_ms;
            session.events.push(event.clone());
        }
        state.events.push_back(event.clone());
        Ok(event)
    }

    fn state(&self) -> HarnessResult<std::sync::MutexGuard<'_, HarnessState>> {
        self.state.lock().map_err(|_| HarnessError::StatePoisoned)
    }
}

fn required(value: String, field: &str) -> HarnessResult<String> {
    if value.trim().is_empty() {
        Err(HarnessError::InvalidConfig(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(value)
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_log_drives_transcript() {
        let harness = MahayanaHarness::new(BuildProfile::DesktopFull);
        let session = harness.create_session("test").unwrap();
        harness
            .append_session_event(
                &session.id,
                "user/message",
                serde_json::json!({"text": "hello"}),
            )
            .unwrap();
        harness
            .append_session_event(
                &session.id,
                "assistant/message",
                serde_json::json!({"text": "world"}),
            )
            .unwrap();
        assert_eq!(
            harness.transcript(&session.id).unwrap(),
            "user: hello\nassistant: world"
        );
    }

    #[test]
    fn profiles_plugins_and_services_are_replaceable() {
        let harness = MahayanaHarness::new(BuildProfile::DesktopFull);
        harness.register_service("tools").unwrap();
        harness
            .mount_plugin(PluginManifest {
                id: "example".into(),
                name: "Example".into(),
                version: "1.0.0".into(),
                services: vec!["tools".into()],
                tools: Vec::new(),
                configuration: Value::Null,
            })
            .unwrap();
        let snapshot = harness.snapshot().unwrap();
        assert_eq!(snapshot.services, vec!["tools"]);
        assert_eq!(snapshot.plugins[0].id, "example");
    }

    #[test]
    fn content_address_is_stable() {
        assert_eq!(
            MahayanaHarness::content_address(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
