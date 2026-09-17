use futures_util::{SinkExt, StreamExt};
use mahayana_product::{MahayanaProductClient, default_mahayana_home};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File, OpenOptions};
#[cfg(windows)]
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use uuid::Uuid;

const OFFICIAL_DEVICE_GATEWAY_URL: &str = "wss://fabushi-mcp.ombhrum.com/agent";
const SESSION_POLL_SECONDS: u64 = 3;
const HEARTBEAT_SECONDS: u64 = 20;
const MAX_COMMAND_CHARS: usize = 64 * 1024;
const MAX_TEXT_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

fn agent_root() -> PathBuf {
    default_mahayana_home().join("device-agent")
}

fn lock_path() -> PathBuf {
    agent_root().join("agent.lock")
}

fn pid_path() -> PathBuf {
    agent_root().join("agent.pid")
}

fn stop_path() -> PathBuf {
    agent_root().join("stop-requested")
}

fn log_path() -> PathBuf {
    agent_root().join("agent.log")
}

fn device_id_path() -> PathBuf {
    default_mahayana_home().join("device-id")
}

fn valid_device_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect()
}

fn private_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(contents.as_bytes())
        .map_err(|error| error.to_string())
}

pub fn current_device_id() -> Result<String, String> {
    if let Some(explicit) = env::var("DEVICE_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| valid_device_id(value))
    {
        return Ok(explicit);
    }
    if env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true") {
        if let Some(run_id) = env::var("GITHUB_RUN_ID").ok().filter(|value| !value.is_empty()) {
            let attempt = env::var("GITHUB_RUN_ATTEMPT").unwrap_or_else(|_| "1".into());
            let job = sanitize_component(&env::var("GITHUB_JOB").unwrap_or_else(|_| "job".into()));
            let value = format!("gha-{run_id}-{attempt}-{job}-cli");
            return Ok(value.chars().take(128).collect());
        }
    }
    let path = device_id_path();
    if let Ok(value) = fs::read_to_string(&path) {
        let value = value.trim();
        if valid_device_id(value) {
            return Ok(value.to_string());
        }
    }
    let value = format!("mahayana-cli-{}", Uuid::new_v4().simple());
    private_file(&path, &format!("{value}\n"))?;
    Ok(value)
}

fn device_name(device_id: &str) -> String {
    if let Some(name) = env::var("DEVICE_NAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return name.chars().take(200).collect();
    }
    let host = env::var("HOSTNAME")
        .or_else(|_| env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| device_id.to_string());
    format!("Mahayana CLI on {}", host.trim())
        .chars()
        .take(200)
        .collect()
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(windows)]
    {
        // `tasklist` can occasionally block for minutes on Windows runners while
        // endpoint/security services initialise. Device start/status must never
        // inherit that unbounded wait because one-line installation calls it.
        let mut child = match Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return false,
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return false;
                    }
                    let mut stdout = String::new();
                    if let Some(mut pipe) = child.stdout.take() {
                        let _ = pipe.read_to_string(&mut stdout);
                    }
                    return stdout.contains(&pid.to_string());
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }
}

fn running_pid() -> Option<u32> {
    fs::read_to_string(pid_path())
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|pid| process_is_running(*pid))
}

fn claim_agent_lock() -> Result<Option<File>, String> {
    fs::create_dir_all(agent_root()).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    match options.open(lock_path()) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if running_pid().is_some() {
                return Ok(None);
            }
            // A hard crash can leave the create-new sentinel behind. No live
            // recorded agent owns it, so clear only the stale sentinel and retry.
            let _ = fs::remove_file(lock_path());
            let _ = fs::remove_file(pid_path());
            let mut retry = OpenOptions::new();
            retry.create_new(true).write(true);
            match retry.open(lock_path()) {
                Ok(file) => Ok(Some(file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
                Err(error) => Err(error.to_string()),
            }
        }
        Err(error) => Err(error.to_string()),
    }
}

pub fn status() -> Result<Value, String> {
    let running = running_pid().is_some();
    Ok(json!({
        "running": running,
        "deviceId": current_device_id()?,
        "ephemeral": env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true"),
        "gateway": OFFICIAL_DEVICE_GATEWAY_URL,
        "pid": fs::read_to_string(pid_path()).ok().map(|value| value.trim().to_string()),
        "log": log_path(),
    }))
}

