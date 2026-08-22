use async_trait::async_trait;
use mahayana_harness::{
    HarnessError, HarnessResult, InterceptorDecision, StorageProvider, ToolDefinition,
    ToolInterceptor,
};
use mahayana_harness_services::{
    AcpProvider, CompactionProvider, CredentialProvider, CredentialReference, SubagentProvider,
    TerminalProvider, WorkflowExecutor,
};
use mahayana_tool_host::{ToolCapabilities, ToolError, ToolHost, ToolRequest, ToolResult};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{ToolHostAdapters, value_to_bytes};

const TERMINAL_OPEN: &str = "terminal.open";
const TERMINAL_WRITE: &str = "terminal.write";
const TERMINAL_READ: &str = "terminal.read";
const TERMINAL_CLOSE: &str = "terminal.close";
const CREDENTIAL_STORE: &str = "credential.store";
const CREDENTIAL_RESOLVE: &str = "credential.resolve";
const CREDENTIAL_REMOVE: &str = "credential.remove";
const COMPACTION_RUN: &str = "compaction.run";
const SUBAGENT_SPAWN: &str = "subagent.spawn";
const SUBAGENT_RESUME: &str = "subagent.resume";
const SUBAGENT_STOP: &str = "subagent.stop";
const WORKFLOW_RUN: &str = "workflow.run";
const ACP_HANDLE: &str = "acp.handle";

#[async_trait]
impl TerminalProvider for ToolHostAdapters {
    async fn open(&self, cwd: Option<&str>) -> HarnessResult<String> {
        let value = self
            .invoke_value(TERMINAL_OPEN, json!({"cwd": cwd}))
            .await?;
        value
            .get("terminalId")
            .and_then(Value::as_str)
            .or_else(|| value.get("id").and_then(Value::as_str))
            .or_else(|| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| HarnessError::ToolExecution("terminal.open returned no id".into()))
    }

    async fn write(&self, terminal_id: &str, data: &[u8]) -> HarnessResult<()> {
        self.invoke_value(
            TERMINAL_WRITE,
            json!({"terminalId": terminal_id, "bytes": data}),
        )
        .await?;
        Ok(())
    }

    async fn read(&self, terminal_id: &str) -> HarnessResult<Vec<u8>> {
        let value = self
            .invoke_value(TERMINAL_READ, json!({"terminalId": terminal_id}))
            .await?;
        if let Some(content) = value.get("content") {
            return value_to_bytes(content.clone());
        }
        value_to_bytes(value)
    }

