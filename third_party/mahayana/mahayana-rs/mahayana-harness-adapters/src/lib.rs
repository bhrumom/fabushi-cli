//! Concrete adapters that bind Mahayana Harness capability seams to the
//! existing Mahayana `ToolHost` and local content-addressed storage.

mod extended;
pub use extended::*;

use async_trait::async_trait;
use mahayana_harness::{HarnessError, HarnessResult};
use mahayana_harness_services::{
    CodeRuntimeProvider, CommandProvider, ContentStore, FileSystemProvider, LspProvider,
    ShellProvider, WebProvider,
};
use mahayana_tool_host::{ToolHost, ToolRequest, ToolResult};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ToolRoute {
    pub shell: String,
    pub fs_read: String,
    pub fs_write: String,
    pub fs_list: String,
    pub fs_remove: String,
    pub lsp: String,
    pub web_search: String,
    pub web_fetch: String,
    pub code_execute: String,
    pub command_execute: String,
}

impl Default for ToolRoute {
    fn default() -> Self {
        Self {
            shell: "shell".into(),
            fs_read: "fs.read".into(),
            fs_write: "fs.write".into(),
            fs_list: "fs.list".into(),
            fs_remove: "fs.remove".into(),
            lsp: "lsp.request".into(),
            web_search: "web.search".into(),
            web_fetch: "web.fetch".into(),
            code_execute: "code.execute".into(),
            command_execute: "command.execute".into(),
        }
    }
}

#[derive(Clone)]
pub struct ToolHostAdapters {
    host: Arc<dyn ToolHost>,
    routes: ToolRoute,
}

impl ToolHostAdapters {
    pub fn new(host: Arc<dyn ToolHost>) -> Self {
        Self {
            host,
            routes: ToolRoute::default(),
        }
    }

    pub fn with_routes(host: Arc<dyn ToolHost>, routes: ToolRoute) -> Self {
        Self { host, routes }
    }

    pub fn routes(&self) -> &ToolRoute {
        &self.routes
    }

    async fn invoke(&self, name: &str, arguments: Value) -> HarnessResult<ToolResult> {
        self.host
            .execute(ToolRequest {
                name: name.to_string(),
                arguments,
            })
            .await
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))
    }

    async fn invoke_value(&self, name: &str, arguments: Value) -> HarnessResult<Value> {
        let result = self.invoke(name, arguments).await?;
        if result.is_error {
            return Err(HarnessError::ToolExecution(value_message(&result.content)));
        }
        Ok(result.content)
    }
}

#[async_trait]
impl ShellProvider for ToolHostAdapters {
    async fn execute(&self, command: &str, cwd: Option<&str>) -> HarnessResult<ToolResult> {
        self.invoke(
            &self.routes.shell,
            json!({
                "command": command,
                "cwd": cwd,
            }),
        )
        .await
    }
}