pub fn ensure_started() -> Result<Value, String> {
    if status()?.get("running").and_then(Value::as_bool) == Some(true) {
        return status();
    }
    fs::create_dir_all(agent_root()).map_err(|error| error.to_string())?;
    let _ = fs::remove_file(stop_path());
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .map_err(|error| error.to_string())?;
    let stderr = stdout.try_clone().map_err(|error| error.to_string())?;
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg("device")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    // The background agent inherits only ordinary runtime context. Login-only
    // credentials must never survive as daemon environment variables.
    for (key, _) in env::vars_os() {
        let upper = key.to_string_lossy().to_ascii_uppercase();
        if upper.starts_with("FABUSHI_CI_TEST_")
            || upper.contains("PASSWORD")
            || upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("API_KEY")
        {
            command.env_remove(key);
        }
    }
    command.env("DEVICE_ID", current_device_id()?);
    let child = command.spawn().map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_millis(250));
    let mut result = status()?;
    result["spawnedPid"] = json!(child.id());
    Ok(result)
}

pub fn request_stop() -> Result<Value, String> {
    fs::create_dir_all(agent_root()).map_err(|error| error.to_string())?;
    private_file(&stop_path(), "stop\n")?;
    Ok(json!({"stopRequested": true, "deviceId": current_device_id()?}))
}

pub fn serve() -> Result<(), String> {
    let Some(_lock) = claim_agent_lock()? else {
        return Ok(());
    };
    let _ = fs::remove_file(stop_path());
    private_file(&pid_path(), &format!("{}\n", std::process::id()))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let result = runtime.block_on(run_agent_loop());
    let _ = fs::remove_file(pid_path());
    let _ = fs::remove_file(lock_path());
    result
}

async fn run_agent_loop() -> Result<(), String> {
    let client = MahayanaProductClient::default();
    loop {
        if stop_path().exists() {
            let _ = fs::remove_file(stop_path());
            return Ok(());
        }
        match client.device_agent_session() {
            Ok(session) => {
                if let Err(error) = connect_session(&client, session).await {
                    eprintln!("Mahayana device agent reconnecting: {error}");
                }
            }
            Err(_) => tokio::time::sleep(Duration::from_secs(SESSION_POLL_SECONDS)).await,
        }
    }
}

fn session_string(session: &Value, key: &str) -> Result<String, String> {
    session
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("device session is missing {key}"))
}

fn tool_descriptors() -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "vps_status",
            "title": "Device status",
            "description": "Return non-sensitive runtime status for this Mahayana CLI device.",
            "inputSchema": {"type":"object","properties":{},"additionalProperties":false},
            "outputSchema": {"type":"object"}
        }),
        json!({
            "name": "run_shell_command",
            "title": "Run shell command",
            "description": "Run a non-sensitive shell command on the signed-in Mahayana CLI device.",
            "inputSchema": {
                "type":"object",
                "properties": {
                    "command":{"type":"string","minLength":1,"maxLength":65536},
                    "cwd":{"type":"string","maxLength":4096}
                },
                "required":["command"],
                "additionalProperties":false
            },
            "outputSchema": {"type":"object"}
        }),
        json!({
            "name": "write_text_file",
            "title": "Write text file",
            "description": "Write a bounded UTF-8 text file on the signed-in Mahayana CLI device.",
            "inputSchema": {
                "type":"object",
                "properties": {
                    "path":{"type":"string","minLength":1,"maxLength":4096},
                    "content":{"type":"string","maxLength":2097152},
                    "createParents":{"type":"boolean"}
                },
                "required":["path","content"],
                "additionalProperties":false
            },
            "outputSchema": {"type":"object"}
        })
    ];
    if env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true") {
        tools.push(json!({
            "name":"ci_session_finish",
            "title":"Finish ephemeral CI session",
            "description":"Finish this ephemeral GitHub Actions device after remote validation.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
            "outputSchema":{"type":"object"}
        }));
    }
    tools
}

