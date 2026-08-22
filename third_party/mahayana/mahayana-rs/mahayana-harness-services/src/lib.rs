//! Product services and capability seams for `mahayana-harness`.
//!
//! These services cover the non-loop portions of a full agent harness while
//! remaining independent from Electron/Swift/Kotlin presentation code.

use async_trait::async_trait;
use mahayana_harness::{HarnessError, HarnessResult, MahayanaHarness, RuntimeSnapshot};
use mahayana_tool_host::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptSection {
    pub id: String,
    pub priority: i32,
    pub content: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub id: String,
    pub root: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingRecord {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialReference {
    pub id: String,
    pub service: String,
    pub account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRecord {
    pub id: String,
    pub name: String,
    pub media_type: Option<String>,
    pub content_address: String,
    pub byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpillRecord {
    pub id: String,
    pub content_address: String,
    pub byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanState {
    pub session_id: String,
    pub steps: Vec<String>,
    pub review_required: bool,
    pub exited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEntry {
    pub id: String,
    pub session_id: String,
    pub instruction: String,
    pub schedule: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRecord {
    pub id: String,
    pub session_id: Option<String>,
    pub rating: Option<i32>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRecord {
    pub display_name: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub session_id: String,
    pub event_id: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub agent_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTask {
    pub id: String,
    pub assignee_agent_id: String,
    pub instruction: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMessage {
    pub id: String,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub members: Vec<TeamMember>,
    pub tasks: Vec<TeamTask>,
    pub messages: Vec<TeamMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardPolicy {
    pub max_repeat_observations: usize,
    pub max_recent_observations: usize,
}

impl Default for GuardPolicy {
    fn default() -> Self {
        Self {
            max_repeat_observations: 3,
            max_recent_observations: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRecord {
    pub name: String,
    pub description: String,
    pub tool: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextFragment {
    pub id: String,
    pub label: String,
    pub content: String,
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRequest {
    pub operation: String,
    pub terminal_id: Option<String>,
    pub data: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemRequest {
    pub operation: String,
    pub path: String,
    pub destination: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRequest {
    pub operation: String,
    pub language: Option<String>,
    pub path: Option<String>,
    pub position: Option<Value>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRequest {
    pub operation: String,
    pub query: Option<String>,
    pub url: Option<String>,
    pub options: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeRuntimeRequest {
    pub language: String,
    pub code: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionRequest {
    pub session_id: String,
    pub events: Vec<Value>,
    pub target_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRequest {
    pub parent_session_id: String,
    pub role: String,
    pub instruction: String,
    pub context: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRequest {
    pub workflow_id: String,
    pub session_id: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRequest {
    pub operation: String,
    pub payload: Value,
}

#[async_trait]
pub trait ContentStore: Send + Sync {
    async fn put(&self, bytes: &[u8]) -> HarnessResult<String>;
    async fn get(&self, address: &str) -> HarnessResult<Option<Vec<u8>>>;
}

#[async_trait]
pub trait ShellProvider: Send + Sync {
    async fn run(&self, request: ShellRequest) -> HarnessResult<ToolResult>;
}

#[async_trait]
pub trait TerminalProvider: Send + Sync {
    async fn perform(&self, request: TerminalRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait FileSystemProvider: Send + Sync {
    async fn perform(&self, request: FileSystemRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait LspProvider: Send + Sync {
    async fn perform(&self, request: LspRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait WebProvider: Send + Sync {
    async fn perform(&self, request: WebRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait CodeRuntimeProvider: Send + Sync {
    async fn run(&self, request: CodeRuntimeRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait CompactionProvider: Send + Sync {
    async fn compact(&self, request: CompactionRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait SubagentProvider: Send + Sync {
    async fn spawn(&self, request: SubagentRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait WorkflowExecutor: Send + Sync {
    async fn execute(&self, request: WorkflowRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait AcpProvider: Send + Sync {
    async fn call(&self, request: AcpRequest) -> HarnessResult<Value>;
}

#[async_trait]
pub trait CommandProvider: Send + Sync {
    async fn invoke(&self, name: &str, arguments: Value) -> HarnessResult<Value>;
}

#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn store(&self, reference: &CredentialReference, secret: &str) -> HarnessResult<()>;
    async fn resolve(&self, reference: &CredentialReference) -> HarnessResult<Option<String>>;
    async fn remove(&self, reference: &CredentialReference) -> HarnessResult<()>;
}

#[derive(Clone, Default)]
pub struct ProviderSet {
    pub content_store: Option<Arc<dyn ContentStore>>,
    pub shell: Option<Arc<dyn ShellProvider>>,
    pub terminal: Option<Arc<dyn TerminalProvider>>,
    pub fs: Option<Arc<dyn FileSystemProvider>>,
    pub lsp: Option<Arc<dyn LspProvider>>,
    pub web: Option<Arc<dyn WebProvider>>,
    pub code: Option<Arc<dyn CodeRuntimeProvider>>,
    pub compaction: Option<Arc<dyn CompactionProvider>>,
    pub subagent: Option<Arc<dyn SubagentProvider>>,
    pub workflow: Option<Arc<dyn WorkflowExecutor>>,
    pub acp: Option<Arc<dyn AcpProvider>>,
    pub command: Option<Arc<dyn CommandProvider>>,
    pub credentials: Option<Arc<dyn CredentialProvider>>,
}

#[derive(Default)]
struct ServicesState {
    prompt_sections: BTreeMap<String, PromptSection>,
    workspaces: BTreeMap<String, WorkspaceRecord>,
    settings: BTreeMap<String, SettingRecord>,
    attachments: BTreeMap<String, AttachmentRecord>,
    spills: BTreeMap<String, SpillRecord>,
    todos: BTreeMap<String, TodoItem>,
    plans: BTreeMap<String, PlanState>,
    schedules: BTreeMap<String, ScheduleEntry>,
    feedback: BTreeMap<String, FeedbackRecord>,
    identity: IdentityRecord,
    teams: BTreeMap<String, AgentTeam>,
    skills: BTreeMap<String, SkillRecord>,
    commands: BTreeMap<String, CommandRecord>,
    context_fragments: BTreeMap<String, ContextFragment>,
    recent_observations: VecDeque<String>,
}

#[derive(Clone)]
pub struct HarnessServices {
    harness: MahayanaHarness,
    providers: Arc<Mutex<ProviderSet>>,
    state: Arc<Mutex<ServicesState>>,
    guard_policy: GuardPolicy,
}

impl HarnessServices {
    pub fn new(harness: MahayanaHarness) -> Self {
        Self {
            harness,
            providers: Arc::new(Mutex::new(ProviderSet::default())),
            state: Arc::new(Mutex::new(ServicesState::default())),
            guard_policy: GuardPolicy::default(),
        }
    }

    pub fn harness(&self) -> MahayanaHarness {
        self.harness.clone()
    }

    pub fn runtime_snapshot(&self) -> HarnessResult<RuntimeSnapshot> {
        self.harness.snapshot()
    }

    pub fn snapshot(&self) -> HarnessResult<Value> {
        let state = self.lock_state()?;
        Ok(serde_json::json!({
            "runtime": self.harness.snapshot()?,
            "promptSections": state.prompt_sections.values().cloned().collect::<Vec<_>>(),
            "workspaces": state.workspaces.values().cloned().collect::<Vec<_>>(),
            "settings": state.settings.values().cloned().collect::<Vec<_>>(),
            "attachments": state.attachments.values().cloned().collect::<Vec<_>>(),
            "spills": state.spills.values().cloned().collect::<Vec<_>>(),
            "todos": state.todos.values().cloned().collect::<Vec<_>>(),
            "plans": state.plans.values().cloned().collect::<Vec<_>>(),
            "schedules": state.schedules.values().cloned().collect::<Vec<_>>(),
            "feedback": state.feedback.values().cloned().collect::<Vec<_>>(),
            "identity": state.identity.clone(),
            "teams": state.teams.values().cloned().collect::<Vec<_>>(),
            "skills": state.skills.values().cloned().collect::<Vec<_>>(),
            "commands": state.commands.values().cloned().collect::<Vec<_>>(),
            "contextFragments": state.context_fragments.values().cloned().collect::<Vec<_>>()
        }))
    }

    pub fn providers(&self) -> Arc<Mutex<ProviderSet>> {
        self.providers.clone()
    }

    pub fn set_guard_policy(&mut self, policy: GuardPolicy) {
        self.guard_policy = policy;
    }

    pub fn register_prompt_section(&self, section: PromptSection) -> HarnessResult<()> {
        self.lock_state()?
            .prompt_sections
            .insert(section.id.clone(), section);
        Ok(())
    }

    pub fn assemble_prompt(&self, base: &str) -> HarnessResult<String> {
        let state = self.lock_state()?;
        let mut sections = state
            .prompt_sections
            .values()
            .filter(|section| section.enabled)
            .cloned()
            .collect::<Vec<_>>();
        sections.sort_by_key(|section| (section.priority, section.id.clone()));

        let mut parts = Vec::with_capacity(sections.len() + 1);
        if !base.trim().is_empty() {
            parts.push(base.trim().to_string());
        }
        parts.extend(
            sections
                .into_iter()
                .map(|section| section.content.trim().to_string())
                .filter(|content| !content.is_empty()),
        );
        Ok(parts.join("\n\n"))
    }

    pub fn inject_context(&self, fragment: ContextFragment) -> HarnessResult<()> {
        self.lock_state()?
            .context_fragments
            .insert(fragment.id.clone(), fragment);
        Ok(())
    }

    pub fn context_fragments(&self) -> HarnessResult<Vec<ContextFragment>> {
        let state = self.lock_state()?;
        let mut fragments = state
            .context_fragments
            .values()
            .cloned()
            .collect::<Vec<_>>();
        fragments.sort_by_key(|fragment| (fragment.priority, fragment.id.clone()));
        Ok(fragments)
    }

    pub fn register_workspace(
        &self,
        root: impl Into<String>,
        label: impl Into<String>,
    ) -> HarnessResult<WorkspaceRecord> {
        let workspace = WorkspaceRecord {
            id: format!("ws_{}", Uuid::new_v4().simple()),
            root: root.into(),
            label: label.into(),
        };
        self.lock_state()?
            .workspaces
            .insert(workspace.id.clone(), workspace.clone());
        Ok(workspace)
    }

    pub fn workspaces(&self) -> HarnessResult<Vec<WorkspaceRecord>> {
        Ok(self.lock_state()?.workspaces.values().cloned().collect())
    }

    pub fn set_setting(
        &self,
        key: impl Into<String>,
        value: Value,
    ) -> HarnessResult<SettingRecord> {
        let key = key.into();
        let record = SettingRecord {
            key: key.clone(),
            value,
        };
        self.lock_state()?.settings.insert(key, record.clone());
        Ok(record)
    }

    pub fn get_setting(&self, key: &str) -> HarnessResult<Option<SettingRecord>> {
        Ok(self.lock_state()?.settings.get(key).cloned())
    }

    pub async fn save_attachment(
        &self,
        name: impl Into<String>,
        media_type: Option<String>,
        bytes: &[u8],
    ) -> HarnessResult<AttachmentRecord> {
        let provider = self.content_store_provider()?;
        let address = provider.put(bytes).await?;
        let record = AttachmentRecord {
            id: format!("attachment_{}", Uuid::new_v4().simple()),
            name: name.into(),
            media_type,
            content_address: address,
            byte_len: bytes.len(),
        };
        self.lock_state()?
            .attachments
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    pub async fn spill(&self, bytes: &[u8]) -> HarnessResult<SpillRecord> {
        let provider = self.content_store_provider()?;
        let address = provider.put(bytes).await?;
        let record = SpillRecord {
            id: format!("spill_{}", Uuid::new_v4().simple()),
            content_address: address,
            byte_len: bytes.len(),
        };
        self.lock_state()?
            .spills
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    pub fn add_todo(&self, text: impl Into<String>) -> HarnessResult<TodoItem> {
        let item = TodoItem {
            id: format!("todo_{}", Uuid::new_v4().simple()),
            text: text.into(),
            done: false,
        };
        self.lock_state()?
            .todos
            .insert(item.id.clone(), item.clone());
        Ok(item)
    }

    pub fn update_todo(&self, id: &str, done: bool) -> HarnessResult<TodoItem> {
        let mut state = self.lock_state()?;
        let item = state
            .todos
            .get_mut(id)
            .ok_or_else(|| HarnessError::NotFound(id.to_string()))?;
        item.done = done;
        Ok(item.clone())
    }

    pub fn set_plan(
        &self,
        session_id: impl Into<String>,
        steps: Vec<String>,
        review_required: bool,
    ) -> HarnessResult<PlanState> {
        let plan = PlanState {
            session_id: session_id.into(),
            steps,
            review_required,
            exited: false,
        };
        self.lock_state()?
            .plans
            .insert(plan.session_id.clone(), plan.clone());
        Ok(plan)
    }

    pub fn exit_plan(&self, session_id: &str, reviewed: bool) -> HarnessResult<PlanState> {
        let mut state = self.lock_state()?;
        let plan = state
            .plans
            .get_mut(session_id)
            .ok_or_else(|| HarnessError::NotFound(session_id.to_string()))?;
        if plan.review_required && !reviewed {
            return Err(HarnessError::InvalidRequest(
                "plan exit requires review".into(),
            ));
        }
        plan.exited = true;
        Ok(plan.clone())
    }

    pub fn schedule(
        &self,
        session_id: impl Into<String>,
        instruction: impl Into<String>,
        schedule: impl Into<String>,
    ) -> HarnessResult<ScheduleEntry> {
        let entry = ScheduleEntry {
            id: format!("schedule_{}", Uuid::new_v4().simple()),
            session_id: session_id.into(),
            instruction: instruction.into(),
            schedule: schedule.into(),
            enabled: true,
        };
        self.lock_state()?
            .schedules
            .insert(entry.id.clone(), entry.clone());
        Ok(entry)
    }

    pub fn set_schedule_enabled(&self, id: &str, enabled: bool) -> HarnessResult<ScheduleEntry> {
        let mut state = self.lock_state()?;
        let entry = state
            .schedules
            .get_mut(id)
            .ok_or_else(|| HarnessError::NotFound(id.to_string()))?;
        entry.enabled = enabled;
        Ok(entry.clone())
    }

    pub fn list_schedules(&self) -> HarnessResult<Vec<ScheduleEntry>> {
        Ok(self.lock_state()?.schedules.values().cloned().collect())
    }

    pub fn get_schedule(&self, schedule_id: &str) -> HarnessResult<ScheduleEntry> {
        self.lock_state()?
            .schedules
            .get(schedule_id)
            .cloned()
            .ok_or_else(|| HarnessError::ServiceNotFound(format!("schedule:{schedule_id}")))
    }

    pub fn delete_schedule(&self, schedule_id: &str) -> HarnessResult<ScheduleEntry> {
        self.lock_state()?
            .schedules
            .remove(schedule_id)
            .ok_or_else(|| HarnessError::ServiceNotFound(format!("schedule:{schedule_id}")))
    }

    pub fn record_feedback(
        &self,
        session_id: Option<String>,
        rating: Option<i32>,
        note: Option<String>,
    ) -> HarnessResult<FeedbackRecord> {
        let record = FeedbackRecord {
            id: format!("feedback_{}", Uuid::new_v4().simple()),
            session_id,
            rating,
            note,
        };
        self.lock_state()?
            .feedback
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    pub fn identity(&self) -> HarnessResult<IdentityRecord> {
        Ok(self.lock_state()?.identity.clone())
    }

    pub fn bind_identity_account(
        &self,
        account_id: Option<String>,
        display_name: Option<String>,
    ) -> HarnessResult<IdentityRecord> {
        let mut state = self.lock_state()?;
        state.identity.account_id = account_id;
        state.identity.display_name = display_name;
        Ok(state.identity.clone())
    }

    pub fn create_team(
        &self,
        name: impl Into<String>,
        members: Vec<TeamMember>,
    ) -> HarnessResult<AgentTeam> {
        let team = AgentTeam {
            id: format!("team_{}", Uuid::new_v4().simple()),
            name: name.into(),
            members,
            tasks: Vec::new(),
            messages: Vec::new(),
        };
        self.lock_state()?
            .teams
            .insert(team.id.clone(), team.clone());
        Ok(team)
    }

    pub fn add_team_task(
        &self,
        team_id: &str,
        assignee_agent_id: impl Into<String>,
        instruction: impl Into<String>,
    ) -> HarnessResult<TeamTask> {
        let mut state = self.lock_state()?;
        let team = state
            .teams
            .get_mut(team_id)
            .ok_or_else(|| HarnessError::NotFound(team_id.to_string()))?;
        let task = TeamTask {
            id: format!("team_task_{}", Uuid::new_v4().simple()),
            assignee_agent_id: assignee_agent_id.into(),
            instruction: instruction.into(),
            status: "pending".into(),
        };
        team.tasks.push(task.clone());
        Ok(task)
    }

    pub fn send_team_message(
        &self,
        team_id: &str,
        from_agent_id: impl Into<String>,
        to_agent_id: Option<String>,
        payload: Value,
    ) -> HarnessResult<TeamMessage> {
        let mut state = self.lock_state()?;
        let team = state
            .teams
            .get_mut(team_id)
            .ok_or_else(|| HarnessError::NotFound(team_id.to_string()))?;
        let message = TeamMessage {
            id: format!("team_message_{}", Uuid::new_v4().simple()),
            from_agent_id: from_agent_id.into(),
            to_agent_id,
            payload,
        };
        team.messages.push(message.clone());
        Ok(message)
    }

    pub fn register_skill(&self, skill: SkillRecord) -> HarnessResult<()> {
        self.lock_state()?.skills.insert(skill.name.clone(), skill);
        Ok(())
    }

    pub fn skills(&self) -> HarnessResult<Vec<SkillRecord>> {
        Ok(self.lock_state()?.skills.values().cloned().collect())
    }

    pub fn register_command(&self, command: CommandRecord) -> HarnessResult<()> {
        self.lock_state()?
            .commands
            .insert(command.name.clone(), command);
        Ok(())
    }

    pub fn commands(&self) -> HarnessResult<Vec<CommandRecord>> {
        Ok(self.lock_state()?.commands.values().cloned().collect())
    }

    pub fn observe(&self, value: impl Into<String>) -> HarnessResult<()> {
        let value = value.into();
        let mut state = self.lock_state()?;
        let repeats = state
            .recent_observations
            .iter()
            .filter(|item| *item == &value)
            .count();
        if repeats >= self.guard_policy.max_repeat_observations {
            return Err(HarnessError::PolicyDenied(format!(
                "guard rejected repeated observation: {value}"
            )));
        }
        state.recent_observations.push_back(value);
        while state.recent_observations.len() > self.guard_policy.max_recent_observations {
            state.recent_observations.pop_front();
        }
        Ok(())
    }

    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> HarnessResult<Vec<SessionSearchHit>> {
        let query = query.to_lowercase();
        let sessions = self.harness.sessions()?;
        let mut hits = Vec::new();
        for session in sessions {
            for event in session.events {
                let text = serde_json::to_string(&event.payload)
                    .map_err(|error| HarnessError::Serialization(error.to_string()))?;
                if event.kind.to_lowercase().contains(&query)
                    || text.to_lowercase().contains(&query)
                {
                    hits.push(SessionSearchHit {
                        session_id: session.id.clone(),
                        event_id: event.id,
                        kind: event.kind,
                        text,
                    });
                }
            }
        }
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn hash_bytes(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("sha256:{:x}", hasher.finalize())
    }

    fn content_store_provider(&self) -> HarnessResult<Arc<dyn ContentStore>> {
        self.providers
            .lock()
            .map_err(|_| HarnessError::InvalidState("provider lock poisoned".into()))?
            .content_store
            .clone()
            .ok_or_else(|| {
                HarnessError::InvalidState("content store provider is not configured".into())
            })
    }

    fn lock_state(&self) -> HarnessResult<MutexGuard<'_, ServicesState>> {
        self.state
            .lock()
            .map_err(|_| HarnessError::InvalidState("services lock poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_sections_are_deterministic() {
        let services = HarnessServices::new(MahayanaHarness::default());
        services
            .register_prompt_section(PromptSection {
                id: "b".into(),
                priority: 20,
                content: "second".into(),
                enabled: true,
            })
            .unwrap();
        services
            .register_prompt_section(PromptSection {
                id: "a".into(),
                priority: 10,
                content: "first".into(),
                enabled: true,
            })
            .unwrap();
        assert_eq!(
            services.assemble_prompt("base").unwrap(),
            "base\n\nfirst\n\nsecond"
        );
    }

    #[test]
    fn session_search_finds_payload_text() {
        let harness = MahayanaHarness::default();
        let session = harness.create_session("search").unwrap();
        harness
            .append_session_event(
                &session.id,
                "message/user",
                serde_json::json!({ "text": "lotus sutra" }),
            )
            .unwrap();
        let services = HarnessServices::new(harness);
        let hits = services.search_sessions("lotus", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, session.id);
    }

    #[test]
    fn repeated_observations_are_guarded() {
        let services = HarnessServices::new(MahayanaHarness::default());
        services.observe("same").unwrap();
        services.observe("same").unwrap();
        services.observe("same").unwrap();
        assert!(services.observe("same").is_err());
    }
}