#[async_trait]
impl FileSystemProvider for ToolHostAdapters {
    async fn read(&self, path: &str) -> HarnessResult<Vec<u8>> {
        let value = self
            .invoke_value(&self.routes.fs_read, json!({"path": path}))
            .await?;
        value_to_bytes(value)
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> HarnessResult<()> {
        self.invoke_value(
            &self.routes.fs_write,
            json!({
                "path": path,
                "bytes": bytes,
                "encoding": "bytes",
            }),
        )
        .await?;
        Ok(())
    }

    async fn list(&self, path: &str) -> HarnessResult<Vec<String>> {
        let value = self
            .invoke_value(&self.routes.fs_list, json!({"path": path}))
            .await?;
        let array = value.as_array().ok_or_else(|| {
            HarnessError::ToolExecution("filesystem list result is not an array".into())
        })?;
        Ok(array
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect())
    }

    async fn remove(&self, path: &str) -> HarnessResult<()> {
        self.invoke_value(&self.routes.fs_remove, json!({"path": path}))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl LspProvider for ToolHostAdapters {
    async fn request(&self, language: &str, method: &str, params: Value) -> HarnessResult<Value> {
        self.invoke_value(
            &self.routes.lsp,
            json!({
                "language": language,
                "method": method,
                "params": params,
            }),
        )
        .await
    }
}

#[async_trait]
impl WebProvider for ToolHostAdapters {
    async fn search(&self, query: &str) -> HarnessResult<Value> {
        self.invoke_value(&self.routes.web_search, json!({"query": query}))
            .await
    }

    async fn fetch(&self, url: &str) -> HarnessResult<Value> {
        self.invoke_value(&self.routes.web_fetch, json!({"url": url}))
            .await
    }
}

#[async_trait]
impl CodeRuntimeProvider for ToolHostAdapters {
    async fn execute(&self, language: &str, source: &str, input: Value) -> HarnessResult<Value> {
        self.invoke_value(
            &self.routes.code_execute,
            json!({
                "language": language,
                "source": source,
                "input": input,
            }),
        )
        .await
    }
}

#[async_trait]
impl CommandProvider for ToolHostAdapters {
    async fn execute(&self, command: &str, input: Value) -> HarnessResult<Value> {
        self.invoke_value(
            &self.routes.command_execute,
            json!({
                "command": command,
                "input": input,
            }),
        )
        .await
    }
}

#[derive(Default)]
pub struct MemoryContentStore {
    blobs: Mutex<BTreeMap<String, Vec<u8>>>,
}

#[async_trait]
impl ContentStore for MemoryContentStore {
    async fn put(&self, bytes: &[u8]) -> HarnessResult<String> {
        let id = content_id(bytes);
        self.blobs
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .insert(id.clone(), bytes.to_vec());
        Ok(id)
    }

    async fn get(&self, content_id: &str) -> HarnessResult<Option<Vec<u8>>> {
        Ok(self
            .blobs
            .lock()
            .map_err(|_| HarnessError::StatePoisoned)?
            .get(content_id)
            .cloned())
    }
}

#[derive(Debug, Clone)]
pub struct FileContentStore {
    root: PathBuf,
}

impl FileContentStore {
    pub fn new(root: impl Into<PathBuf>) -> HarnessResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, content_id: &str) -> HarnessResult<PathBuf> {
        let hash = content_id
            .strip_prefix("sha256:")
            .ok_or_else(|| HarnessError::InvalidConfig("content id must use sha256".into()))?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(HarnessError::InvalidConfig(
                "content id contains an invalid sha256 digest".into(),
            ));
        }
        Ok(self.root.join(&hash[..2]).join(hash))
    }
}

#[async_trait]
impl ContentStore for FileContentStore {
    async fn put(&self, bytes: &[u8]) -> HarnessResult<String> {
        let id = content_id(bytes);
        let path = self.path_for(&id)?;
        if path.exists() {
            return Ok(id);
        }
        let parent = path.parent().ok_or_else(|| {
            HarnessError::ToolExecution("content store path has no parent".into())
        })?;
        std::fs::create_dir_all(parent)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        std::fs::write(&tmp, bytes)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        std::fs::rename(&tmp, &path)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))?;
        Ok(id)
    }

    async fn get(&self, content_id: &str) -> HarnessResult<Option<Vec<u8>>> {
        let path = self.path_for(content_id)?;
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|error| HarnessError::ToolExecution(error.to_string()))
    }
}

fn content_id(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn value_message(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn value_to_bytes(value: Value) -> HarnessResult<Vec<u8>> {
    if let Some(text) = value.as_str() {
        return Ok(text.as_bytes().to_vec());
    }
    if let Some(bytes) = value.as_array() {
        return bytes
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|byte| u8::try_from(byte).ok())
                    .ok_or_else(|| {
                        HarnessError::ToolExecution(
                            "filesystem byte result contains a non-byte value".into(),
                        )
                    })
            })
            .collect();
    }
    Err(HarnessError::ToolExecution(
        "filesystem read result is neither text nor byte array".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mahayana_core::BuildProfile;
    use mahayana_tool_host::{ToolCapabilities, ToolError};

    struct EchoToolHost;

    #[async_trait]
    impl ToolHost for EchoToolHost {
        async fn execute(&self, request: ToolRequest) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                content: json!({
                    "name": request.name,
                    "arguments": request.arguments,
                }),
                is_error: false,
            })
        }

        fn capabilities(&self) -> ToolCapabilities {
            ToolCapabilities::for_profile(BuildProfile::DesktopFull)
        }
    }

    #[tokio::test]
    async fn shell_routes_through_existing_tool_host() {
        let adapters = ToolHostAdapters::new(Arc::new(EchoToolHost));
        let result = ShellProvider::execute(&adapters, "pwd", Some("/tmp"))
            .await
            .unwrap();
        assert_eq!(result.content["name"], "shell");
        assert_eq!(result.content["arguments"]["command"], "pwd");
        assert_eq!(result.content["arguments"]["cwd"], "/tmp");
    }

    #[tokio::test]
    async fn memory_store_is_content_addressed() {
        let store = MemoryContentStore::default();
        let id = store.put(b"hello").await.unwrap();
        assert!(id.starts_with("sha256:"));
        assert_eq!(store.get(&id).await.unwrap(), Some(b"hello".to_vec()));
    }
}