    async fn close(&self, terminal_id: &str) -> HarnessResult<()> {
        self.invoke_value(TERMINAL_CLOSE, json!({"terminalId": terminal_id}))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl CredentialProvider for ToolHostAdapters {
    fn id(&self) -> &str {
        "mahayana-tool-host"
    }

    async fn store(&self, label: &str, secret: &[u8]) -> HarnessResult<CredentialReference> {
        let value = self
            .invoke_value(
                CREDENTIAL_STORE,
                json!({"label": label, "secretBytes": secret}),
            )
            .await?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| value.get("referenceId").and_then(Value::as_str))
            .or_else(|| value.as_str())
            .ok_or_else(|| {
                HarnessError::ToolExecution("credential.store returned no reference id".into())
            })?;
        Ok(CredentialReference {
            id: id.to_string(),
            provider: self.id().to_string(),
            label: label.to_string(),
        })
    }

    async fn resolve(&self, reference: &CredentialReference) -> HarnessResult<Vec<u8>> {
        let value = self
            .invoke_value(CREDENTIAL_RESOLVE, json!({"id": reference.id}))
            .await?;
        if let Some(secret) = value.get("secretBytes") {
            return value_to_bytes(secret.clone());
        }
        value_to_bytes(value)
    }

    async fn remove(&self, reference: &CredentialReference) -> HarnessResult<()> {
        self.invoke_value(CREDENTIAL_REMOVE, json!({"id": reference.id}))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl CompactionProvider for ToolHostAdapters {
    async fn compact(&self, session_id: &str, transcript: &str) -> HarnessResult<String> {
        let value = self
            .invoke_value(
                COMPACTION_RUN,
                json!({"sessionId": session_id, "transcript": transcript}),
            )
            .await?;
        value
            .get("summary")
            .and_then(Value::as_str)
            .or_else(|| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| HarnessError::ToolExecution("compaction.run returned no summary".into()))
    }
}

#[async_trait]
impl SubagentProvider for ToolHostAdapters {
    async fn spawn(&self, preset: &str, instruction: &str) -> HarnessResult<String> {
        let value = self
            .invoke_value(
                SUBAGENT_SPAWN,
                json!({"preset": preset, "instruction": instruction}),
            )
            .await?;
        value
            .get("agentId")
            .and_then(Value::as_str)
            .or_else(|| value.get("id").and_then(Value::as_str))
            .or_else(|| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| HarnessError::ToolExecution("subagent.spawn returned no id".into()))
    }

    async fn resume(&self, subagent_id: &str, instruction: &str) -> HarnessResult<Value> {
        self.invoke_value(
            SUBAGENT_RESUME,
            json!({"agentId": subagent_id, "instruction": instruction}),
        )
        .await
    }

    async fn stop(&self, subagent_id: &str) -> HarnessResult<()> {
        self.invoke_value(SUBAGENT_STOP, json!({"agentId": subagent_id}))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl WorkflowExecutor for ToolHostAdapters {
    async fn run(&self, workflow_id: &str, input: Value) -> HarnessResult<Value> {
        self.invoke_value(
            WORKFLOW_RUN,
            json!({"workflowId": workflow_id, "input": input}),
        )
        .await
    }
}

#[async_trait]
impl AcpProvider for ToolHostAdapters {
    async fn handle(&self, method: &str, params: Value) -> HarnessResult<Value> {
        self.invoke_value(ACP_HANDLE, json!({"method": method, "params": params}))
            .await
    }
}

#[derive(Default)]
pub struct MemoryStorageProvider {
    values: Mutex<BTreeMap<(String, String), Vec<u8>>>,
}

#[async_trait]
impl StorageProvider for MemoryStorageProvider {
    async fn get(&self, namespace: &str, key: &str) -> HarnessResult<Option<Vec<u8>>> {
        Ok(self
            .values
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn put(&self, namespace: &str, key: &str, bytes: &[u8]) -> HarnessResult<()> {
        self.values
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .insert((namespace.to_string(), key.to_string()), bytes.to_vec());
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> HarnessResult<()> {
        self.values
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .remove(&(namespace.to_string(), key.to_string()));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileStorageProvider {
    root: PathBuf,
}

impl FileStorageProvider {
    pub fn new(root: impl Into<PathBuf>) -> HarnessResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        Ok(Self { root })
    }

    fn path_for(&self, namespace: &str, key: &str) -> HarnessResult<PathBuf> {
        validate_logical_name(namespace, "namespace")?;
        validate_logical_name(key, "key")?;
        let namespace_hash = sha256(namespace.as_bytes());
        let key_hash = sha256(key.as_bytes());
        Ok(self.root.join(namespace_hash).join(key_hash))
    }
}

#[async_trait]
impl StorageProvider for FileStorageProvider {
    async fn get(&self, namespace: &str, key: &str) -> HarnessResult<Option<Vec<u8>>> {
        let path = self.path_for(namespace, key)?;
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))
    }

    async fn put(&self, namespace: &str, key: &str, bytes: &[u8]) -> HarnessResult<()> {
        let path = self.path_for(namespace, key)?;
        let parent = path.parent().ok_or_else(|| {
            HarnessError::ToolExecution("storage path has no parent directory".into())
        })?;
        std::fs::create_dir_all(parent)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&temporary, bytes)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        std::fs::rename(&temporary, &path)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> HarnessResult<()> {
        let path = self.path_for(namespace, key)?;
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub denied_tools: BTreeSet<String>,
    pub allowed_read_roots: Vec<PathBuf>,
    pub allowed_write_roots: Vec<PathBuf>,
    pub max_argument_bytes: usize,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            denied_tools: BTreeSet::new(),
            allowed_read_roots: Vec::new(),
            allowed_write_roots: Vec::new(),
            max_argument_bytes: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SandboxPolicyInterceptor {
    policy: SandboxPolicy,
}

impl SandboxPolicyInterceptor {
    pub fn new(policy: SandboxPolicy) -> Self {
        Self { policy }
    }

    fn path_allowed(path: &Path, roots: &[PathBuf]) -> bool {
        if roots.is_empty() {
            return true;
        }
        roots.iter().any(|root| path.starts_with(root))
    }
}

#[async_trait]
impl ToolInterceptor for SandboxPolicyInterceptor {
    async fn before_execute(
        &self,
        definition: &ToolDefinition,
        request: &ToolRequest,
    ) -> HarnessResult<InterceptorDecision> {
        if self.policy.denied_tools.contains(&request.name) {
            return Ok(InterceptorDecision::Reject(format!(
                "tool {} is denied by sandbox policy",
                request.name
            )));
        }
        let encoded = serde_json::to_vec(&request.arguments)
            .map_err(|error| HarnessError::InvalidConfig(error.to_string()))?;
        if encoded.len() > self.policy.max_argument_bytes {
            return Ok(InterceptorDecision::Reject(format!(
                "tool arguments exceed sandbox limit of {} bytes",
                self.policy.max_argument_bytes
            )));
        }
        let path = request
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| request.arguments.get("cwd").and_then(Value::as_str));
        if let Some(path) = path {
            let path = PathBuf::from(path);
            if has_parent_escape(&path) {
                return Ok(InterceptorDecision::Reject(
                    "sandbox path contains parent traversal".into(),
                ));
            }
            let is_write = definition
                .tags
                .iter()
                .any(|tag| matches!(tag.as_str(), "write" | "filesystem:write" | "mutating"));
            let roots = if is_write {
                &self.policy.allowed_write_roots
            } else {
                &self.policy.allowed_read_roots
            };
            if !Self::path_allowed(&path, roots) {
                return Ok(InterceptorDecision::Reject(format!(
                    "path {} is outside sandbox roots",
                    path.display()
                )));
            }
        }
        Ok(InterceptorDecision::Continue)
    }

    async fn after_execute(
        &self,
        _definition: &ToolDefinition,
        _request: &ToolRequest,
        _result: &ToolResult,
    ) -> HarnessResult<()> {
        Ok(())
    }
}

pub struct DeadlineToolHost {
    inner: Arc<dyn ToolHost>,
    timeout: Duration,
}

impl DeadlineToolHost {
    pub fn new(inner: Arc<dyn ToolHost>, timeout: Duration) -> HarnessResult<Self> {
        if timeout.is_zero() {
            return Err(HarnessError::InvalidConfig(
                "tool deadline must be greater than zero".into(),
            ));
        }
        Ok(Self { inner, timeout })
    }
}

#[async_trait]
impl ToolHost for DeadlineToolHost {
    async fn execute(&self, request: ToolRequest) -> Result<ToolResult, ToolError> {
        match tokio::time::timeout(self.timeout, self.inner.execute(request)).await {
            Ok(result) => result,
            Err(_) => Err(ToolError::Execution(format!(
                "tool execution exceeded {}ms deadline",
                self.timeout.as_millis()
            ))),
        }
    }

    fn capabilities(&self) -> ToolCapabilities {
        self.inner.capabilities()
    }
}

fn validate_logical_name(value: &str, field: &str) -> HarnessResult<()> {
    if value.trim().is_empty() {
        return Err(HarnessError::InvalidConfig(format!(
            "{field} must not be empty"
        )));
    }
    if value.contains('\0') {
        return Err(HarnessError::InvalidConfig(format!(
            "{field} contains a NUL byte"
        )));
    }
    Ok(())
}

fn has_parent_escape(path: &Path) -> bool {
    path.components()
        .any(|component| component == Component::ParentDir)
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mahayana_core::BuildProfile;
    use mahayana_harness::ToolDefinition;

    struct DelayHost;

    #[async_trait]
    impl ToolHost for DelayHost {
        async fn execute(&self, _request: ToolRequest) -> Result<ToolResult, ToolError> {
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok(ToolResult {
                content: Value::Null,
                is_error: false,
            })
        }

        fn capabilities(&self) -> ToolCapabilities {
            ToolCapabilities::for_profile(BuildProfile::DesktopFull)
        }
    }

    #[tokio::test]
    async fn deadline_wrapper_interrupts_slow_tool() {
        let host = DeadlineToolHost::new(Arc::new(DelayHost), Duration::from_millis(1)).unwrap();
        let error = host
            .execute(ToolRequest {
                name: "slow".into(),
                arguments: Value::Null,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
    }

    #[tokio::test]
    async fn memory_storage_round_trips() {
        let storage = MemoryStorageProvider::default();
        storage.put("session", "one", b"value").await.unwrap();
        assert_eq!(
            storage.get("session", "one").await.unwrap(),
            Some(b"value".to_vec())
        );
        storage.delete("session", "one").await.unwrap();
        assert_eq!(storage.get("session", "one").await.unwrap(), None);
    }

    #[tokio::test]
    async fn sandbox_rejects_parent_escape() {
        let interceptor = SandboxPolicyInterceptor::new(SandboxPolicy::default());
        let decision = interceptor
            .before_execute(
                &ToolDefinition {
                    name: "fs.read".into(),
                    description: "read".into(),
                    input_schema: Value::Null,
                    read_only: true,
                    requires_approval: false,
                    tags: vec!["filesystem:read".into()],
                },
                &ToolRequest {
                    name: "fs.read".into(),
                    arguments: json!({"path": "../secret"}),
                },
            )
            .await
            .unwrap();
        assert!(matches!(decision, InterceptorDecision::Reject(_)));
    }
}