fn tool_schema_version(tools: &[Value]) -> String {
    let bytes = serde_json::to_vec(tools).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

async fn connect_session(client: &MahayanaProductClient, session: Value) -> Result<(), String> {
    // The workspace intentionally contains dependencies that enable both Rustls
    // crypto backends. Select one explicitly before tokio-tungstenite builds a
    // TLS client so standalone CLI/device-agent startup is deterministic.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let access_token = session_string(&session, "accessToken")?;
    let device_id = session_string(&session, "deviceId")?;
    let session_id = session_string(&session, "sessionId")?;
    if !valid_device_id(&device_id) {
        return Err("device session contains an invalid deviceId".into());
    }
    let mut request = OFFICIAL_DEVICE_GATEWAY_URL
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .map_err(|error| error.to_string())?,
    );
    let (socket, _) = connect_async(request)
        .await
        .map_err(|error| format!("official gateway connection failed: {error}"))?;
    let (mut sink, mut stream) = socket.split();
    let (outgoing, mut outbound) = mpsc::channel::<Message>(64);
    let writer = tokio::spawn(async move {
        while let Some(message) = outbound.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });
    let tools = tool_descriptors();
    let capabilities = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let ephemeral = env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true");
    let metadata = if ephemeral {
        json!({
            "kind":"github-actions",
            "repository": env::var("GITHUB_REPOSITORY").ok(),
            "workflow": env::var("GITHUB_WORKFLOW").ok(),
            "job": env::var("GITHUB_JOB").ok(),
            "runId": env::var("GITHUB_RUN_ID").ok(),
            "runAttempt": env::var("GITHUB_RUN_ATTEMPT").ok(),
            "sha": env::var("GITHUB_SHA").ok(),
            "runnerName": env::var("RUNNER_NAME").ok(),
            "runnerOs": env::var("RUNNER_OS").ok(),
            "runnerArch": env::var("RUNNER_ARCH").ok()
        })
    } else {
        json!({"kind":"mahayana-cli"})
    };
    let registration = json!({
        "type":"register",
        "deviceId": device_id,
        "name": device_name(&device_id),
        "platform": env::consts::OS,
        "capabilities": capabilities,
        "tools": tools,
        "toolSchemaVersion": tool_schema_version(&tool_descriptors()),
        "generation": Uuid::new_v4().simple().to_string(),
        "leaseSeconds": if ephemeral { 7200 } else { 14400 },
        "metadata": metadata
    });
    outgoing
        .send(Message::Text(registration.to_string().into()))
        .await
        .map_err(|_| "device gateway writer stopped".to_string())?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECONDS));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut registered = false;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if stop_path().exists() {
                    let _ = outgoing.send(Message::Close(None)).await;
                    break;
                }
                let still_current = client.device_agent_session().ok().is_some_and(|current| {
                    current.get("accessToken") == Some(&Value::String(access_token.clone()))
                        && current.get("deviceId") == Some(&Value::String(device_id.clone()))
                        && current.get("sessionId") == Some(&Value::String(session_id.clone()))
                });
                if !still_current {
                    let _ = outgoing.send(Message::Close(None)).await;
                    break;
                }
                if registered {
                    let _ = outgoing.send(Message::Text(json!({"type":"heartbeat","at": now_millis()}).to_string().into())).await;
                }
            }
            incoming = stream.next() => {
                let Some(incoming) = incoming else { break; };
                let message = incoming.map_err(|error| error.to_string())?;
                match message {
                    Message::Text(text) => {
                        let Ok(value) = serde_json::from_str::<Value>(&text) else { continue; };
                        match value.get("type").and_then(Value::as_str) {
                            Some("registered") => {
                                registered = true;
                                println!("Mahayana controllable device online: {device_id}");
                            }
                            Some("call") => {
                                let request_id = value.get("requestId").and_then(Value::as_str).unwrap_or_default().to_string();
                                let tool_name = value.get("toolName").and_then(Value::as_str).unwrap_or_default().to_string();
                                let arguments = value.get("arguments").cloned().unwrap_or_else(|| json!({}));
                                if request_id.is_empty() || tool_name.is_empty() { continue; }
                                let sender = outgoing.clone();
                                tokio::spawn(async move {
                                    let result = tokio::task::spawn_blocking(move || execute_tool(&tool_name, arguments)).await;
                                    let response = match result {
                                        Ok(Ok(value)) => json!({"type":"result","requestId":request_id,"ok":true,"result":mcp_success(value)}),
                                        Ok(Err(error)) => json!({"type":"result","requestId":request_id,"ok":false,"error":error}),
                                        Err(error) => json!({"type":"result","requestId":request_id,"ok":false,"error":error.to_string()}),
                                    };
                                    let _ = sender.send(Message::Text(response.to_string().into())).await;
                                });
                            }
                            _ => {}
                        }
                    }
                    Message::Ping(payload) => {
                        let _ = outgoing.send(Message::Pong(payload)).await;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    drop(outgoing);
    writer.abort();
    Ok(())
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn mcp_success(value: Value) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    json!({
        "content":[{"type":"text","text":text}],
        "structuredContent":value
    })
}

fn truncate_bytes(bytes: &[u8]) -> String {
    let slice = if bytes.len() > MAX_OUTPUT_BYTES {
        &bytes[..MAX_OUTPUT_BYTES]
    } else {
        bytes
    };
    let mut value = String::from_utf8_lossy(slice).into_owned();
    if bytes.len() > MAX_OUTPUT_BYTES {
        value.push_str("\n<output truncated>");
    }
    value
}

fn execute_tool(tool_name: &str, arguments: Value) -> Result<Value, String> {
    match tool_name {
        "vps_status" => Ok(json!({
            "hostname": env::var("HOSTNAME").or_else(|_| env::var("COMPUTERNAME")).unwrap_or_else(|_| "unknown".into()),
            "platform": env::consts::OS,
            "arch": env::consts::ARCH,
            "pid": std::process::id(),
            "cwd": env::current_dir().ok(),
            "deviceId": current_device_id()?,
            "ephemeral": env::var("GITHUB_ACTIONS").ok().as_deref() == Some("true")
        })),
        "run_shell_command" => run_shell_command(arguments),
        "write_text_file" => write_text_file(arguments),
        "ci_session_finish" => finish_ci_session(),
        _ => Err(format!("unsupported Mahayana CLI device tool: {tool_name}")),
    }
}

fn run_shell_command(arguments: Value) -> Result<Value, String> {
    let command_text = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_COMMAND_CHARS)
        .ok_or_else(|| "command must be a non-empty bounded string".to_string())?;
    let cwd = arguments
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if cwd.as_ref().is_some_and(|path| !path.is_dir()) {
        return Err("cwd must reference an existing directory".into());
    }
    let start = Instant::now();
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", command_text]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.args(["-lc", command_text]);
        command
    };
    if let Some(cwd) = cwd.as_ref() {
        command.current_dir(cwd);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    Ok(json!({
        "command": command_text,
        "cwd": cwd,
        "status": if output.status.success() { "completed" } else { "failed" },
        "exitCode": output.status.code(),
        "durationMs": start.elapsed().as_millis(),
        "stdout": truncate_bytes(&output.stdout),
        "stderr": truncate_bytes(&output.stderr),
        "truncated": output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_OUTPUT_BYTES
    }))
}


fn finish_ci_session() -> Result<Value, String> {
    if env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true") {
        return Err("ci_session_finish is available only on an ephemeral GitHub Actions device".into());
    }
    let path = env::var("FABUSHI_CI_FINISH_FILE")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "the CI finish file is not configured".to_string())?;
    private_file(&path, &json!({"finished":true,"at":now_millis()}).to_string())?;
    Ok(json!({"finished":true,"deviceId":current_device_id()?}))
}

fn write_text_file(arguments: Value) -> Result<Value, String> {
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .map(PathBuf::from)
        .ok_or_else(|| "path must be a non-empty bounded string".to_string())?;
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "content must be UTF-8 text".to_string())?;
    if content.len() > MAX_TEXT_FILE_BYTES {
        return Err("content exceeds the 2 MiB device write limit".into());
    }
    let create_parents = arguments
        .get("createParents")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if create_parents {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    fs::write(&path, content.as_bytes()).map_err(|error| error.to_string())?;
    Ok(json!({"path":path,"bytes":content.len(),"written":true}))
}
