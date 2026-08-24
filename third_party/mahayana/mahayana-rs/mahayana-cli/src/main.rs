use clap::Args;
use clap::Parser;
use clap::Subcommand;
#[cfg(feature = "codex-compat")]
use codex_cli::plugin_cmd::PluginCli;
#[cfg(feature = "codex-compat")]
use codex_cli::plugin_cmd::PluginSubcommand;
use mahayana_feature_host::FeatureHostController;
use mahayana_host::HostCreateConfig;
use mahayana_host::default_automation_path;
use mahayana_host::default_product_session_path;
use mahayana_host::default_product_surface_path;
use mahayana_host_protocol::AutomationTrigger;
use mahayana_host_protocol::FeatureCommand;
use mahayana_host_protocol::HostConfig;
use mahayana_host_protocol::HostEvent;
use mahayana_host_protocol::HostMode;
use mahayana_host_protocol::ListenerPlatform;
use mahayana_host_protocol::SurfacePlatform;
use mahayana_platform_core::HostPlatform;
use mahayana_plugin_host::LocalPlugin;
use mahayana_plugin_runtime::{ArtifactFormat, ArtifactResolver, ExternalReleaseManifest};
use mahayana_product::MahayanaProductClient;
use mahayana_product::default_mahayana_home;
use mahayana_product::redact_secrets;
use mahayana_runtime::mahayana_runtime_close;
use mahayana_runtime::mahayana_runtime_create;
use mahayana_runtime::mahayana_runtime_execute;
use mahayana_runtime::mahayana_runtime_free_string;
use mahayana_runtime::mahayana_runtime_interrupt;
use mahayana_runtime::mahayana_runtime_last_error;
use mahayana_runtime::mahayana_runtime_receive;
use mahayana_runtime::mahayana_runtime_resolve_approval;
use plugin_archive::pack_plugin_bundle_tar_gz;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::io::{self};
use std::os::raw::c_char;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

mod chat_tui;
mod plugin_archive;
mod plugin_dev;
mod plugin_dev_template;

#[derive(Debug, Parser)]
#[command(
    name = "mahayana",
    version,
    about = "大乘 CLI：智能编程代理、插件、MCP 与 Mini App 统一宿主"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Args)]
struct EmbeddedAgentArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<OsString>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// 登录大乘账号。
    #[command(
        after_help = "登录方式：\n  mahayana login alipay\n  mahayana login password <用户名>\n  mahayana login test [--token-stdin]"
    )]
    Login {
        #[arg(trailing_var_arg = true, value_name = "METHOD_OR_ARGUMENT")]
        args: Vec<String>,
    },
    /// 注册大乘账号。
    Register {
        #[arg(trailing_var_arg = true, value_name = "ARGUMENT")]
        args: Vec<String>,
    },
    /// 向邮箱发送验证码。
    SendCode { email: String },
    /// 退出当前大乘账号。
    Logout,
    /// 查看当前大乘账号认证状态。
    Auth,
    /// 查看服务端权威的模型 Token 用量与剩余额度。
    Usage,
    /// 查看大乘运行时状态。
    Status,
    /// 列出会话联系人。
    #[command(alias = "list")]
    Contacts,
    /// 列出或调用联系人、机器人、插件、小程序和应用能力。
    Capability {
        #[command(subcommand)]
        command: CapabilityCommand,
    },
    /// 管理 Fabushi 连接器账户与工具。
    Connector {
        #[command(subcommand)]
        command: ConnectorCommand,
    },
    /// 管理私有和团队 Skill。
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    /// 管理可见/隐藏机器人。
    Bot {
        #[command(subcommand)]
        command: BotCommand,
    },
    /// 管理外部事件监听器。
    Listener {
        #[command(subcommand)]
        command: ListenerCommand,
    },
    /// 管理计划或事件驱动的自动化例程。
    #[command(alias = "automation")]
    Routine {
        #[command(subcommand)]
        command: RoutineCommand,
    },
    /// 查看指定会话的消息历史。
    History { conversation_id: String },
    /// 向指定会话发送消息。
    Send {
        conversation_id: String,
        #[arg(required = true, trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// 打开交互式聊天，可选继续指定会话。
    Chat { conversation_id: Option<String> },
    /// 非交互运行大乘智能编程代理。
    #[command(visible_alias = "e")]
    Exec(EmbeddedAgentArgs),
    /// 非交互执行代码审查。
    Review(EmbeddedAgentArgs),
    /// 管理外部 MCP 服务。
    Mcp(EmbeddedAgentArgs),
    /// 以 stdio 启动大乘 MCP 服务。
    McpServer(EmbeddedAgentArgs),
    /// 启动或管理大乘 App Server。
    AppServer(EmbeddedAgentArgs),
    /// 管理支持远程控制的 App Server。
    RemoteControl(EmbeddedAgentArgs),
    /// 启动大乘桌面应用。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    App(EmbeddedAgentArgs),
    /// 生成 Shell 自动补全脚本。
    Completion(EmbeddedAgentArgs),
    /// 更新大乘 CLI。
    Update(EmbeddedAgentArgs),
    /// 诊断大乘 CLI、配置、认证和运行环境。
    Doctor(EmbeddedAgentArgs),
    /// 在大乘提供的沙箱中运行命令。
    Sandbox(EmbeddedAgentArgs),
    /// 调试工具。
    Debug(EmbeddedAgentArgs),
    /// 应用智能代理最近生成的补丁。
    #[command(visible_alias = "a")]
    Apply(EmbeddedAgentArgs),
    /// 恢复以前的交互会话。
    Resume(EmbeddedAgentArgs),
    /// 归档已保存的会话。
    Archive(EmbeddedAgentArgs),
    /// 永久删除已保存的会话。
    Delete(EmbeddedAgentArgs),
    /// 取消归档已保存的会话。
    Unarchive(EmbeddedAgentArgs),
    /// 从以前的会话创建分支。
    Fork(EmbeddedAgentArgs),
    /// 浏览云端任务并在本地应用修改。
    #[command(alias = "cloud-tasks")]
    Cloud(EmbeddedAgentArgs),
    /// 启动独立 Exec Server 服务。
    ExecServer(EmbeddedAgentArgs),
    /// 查看和管理功能开关。
    Features(EmbeddedAgentArgs),
    #[command(hide = true)]
    Execpolicy(EmbeddedAgentArgs),
    #[command(hide = true, name = "responses-api-proxy")]
    ResponsesApiProxy(EmbeddedAgentArgs),
    #[command(hide = true, name = "stdio-to-uds")]
    StdioToUds(EmbeddedAgentArgs),
    /// 管理和运行大乘 Mini App。
    Miniapp {
        #[arg(required = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// 浏览和安装大乘市场内容。
    Marketplace {
        #[command(subcommand)]
        command: MarketplaceCommand,
    },
    /// 创建、安装、验证和发布大乘插件。
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// 查看和管理大乘钱包。
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    /// 查看和恢复大乘购买记录。
    Purchases {
        #[command(subcommand)]
        command: PurchasesCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CapabilityCommand {
    /// 列出共享能力；可按标题、稳定 ID、mention 或描述搜索。
    List {
        #[arg(long, short = 'q')]
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// 通过稳定 capability ID 或 @mention 调用能力。
    Invoke {
        capability_id: String,
        #[arg(required = true, trailing_var_arg = true)]
        text: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ConnectorCommand {
    /// 列出真实 Codex MCP 连接状态、账户与工具。
    List,
    /// 打开连接器授权流程；本地 Git 直接连接。
    Connect {
        connector_id: String,
        #[arg(long)]
        label: Option<String>,
    },
    /// 重命名一个本地账户标签。
    Rename {
        connector_id: String,
        account_id: String,
        label: String,
    },
    /// 移除一个账户；独立 MCP OAuth 会同步删除本地令牌。
    Remove {
        connector_id: String,
        account_id: String,
    },
    /// 启用或禁用一个已发现工具。
    Tool {
        connector_id: String,
        tool_id: String,
        enabled: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    List {
        #[arg(long)]
        agent_id: Option<String>,
    },
    /// 创建或更新私有 Skill。
    Upsert {
        #[arg(long)]
        id: Option<String>,
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        use_when: String,
        #[arg(long)]
        instructions: String,
        #[arg(long)]
        owner_agent_id: Option<String>,
    },
    Delete {
        id: String,
    },
    Publish {
        id: String,
        team_id: String,
    },
    Unpublish {
        id: String,
    },
    Sync {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum BotCommand {
    List,
    Hide { id: String },
    Show { id: String },
}

#[derive(Debug, Subcommand)]
enum ListenerCommand {
    List,
    Connect { platform: String },
}

#[derive(Debug, Subcommand)]
enum RoutineCommand {
    List,
    Create {
        name: String,
        #[arg(long)]
        prompt: String,
        #[arg(long, default_value = "@daily")]
        schedule: String,
        /// Event source: slack/github/git/teams/linear/sentry/pagerduty.
        #[arg(long)]
        source: Option<String>,
        /// Event name used with --source, for example pull_request.opened.
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        disabled: bool,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Run {
        id: String,
    },
    Delete {
        id: String,
    },
    /// Inject a local event into matching event-driven routines. This is the
    /// CLI/webhook bridge for platforms whose hosted listener service is not
    /// available in this build.
    TriggerEvent {
        source: String,
        event: String,
        #[arg(long)]
        payload: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum MarketplaceCommand {
    Browse,
    Search {
        query: String,
    },
    /// Download an approved plugin archive after exact version, size, and SHA-256 verification.
    Download {
        plugin_id: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// Download, verify, and safely install an approved plugin.
    Install {
        plugin_id: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, default_value = ".")]
        repository: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Non-destructively add an MCP plugin under .agents/plugins/plugins/<name>.
    Init {
        name: String,
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, value_enum, default_value_t = plugin_dev::PluginTemplate::Conversational)]
        profile: plugin_dev::PluginTemplate,
    },
    List {
        #[arg(long, short = 'm')]
        marketplace: Option<String>,
        #[arg(long)]
        available: bool,
        #[arg(long)]
        json: bool,
    },
    Info {
        plugin_id: String,
    },
    #[command(alias = "add")]
    Install {
        plugin: String,
        #[arg(long, short = 'm')]
        marketplace: Option<String>,
        #[arg(long)]
        json: bool,
        /// Permit bundled stdio/local runtimes after source validation.
        #[arg(long)]
        allow_local: bool,
    },
    Update {
        plugin: String,
        #[arg(long, short = 'm')]
        marketplace: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(alias = "remove")]
    Uninstall {
        plugin: String,
        #[arg(long, short = 'm')]
        marketplace: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// 管理兼容的插件市场来源。
    Marketplace(EmbeddedAgentArgs),
    Open {
        plugin_id: String,
    },
    Run {
        plugin_id: String,
        command: String,
        #[arg(long, value_name = "ARGUMENTS")]
        json: Option<String>,
    },
    Validate {
        path: PathBuf,
    },
    /// Validate a plugin and execute its declared local test suite.
    Test {
        path: PathBuf,
    },
    Pack {
        path: PathBuf,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    Publish {
        path: PathBuf,
        #[arg(long)]
        plugin_id: String,
        #[arg(long)]
        version: String,
        /// External artifact source as JSON. Examples:
        /// {"type":"github-release","repository":"owner/repo","tag":"v1.0.0","asset":"plugin.tar.gz"}
        /// {"type":"npm","package":"@scope/plugin","version":"1.0.0"}
        /// {"type":"https","url":"https://cdn.example/plugin.tar.gz"}
        #[arg(long, value_name = "JSON")]
        artifact_source: String,
    },
}

#[derive(Debug, Subcommand)]
enum WalletCommand {
    Balance,
    History,
    TopUp {
        sku: String,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PurchasesCommand {
    List,
    Restore,
}

fn main() {
    #[cfg(feature = "codex-compat")]
    {
        let raw_args = std::env::args_os().collect::<Vec<_>>();
        if should_run_embedded_agent_cli(raw_args.get(1..).unwrap_or_default()) {
            if let Err(error) = codex_cli::run_multitool_with_args(raw_args) {
                eprintln!("错误：{error}");
                std::process::exit(1);
            }
            return;
        }
    }

    #[cfg(feature = "codex-compat")]
    let arg0_guard = codex_arg0::arg0_dispatch();
    #[cfg(feature = "codex-compat")]
    let codex_executable_path = arg0_guard
        .as_ref()
        .and_then(|guard| guard.paths().codex_self_exe.as_deref());
    #[cfg(not(feature = "codex-compat"))]
    let codex_executable_path: Option<&Path> = None;
    if let Err(error) = run(codex_executable_path, Cli::parse()) {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}

#[cfg(any(feature = "codex-compat", test))]
fn should_run_embedded_agent_cli(args: &[OsString]) -> bool {
    let Some(command) = args.first().and_then(|value| value.to_str()) else {
        return false;
    };
    if command == "help" {
        return args
            .get(1)
            .and_then(|value| value.to_str())
            .is_some_and(|command| !is_product_command(command));
    }
    !matches!(command, "-h" | "--help" | "-V" | "--version") && !is_product_command(command)
}

#[cfg(any(feature = "codex-compat", test))]
fn is_product_command(command: &str) -> bool {
    matches!(
        command,
        "login"
            | "register"
            | "send-code"
            | "logout"
            | "auth"
            | "usage"
            | "status"
            | "contacts"
            | "list"
            | "history"
            | "send"
            | "chat"
            | "connector"
            | "skill"
            | "bot"
            | "listener"
            | "routine"
            | "automation"
            | "miniapp"
            | "marketplace"
            | "plugin"
            | "wallet"
            | "purchases"
    )
}

#[cfg(feature = "codex-compat")]
fn run_embedded_agent_command(
    command_path: &[&str],
    EmbeddedAgentArgs { args }: EmbeddedAgentArgs,
) -> Result<(), String> {
    let mut argv = Vec::with_capacity(command_path.len() + args.len() + 1);
    argv.push(OsString::from("mahayana"));
    argv.extend(command_path.iter().map(OsString::from));
    argv.extend(args);
    codex_cli::run_multitool_with_args(argv).map_err(|error| error.to_string())
}

#[cfg(not(feature = "codex-compat"))]
fn run_embedded_agent_command(
    command_path: &[&str],
    _args: EmbeddedAgentArgs,
) -> Result<(), String> {
    Err(format!(
        "mahayana {} requires the optional codex-compat adapter in this build",
        command_path.join(" ")
    ))
}

fn run(codex_executable_path: Option<&Path>, cli: Cli) -> Result<(), String> {
    match cli.command {
        Some(CliCommand::Login { args }) => login(args),
        Some(CliCommand::Register { args }) => register(args),
        Some(CliCommand::SendCode { email }) => send_verification_code(vec![email]),
        Some(CliCommand::Logout) => product_command("mahayana.auth.logout", json!({})),
        Some(CliCommand::Auth) => product_command("mahayana.auth.status", json!({})),
        Some(CliCommand::Usage) => model_usage_command(),
        Some(CliCommand::Status) => with_runtime(codex_executable_path, |runtime| {
            print_json(&runtime.execute(json!({"@type": "mahayana.runtime.status"}))?)
        }),
        Some(CliCommand::Contacts) => with_runtime(codex_executable_path, |runtime| {
            let response = runtime.execute(json!({"@type": "mahayana.conversation.list"}))?;
            print_conversations(&response)
        }),
        Some(CliCommand::Capability { command }) => {
            with_runtime(codex_executable_path, |runtime| {
                capability_command(runtime, command)
            })
        }
        Some(CliCommand::Connector { command }) => {
            feature_surface_command(codex_executable_path, |controller| {
                connector_command(controller, command)
            })
        }
        Some(CliCommand::Skill { command }) => {
            feature_surface_command(codex_executable_path, |controller| {
                skill_command(controller, command)
            })
        }
        Some(CliCommand::Bot { command }) => {
            feature_surface_command(codex_executable_path, |controller| {
                bot_command(controller, command)
            })
        }
        Some(CliCommand::Listener { command }) => {
            feature_surface_command(codex_executable_path, |controller| {
                listener_command(controller, command)
            })
        }
        Some(CliCommand::Routine { command }) => {
            feature_surface_command(codex_executable_path, |controller| {
                routine_command(controller, command)
            })
        }
        Some(CliCommand::History { conversation_id }) => {
            with_runtime(codex_executable_path, |runtime| {
                let response = runtime.execute(json!({
                    "@type": "mahayana.conversation.history",
                    "conversationId": conversation_id,
                    "limit": 100,
                }))?;
                print_history(&response)
            })
        }
        Some(CliCommand::Send {
            conversation_id,
            text,
        }) => {
            let text = text.join(" ");
            with_runtime(codex_executable_path, |runtime| {
                send_and_stream(runtime, &conversation_id, &text)
            })
        }
        Some(CliCommand::Chat { conversation_id }) => {
            with_runtime(codex_executable_path, |runtime| {
                interactive_chat(runtime, conversation_id)
            })
        }
        Some(CliCommand::Exec(args)) => run_embedded_agent_command(&["exec"], args),
        Some(CliCommand::Review(args)) => run_embedded_agent_command(&["review"], args),
        Some(CliCommand::Mcp(args)) => run_embedded_agent_command(&["mcp"], args),
        Some(CliCommand::McpServer(args)) => run_embedded_agent_command(&["mcp-server"], args),
        Some(CliCommand::AppServer(args)) => run_embedded_agent_command(&["app-server"], args),
        Some(CliCommand::RemoteControl(args)) => {
            run_embedded_agent_command(&["remote-control"], args)
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        Some(CliCommand::App(args)) => run_embedded_agent_command(&["app"], args),
        Some(CliCommand::Completion(args)) => run_embedded_agent_command(&["completion"], args),
        Some(CliCommand::Update(args)) => run_embedded_agent_command(&["update"], args),
        Some(CliCommand::Doctor(args)) => run_embedded_agent_command(&["doctor"], args),
        Some(CliCommand::Sandbox(args)) => run_embedded_agent_command(&["sandbox"], args),
        Some(CliCommand::Debug(args)) => run_embedded_agent_command(&["debug"], args),
        Some(CliCommand::Apply(args)) => run_embedded_agent_command(&["apply"], args),
        Some(CliCommand::Resume(args)) => run_embedded_agent_command(&["resume"], args),
        Some(CliCommand::Archive(args)) => run_embedded_agent_command(&["archive"], args),
        Some(CliCommand::Delete(args)) => run_embedded_agent_command(&["delete"], args),
        Some(CliCommand::Unarchive(args)) => run_embedded_agent_command(&["unarchive"], args),
        Some(CliCommand::Fork(args)) => run_embedded_agent_command(&["fork"], args),
        Some(CliCommand::Cloud(args)) => run_embedded_agent_command(&["cloud"], args),
        Some(CliCommand::ExecServer(args)) => run_embedded_agent_command(&["exec-server"], args),
        Some(CliCommand::Features(args)) => run_embedded_agent_command(&["features"], args),
        Some(CliCommand::Execpolicy(args)) => run_embedded_agent_command(&["execpolicy"], args),
        Some(CliCommand::ResponsesApiProxy(args)) => {
            run_embedded_agent_command(&["responses-api-proxy"], args)
        }
        Some(CliCommand::StdioToUds(args)) => run_embedded_agent_command(&["stdio-to-uds"], args),
        Some(CliCommand::Miniapp { args }) => miniapp_command(codex_executable_path, args),
        Some(CliCommand::Marketplace { command }) => marketplace_command(command),
        Some(CliCommand::Plugin { command }) => plugin_command(codex_executable_path, command),
        Some(CliCommand::Wallet { command }) => wallet_command(command),
        Some(CliCommand::Purchases { command }) => purchases_command(command),
        None => with_runtime(codex_executable_path, |runtime| {
            interactive_chat(runtime, None)
        }),
    }
}

fn capability_command(runtime: &RuntimeHandle, command: CapabilityCommand) -> Result<(), String> {
    match command {
        CapabilityCommand::List { query, json } => {
            let response = runtime.execute(json!({
                "@type": "mahayana.capability.list",
                "query": query,
            }))?;
            if json {
                print_json(&response)
            } else {
                for capability in response
                    .get("data")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    println!(
                        "{} {}",
                        capability
                            .get("mention")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                        capability
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    );
                }
                Ok(())
            }
        }
        CapabilityCommand::Invoke {
            capability_id,
            text,
        } => print_json(&runtime.execute(json!({
            "@type": "mahayana.capability.invoke",
            "capabilityId": capability_id,
            "text": text.join(" "),
        }))?),
    }
}

fn feature_request_id(prefix: &str) -> String {
    format!("cli-{prefix}-{}", uuid::Uuid::new_v4())
}

fn parse_listener_platform(value: &str) -> Result<ListenerPlatform, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "slack" => Ok(ListenerPlatform::Slack),
        "github" | "gh" => Ok(ListenerPlatform::Github),
        "git" => Ok(ListenerPlatform::Git),
        "teams" | "microsoft-teams" | "microsoftteams" => Ok(ListenerPlatform::Teams),
        "linear" => Ok(ListenerPlatform::Linear),
        "sentry" => Ok(ListenerPlatform::Sentry),
        "pagerduty" | "pager-duty" => Ok(ListenerPlatform::Pagerduty),
        other => Err(format!(
            "不支持的事件来源 {other}；可用值：slack/github/git/teams/linear/sentry/pagerduty"
        )),
    }
}

fn cli_feature_host(codex_executable_path: Option<&Path>) -> Result<FeatureHostController, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut host = HostCreateConfig::default();
    host.runtime.data_dir = Some(default_mahayana_home().join("runtime"));
    host.runtime.workspace_roots = vec![cwd.clone()];
    if let Ok(base_url) = std::env::var("MAHAYANA_RESPONSES_BASE_URL")
        && !base_url.trim().is_empty()
    {
        host.runtime.model.base_url = Some(base_url);
    }
    host.product_session_path = Some(default_product_session_path());
    host.product_surface_state_path = Some(default_product_surface_path());
    host.automation_path = Some(default_automation_path());
    host.codex_executable_path = codex_executable_path.map(Path::to_path_buf);
    host.cwd = Some(cwd);
    host.host_platform = Some(HostPlatform::Cli);
    host.inherit_installed_plugins = Some(true);
    host.use_codex_account = std::env::var("MAHAYANA_USE_CODEX_ACCOUNT").as_deref() == Ok("1");
    if let Some(codex_home) = std::env::var_os("MAHAYANA_CODEX_HOME") {
        host.codex_home = Some(PathBuf::from(codex_home));
    }
    FeatureHostController::create_with_host_config(
        HostConfig {
            profile_id: "cli".into(),
            mode: HostMode::Production,
        },
        SurfacePlatform::Mock,
        host,
    )
    .map_err(|error| error.to_string())
}

fn drain_feature_events(controller: &FeatureHostController) -> Result<Vec<HostEvent>, String> {
    let mut events = Vec::new();
    while let Some(event) = controller.receive().map_err(|error| error.to_string())? {
        events.push(event);
    }
    Ok(events)
}

fn feature_surface_command(
    codex_executable_path: Option<&Path>,
    operation: impl FnOnce(&FeatureHostController) -> Result<(), String>,
) -> Result<(), String> {
    let controller = cli_feature_host(codex_executable_path)?;
    // Drop host.ready so command output contains only the requested surface.
    let _ = drain_feature_events(&controller)?;
    operation(&controller)
}

fn execute_feature_and_print(
    controller: &FeatureHostController,
    command: FeatureCommand,
) -> Result<(), String> {
    controller
        .execute(command)
        .map_err(|error| error.to_string())?;
    let events = drain_feature_events(controller)?;
    print_json(&serde_json::to_value(events).map_err(|error| error.to_string())?)
}

fn connector_command(
    controller: &FeatureHostController,
    command: ConnectorCommand,
) -> Result<(), String> {
    let request_id = feature_request_id("connector");
    let command = match command {
        ConnectorCommand::List => FeatureCommand::ConnectorList { request_id },
        ConnectorCommand::Connect {
            connector_id,
            label,
        } => FeatureCommand::ConnectorConnect {
            request_id,
            connector_id,
            account_label: label,
        },
        ConnectorCommand::Rename {
            connector_id,
            account_id,
            label,
        } => FeatureCommand::ConnectorRenameAccount {
            request_id,
            connector_id,
            account_id,
            label,
        },
        ConnectorCommand::Remove {
            connector_id,
            account_id,
        } => FeatureCommand::ConnectorRemoveAccount {
            request_id,
            connector_id,
            account_id,
        },
        ConnectorCommand::Tool {
            connector_id,
            tool_id,
            enabled,
        } => FeatureCommand::ConnectorSetToolEnabled {
            request_id,
            connector_id,
            tool_id,
            enabled,
        },
    };
    execute_feature_and_print(controller, command)
}

fn skill_command(controller: &FeatureHostController, command: SkillCommand) -> Result<(), String> {
    let request_id = feature_request_id("skill");
    let command = match command {
        SkillCommand::List { agent_id } => FeatureCommand::SkillList {
            request_id,
            agent_id,
        },
        SkillCommand::Upsert {
            id,
            name,
            description,
            use_when,
            instructions,
            owner_agent_id,
        } => FeatureCommand::SkillUpsert {
            request_id,
            id,
            name,
            description,
            use_when,
            instructions,
            owner_agent_id,
        },
        SkillCommand::Delete { id } => FeatureCommand::SkillDelete { request_id, id },
        SkillCommand::Publish { id, team_id } => FeatureCommand::SkillPublish {
            request_id,
            id,
            team_id,
        },
        SkillCommand::Unpublish { id } => FeatureCommand::SkillUnpublish { request_id, id },
        SkillCommand::Sync { id } => FeatureCommand::SkillSync { request_id, id },
    };
    execute_feature_and_print(controller, command)
}

fn bot_command(controller: &FeatureHostController, command: BotCommand) -> Result<(), String> {
    let request_id = feature_request_id("bot");
    let command = match command {
        BotCommand::List => FeatureCommand::BotList { request_id },
        BotCommand::Hide { id } => FeatureCommand::BotSetHidden {
            request_id,
            id,
            hidden: true,
        },
        BotCommand::Show { id } => FeatureCommand::BotSetHidden {
            request_id,
            id,
            hidden: false,
        },
    };
    execute_feature_and_print(controller, command)
}

fn listener_command(
    controller: &FeatureHostController,
    command: ListenerCommand,
) -> Result<(), String> {
    let request_id = feature_request_id("listener");
    let command = match command {
        ListenerCommand::List => FeatureCommand::ListenerList { request_id },
        ListenerCommand::Connect { platform } => FeatureCommand::ListenerConnect {
            request_id,
            platform: parse_listener_platform(&platform)?,
        },
    };
    execute_feature_and_print(controller, command)
}

fn routine_command(
    controller: &FeatureHostController,
    command: RoutineCommand,
) -> Result<(), String> {
    match command {
        RoutineCommand::List => execute_feature_and_print(
            controller,
            FeatureCommand::AutomationList {
                request_id: feature_request_id("routine-list"),
                agent_id: None,
            },
        ),
        RoutineCommand::Create {
            name,
            prompt,
            schedule,
            source,
            event,
            filter,
            disabled,
        } => {
            let trigger = match (source, event) {
                (None, None) => Some(AutomationTrigger::Schedule {
                    schedule: schedule.clone(),
                }),
                (Some(source), Some(event)) => Some(AutomationTrigger::Event {
                    source: parse_listener_platform(&source)?,
                    event,
                    filter,
                }),
                _ => {
                    return Err("事件例程必须同时提供 --source 和 --event".into());
                }
            };
            execute_feature_and_print(
                controller,
                FeatureCommand::AutomationUpsert {
                    request_id: feature_request_id("routine-create"),
                    id: None,
                    agent_id: None,
                    name,
                    prompt,
                    schedule,
                    trigger,
                    enabled: !disabled,
                },
            )
        }
        RoutineCommand::Pause { id } => execute_feature_and_print(
            controller,
            FeatureCommand::AutomationSetEnabled {
                request_id: feature_request_id("routine-pause"),
                id,
                enabled: false,
                agent_id: None,
            },
        ),
        RoutineCommand::Resume { id } => execute_feature_and_print(
            controller,
            FeatureCommand::AutomationSetEnabled {
                request_id: feature_request_id("routine-resume"),
                id,
                enabled: true,
                agent_id: None,
            },
        ),
        RoutineCommand::Run { id } => execute_feature_and_print(
            controller,
            FeatureCommand::AutomationRun {
                request_id: feature_request_id("routine-run"),
                id,
                agent_id: None,
            },
        ),
        RoutineCommand::Delete { id } => execute_feature_and_print(
            controller,
            FeatureCommand::AutomationDelete {
                request_id: feature_request_id("routine-delete"),
                id,
                agent_id: None,
            },
        ),
        RoutineCommand::TriggerEvent {
            source,
            event,
            payload,
        } => {
            let source = parse_listener_platform(&source)?;
            controller
                .execute(FeatureCommand::AutomationList {
                    request_id: feature_request_id("routine-event-list"),
                    agent_id: None,
                })
                .map_err(|error| error.to_string())?;
            let listed = drain_feature_events(controller)?;
            let automations = listed
                .into_iter()
                .find_map(|event| match event {
                    HostEvent::AutomationListed { automations, .. } => Some(automations),
                    _ => None,
                })
                .unwrap_or_default();
            let payload = payload.unwrap_or_default();
            let matching = automations
                .into_iter()
                .filter(|automation| automation.enabled)
                .filter(|automation| match automation.trigger.as_ref() {
                    Some(AutomationTrigger::Event {
                        source: trigger_source,
                        event: trigger_event,
                        filter,
                    }) => {
                        *trigger_source == source
                            && trigger_event == &event
                            && filter
                                .as_ref()
                                .is_none_or(|filter| payload.contains(filter.as_str()))
                    }
                    _ => false,
                })
                .map(|automation| automation.id)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                return Err(format!("没有启用的例程匹配事件 {source:?}/{event}"));
            }
            let mut events = Vec::new();
            for id in matching {
                controller
                    .execute(FeatureCommand::AutomationRun {
                        request_id: feature_request_id("routine-event-run"),
                        id,
                        agent_id: None,
                    })
                    .map_err(|error| error.to_string())?;
                events.extend(drain_feature_events(controller)?);
            }
            print_json(&serde_json::to_value(events).map_err(|error| error.to_string())?)
        }
    }
}

struct VerifiedMarketplaceArchive {
    version: String,
    package_sha256: String,
    package_size: u64,
    format: ArtifactFormat,
    archive: Vec<u8>,
}

fn verified_marketplace_archive(
    client: &MahayanaProductClient,
    plugin_id: &str,
    requested_version: Option<&str>,
) -> Result<VerifiedMarketplaceArchive, String> {
    let listing = client
        .marketplace_browse(Some(plugin_id), Some("cli"))
        .map_err(|error| error.to_string())?;
    let plugin = listing
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("pluginId").and_then(Value::as_str) == Some(plugin_id))
        })
        .ok_or_else(|| format!("市场中没有已审核插件 {plugin_id}"))?;
    let version = requested_version
        .map(str::to_string)
        .or_else(|| {
            plugin
                .get("latestVersion")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| "市场条目没有可下载版本".to_string())?;
    let metadata = client
        .marketplace_release_metadata(plugin_id, &version)
        .map_err(|error| error.to_string())?;
    if metadata.get("pluginId").and_then(Value::as_str) != Some(plugin_id)
        || metadata.get("version").and_then(Value::as_str) != Some(version.as_str())
    {
        return Err("市场版本元数据与请求的插件或版本不一致".into());
    }
    if let Some(release_value) = metadata.get("releaseManifest")
        && release_value.get("protocol").and_then(Value::as_str)
            == Some("mahayana.external-release.v1")
    {
        let release = serde_json::from_value::<ExternalReleaseManifest>(release_value.clone())
            .map_err(|error| format!("市场 external release manifest 无效：{error}"))?;
        release.validate().map_err(|error| error.to_string())?;
        if release.plugin_id != plugin_id || release.version != version {
            return Err("市场 external release manifest 身份与请求不一致".into());
        }
        let artifact = release
            .select_artifact(
                "cli",
                &["native", "desktop-stdio", "deepseek-js", "web-wasm", "mcp"],
            )
            .map_err(|error| error.to_string())?;
        let archive = ArtifactResolver::new()
            .map_err(|error| error.to_string())?
            .download_verified(artifact)
            .map_err(|error| error.to_string())?;
        return Ok(VerifiedMarketplaceArchive {
            version,
            package_sha256: artifact.sha256.to_ascii_lowercase(),
            package_size: artifact.size,
            format: artifact.format.clone(),
            archive,
        });
    }

    // Legacy releases keep their historical metadata shape. The legacy
    // download endpoint is now only a 307 compatibility redirect to the
    // publisher's external HTTPS artifact; no marketplace bytes are stored.
    let expected_sha256 = metadata
        .get("packageSha256")
        .and_then(Value::as_str)
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "市场版本元数据缺少有效 packageSha256".to_string())?
        .to_ascii_lowercase();
    let expected_size = metadata
        .get("packageSize")
        .and_then(Value::as_u64)
        .filter(|size| *size > 0 && *size <= 50 * 1024 * 1024)
        .ok_or_else(|| "市场版本元数据缺少有效 packageSize".to_string())?;
    let archive = client
        .download_marketplace_plugin(
            plugin_id,
            &version,
            usize::try_from(expected_size)
                .map_err(|_| "市场插件包大小超出当前平台限制".to_string())?,
        )
        .map_err(|error| error.to_string())?;
    if archive.len() as u64 != expected_size {
        return Err("下载的插件包大小与市场版本元数据不一致".into());
    }
    let actual_sha256 = format!("{:x}", Sha256::digest(&archive));
    if actual_sha256 != expected_sha256 {
        return Err("云端插件包哈希与市场版本元数据不一致".into());
    }
    Ok(VerifiedMarketplaceArchive {
        version,
        package_sha256: actual_sha256,
        package_size: expected_size,
        format: ArtifactFormat::TarGz,
        archive,
    })
}

fn marketplace_command(command: MarketplaceCommand) -> Result<(), String> {
    let client = MahayanaProductClient::default();
    match command {
        MarketplaceCommand::Browse => {
            let response = client
                .marketplace_browse(None, Some("cli"))
                .map_err(|error| error.to_string())?;
            print_json(&response)
        }
        MarketplaceCommand::Search { query } => {
            let response = client
                .marketplace_browse(Some(&query), Some("cli"))
                .map_err(|error| error.to_string())?;
            print_json(&response)
        }
        MarketplaceCommand::Download {
            plugin_id,
            version,
            output,
        } => {
            let verified = verified_marketplace_archive(&client, &plugin_id, version.as_deref())?;
            let extension = match verified.format {
                ArtifactFormat::TarGz => "tar.gz",
                ArtifactFormat::Zip => "zip",
                ArtifactFormat::Directory => "artifact",
            };
            let output = output.unwrap_or_else(|| {
                PathBuf::from(format!("{plugin_id}-{}.{extension}", verified.version))
            });
            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        format!("拒绝覆盖已存在的下载文件 {}", output.display())
                    } else {
                        error.to_string()
                    }
                })?;
            if let Err(error) = file.write_all(&verified.archive) {
                drop(file);
                let _ = fs::remove_file(&output);
                return Err(error.to_string());
            }
            print_json(&json!({
                "downloaded": true,
                "pluginId": plugin_id,
                "version": verified.version,
                "packageSha256": verified.package_sha256,
                "packageSize": verified.package_size,
                "output": output,
            }))
        }
        MarketplaceCommand::Install {
            plugin_id,
            version,
            repository,
        } => {
            let verified = verified_marketplace_archive(&client, &plugin_id, version.as_deref())?;
            if verified.format != ArtifactFormat::TarGz {
                return Err(
                    "Codex marketplace repository install currently requires a tar.gz CLI artifact"
                        .into(),
                );
            }
            print_json(&plugin_dev::install_marketplace_bundle(
                &repository,
                &plugin_id,
                &verified.version,
                &verified.archive,
            )?)
        }
    }
}

fn model_usage_command() -> Result<(), String> {
    let usage = MahayanaProductClient::default()
        .model_usage()
        .map_err(|error| error.to_string())?;
    let response = serde_json::to_value(usage).map_err(|error| error.to_string())?;
    print_json(&response)
}

fn plugin_command(
    codex_executable_path: Option<&Path>,
    command: PluginCommand,
) -> Result<(), String> {
    match command {
        PluginCommand::Init {
            name,
            repository,
            title,
            profile,
        } => print_json(&plugin_dev::init_repository(
            &repository,
            &name,
            title.as_deref(),
            profile,
        )?),
        PluginCommand::List {
            marketplace,
            available,
            json,
        } => {
            let mut args = vec!["list".to_string()];
            if let Some(marketplace) = marketplace {
                args.extend(["--marketplace".into(), marketplace]);
            }
            if json || available {
                args.push("--json".into());
            }
            if available {
                args.push("--available".into());
            }
            run_codex_plugin(args)
        }
        PluginCommand::Info { plugin_id } => {
            let response = MahayanaProductClient::default()
                .marketplace_browse(Some(&plugin_id), Some("cli"))
                .map_err(|error| error.to_string())?;
            print_json(&response)
        }
        PluginCommand::Install {
            plugin,
            marketplace,
            json,
            allow_local,
        } => {
            if marketplace.is_none() && plugin_dev::looks_like_github_source(&plugin) {
                let remote = plugin_dev::validate_github_source(&plugin)?;
                if remote.has_local_runtimes && !allow_local {
                    return Err(
                        "仓库包含本地 CLI/stdio runtime；请审查源码后使用 --allow-local 单独授权"
                            .into(),
                    );
                }
                let mut marketplace_args =
                    vec!["marketplace".to_string(), "add".to_string(), plugin.clone()];
                if json {
                    marketplace_args.push("--json".into());
                }
                run_codex_plugin(marketplace_args)?;
                for plugin_name in remote.plugins {
                    let mut args = vec![
                        "add".to_string(),
                        plugin_name,
                        "--marketplace".into(),
                        remote.marketplace.clone(),
                    ];
                    if json {
                        args.push("--json".into());
                    }
                    run_codex_plugin(args)?;
                }
                return Ok(());
            }
            let mut args = vec!["add".to_string(), plugin];
            if let Some(marketplace) = marketplace {
                args.extend(["--marketplace".into(), marketplace]);
            }
            if json {
                args.push("--json".into());
            }
            run_codex_plugin(args)
        }
        PluginCommand::Update {
            plugin,
            marketplace,
            json,
        } => {
            let mut args = vec!["add".to_string(), plugin];
            if let Some(marketplace) = marketplace {
                args.extend(["--marketplace".into(), marketplace]);
            }
            if json {
                args.push("--json".into());
            }
            run_codex_plugin(args)
        }
        PluginCommand::Uninstall {
            plugin,
            marketplace,
            json,
        } => {
            let mut args = vec!["remove".to_string(), plugin];
            if let Some(marketplace) = marketplace {
                args.extend(["--marketplace".into(), marketplace]);
            }
            if json {
                args.push("--json".into());
            }
            run_codex_plugin(args)
        }
        PluginCommand::Marketplace(args) => {
            run_embedded_agent_command(&["plugin", "marketplace"], args)
        }
        PluginCommand::Open { plugin_id } => with_runtime(codex_executable_path, |runtime| {
            interactive_chat(runtime, Some(format!("miniapp:{plugin_id}")))
        }),
        PluginCommand::Run {
            plugin_id,
            command,
            json,
        } => {
            let arguments = json
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|error| format!("--json 必须是合法 JSON：{error}"))?
                .unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return Err("--json 必须是 MCP Tool 参数对象".into());
            }
            let message = format!("/{plugin_id}:{command} {arguments}");
            with_runtime(codex_executable_path, |runtime| {
                send_and_stream(runtime, &format!("miniapp:{plugin_id}"), &message)
            })
        }
        PluginCommand::Validate { path } => print_json(&plugin_dev::validate_path(&path)?),
        PluginCommand::Test { path } => print_json(&plugin_dev::test_path(&path)?),
        PluginCommand::Pack { path, output } => {
            let path = plugin_dev::absolute_path(&path)?;
            LocalPlugin::load(&path).map_err(|error| error.to_string())?;
            let archive = pack_plugin_bundle_tar_gz(&path, 50 * 1024 * 1024)
                .map_err(|error| error.to_string())?;
            let output = output.unwrap_or_else(|| path.with_extension("tar.gz"));
            fs::write(&output, archive).map_err(|error| error.to_string())?;
            println!("{}", output.display());
            Ok(())
        }
        PluginCommand::Publish {
            path,
            plugin_id,
            version,
            artifact_source,
        } => {
            let path = plugin_dev::absolute_path(&path)?;
            let plugin = LocalPlugin::load(&path).map_err(|error| error.to_string())?;
            if plugin.legacy.name != plugin_id {
                return Err(format!(
                    "插件清单标识 {} 与发布参数 {plugin_id} 不一致",
                    plugin.legacy.name
                ));
            }
            if plugin.legacy.version.as_deref() != Some(version.as_str()) {
                return Err(format!(
                    "插件清单版本 {} 与发布参数 {version} 不一致",
                    plugin.legacy.version.as_deref().unwrap_or("<missing>")
                ));
            }
            plugin_dev::test_path(&path)?;
            let archive = pack_plugin_bundle_tar_gz(&path, 50 * 1024 * 1024)
                .map_err(|error| error.to_string())?;
            let package_size = archive.len() as u64;
            let package_sha256 = format!("{:x}", Sha256::digest(&archive));
            let platforms = plugin_dev::supported_marketplace_platforms(&path)?;
            let source_provenance = plugin_dev::github_source_identity(&path)?;
            let artifact_source: Value = serde_json::from_str(&artifact_source)
                .map_err(|error| format!("--artifact-source 必须是合法 JSON: {error}"))?;
            let release_manifest = json!({
                "schemaVersion": 1,
                "protocol": "mahayana.external-release.v1",
                "pluginId": plugin_id,
                "version": version,
                "permissions": [],
                "artifacts": [{
                    "id": "plugin-package",
                    "runtime": "plugin-package",
                    "platforms": platforms,
                    "source": artifact_source,
                    "sha256": package_sha256,
                    "size": package_size,
                    "format": "tar-gz"
                }]
            });
            let client = MahayanaProductClient::default();
            let response = client
                .publish_external_plugin(
                    &plugin_id,
                    &version,
                    &plugin.legacy.name,
                    "",
                    &platforms,
                    &source_provenance,
                    &release_manifest,
                )
                .map_err(|error| error.to_string())?;
            print_json(&response)
        }
    }
}

#[cfg(not(feature = "codex-compat"))]
fn run_codex_plugin(_args: Vec<String>) -> Result<(), String> {
    Err("legacy plugin marketplace commands require the optional codex-compat adapter; use mahayana marketplace for the native product path".into())
}

#[cfg(feature = "codex-compat")]
fn run_codex_plugin(args: Vec<String>) -> Result<(), String> {
    let cli = PluginCli::try_parse_from(std::iter::once("mahayana plugin".to_string()).chain(args))
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime
        .block_on(async move {
            let overrides = cli
                .config_overrides
                .parse_overrides()
                .map_err(anyhow::Error::msg)?;
            match cli.subcommand {
                PluginSubcommand::Add(args) => {
                    codex_cli::plugin_cmd::run_plugin_add(overrides, args).await
                }
                PluginSubcommand::List(args) => {
                    codex_cli::plugin_cmd::run_plugin_list(overrides, args).await
                }
                PluginSubcommand::Marketplace(marketplace) => marketplace.run().await,
                PluginSubcommand::Remove(args) => {
                    codex_cli::plugin_cmd::run_plugin_remove(overrides, args).await
                }
            }
        })
        .map_err(|error| error.to_string())
}

fn wallet_command(command: WalletCommand) -> Result<(), String> {
    let client = MahayanaProductClient::default();
    let response = match command {
        WalletCommand::Balance => {
            serde_json::to_value(client.wallet_balance().map_err(|error| error.to_string())?)
        }
        WalletCommand::History => serde_json::to_value(
            client
                .wallet_history(None)
                .map_err(|error| error.to_string())?,
        ),
        WalletCommand::TopUp {
            sku,
            idempotency_key,
        } => {
            let idempotency_key =
                idempotency_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            Ok(client
                .wallet_top_up(&sku, &idempotency_key)
                .map_err(|error| error.to_string())?)
        }
    }
    .map_err(|error| error.to_string())?;
    print_json(&response)
}

fn purchases_command(command: PurchasesCommand) -> Result<(), String> {
    let client = MahayanaProductClient::default();
    let response = match command {
        PurchasesCommand::List => {
            serde_json::to_value(client.purchases(None).map_err(|error| error.to_string())?)
        }
        PurchasesCommand::Restore => serde_json::to_value(
            client
                .restore_purchases()
                .map_err(|error| error.to_string())?,
        ),
    }
    .map_err(|error| error.to_string())?;
    print_json(&response)
}

fn miniapp_command(codex_executable_path: Option<&Path>, args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("registry") => product_command("mahayana.miniapps.registry", json!({})),
        Some("chat") => {
            let miniapp_id = args
                .get(1)
                .ok_or_else(|| "用法：mahayana miniapp chat <miniapp-id> <消息>".to_string())?;
            let message = args.get(2..).unwrap_or_default().join(" ");
            if message.trim().is_empty() {
                return Err("小程序消息不能为空".into());
            }
            with_runtime(codex_executable_path, |runtime| {
                send_and_stream(runtime, &format!("miniapp:{miniapp_id}"), &message)
            })
        }
        _ => Err("用法：mahayana miniapp registry|chat".into()),
    }
}

fn login(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("alipay") => alipay_login(),
        Some("password") => password_login(&args[1..]),
        Some("test") => test_account_login(&args[1..]),
        Some(other) => Err(format!(
            "未知登录方式：{other}。使用 mahayana login alipay、mahayana login password <用户名> 或 mahayana login test"
        )),
    }
}

fn test_account_login(args: &[String]) -> Result<(), String> {
    let token = match args {
        [] => std::env::var("MAHAYANA_TEST_ACCOUNT_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Ok)
            .unwrap_or_else(|| {
                rpassword::prompt_password("测试账号访问令牌：").map_err(|error| error.to_string())
            })?,
        [mode] if mode == "--token-stdin" => read_line()?,
        _ => {
            return Err("用法：mahayana login test [--token-stdin]；令牌不得放在命令参数中".into());
        }
    };
    MahayanaProductClient::default()
        .store_test_account_session(&token)
        .map_err(|error| error.to_string())?;
    println!("测试账号 TestAccount 登录成功。会话已加密保存，AI 测试额度不设日常上限。");
    Ok(())
}

fn password_login(args: &[String]) -> Result<(), String> {
    let username = args
        .first()
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "用法：mahayana login password <用户名> [--password-stdin]".to_string())?;
    let password = read_password(args.get(1).map(String::as_str))?;
    let response = MahayanaProductClient::default()
        .execute(
            "mahayana.auth.password.login",
            &json!({"username": username, "password": password}),
        )
        .map_err(|error| error.to_string())?;
    if response.get("sessionStored").and_then(Value::as_bool) != Some(true) {
        return Err("官方登录没有返回可保存的软件会话".into());
    }
    println!("登录成功。App 与 CLI 将共用同一大乘账号会话；无需 OpenAI 登录。");
    Ok(())
}

fn send_verification_code(args: Vec<String>) -> Result<(), String> {
    let email = args
        .first()
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "用法：mahayana send-code <邮箱>".to_string())?;
    product_command(
        "mahayana.auth.verification.send",
        json!({"email": email, "type": "register"}),
    )
}

fn register(args: Vec<String>) -> Result<(), String> {
    if args.len() < 3 {
        return Err("用法：mahayana register <用户名> <邮箱> <验证码> [--password-stdin]".into());
    }
    let password = read_password(args.get(3).map(String::as_str))?;
    product_command(
        "mahayana.auth.register",
        json!({
            "username": args[0],
            "email": args[1],
            "verificationCode": args[2],
            "password": password,
        }),
    )
}

fn read_password(mode: Option<&str>) -> Result<String, String> {
    let password = if mode == Some("--password-stdin") {
        read_line()?
    } else if mode.is_none() {
        rpassword::prompt_password("密码：").map_err(|error| error.to_string())?
    } else {
        return Err("密码不能放在命令参数中；请省略密码或使用 --password-stdin".into());
    };
    let password = password.trim().to_string();
    if password.is_empty() {
        return Err("密码不能为空".into());
    }
    Ok(password)
}

fn alipay_login() -> Result<(), String> {
    let client = MahayanaProductClient::default();
    let authorization = client
        .execute("mahayana.auth.alipay.start", &json!({"platform": "cli"}))
        .map_err(|error| error.to_string())?;
    let url = authorization
        .get("loginUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "登录服务没有返回支付宝授权地址".to_string())?;
    let state = authorization
        .get("state")
        .and_then(Value::as_str)
        .ok_or_else(|| "登录服务没有返回会话状态".to_string())?
        .to_string();
    println!("请在浏览器完成支付宝登录：\n{url}");
    open_browser(url);
    for _ in 0..150 {
        thread::sleep(Duration::from_secs(2));
        let response = client
            .execute("mahayana.auth.alipay.poll", &json!({"state": state}))
            .map_err(|error| error.to_string())?;
        match response.get("status").and_then(Value::as_str) {
            Some("complete") => {
                println!("登录成功。软件会话已安全保存；Codex 不需要 OpenAI 登录。");
                return Ok(());
            }
            Some("expired") | Some("failed") => {
                return Err(format!("支付宝登录未完成：{}", redact_secrets(&response)));
            }
            _ => {}
        }
    }
    Err("支付宝登录等待超时，请重试".into())
}

fn product_command(request_type: &str, request: Value) -> Result<(), String> {
    let response = MahayanaProductClient::default()
        .execute(request_type, &request)
        .map_err(|error| error.to_string())?;
    print_json(&redact_secrets(&response))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut value = Command::new("cmd");
        value.args(["/C", "start", ""]);
        value
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return;
    let _ = command.arg(url).spawn();
}

fn with_runtime(
    codex_executable_path: Option<&Path>,
    operation: impl FnOnce(&RuntimeHandle) -> Result<(), String>,
) -> Result<(), String> {
    let runtime = RuntimeHandle::create(codex_executable_path)?;
    operation(&runtime)
}

struct RuntimeHandle(u64);

impl RuntimeHandle {
    fn create(codex_executable_path: Option<&Path>) -> Result<Self, String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let use_codex_account = std::env::var("MAHAYANA_USE_CODEX_ACCOUNT").as_deref() == Ok("1");
        let mut config = json!({
            "codexExecutablePath": codex_executable_path,
            "hostPlatform": "cli",
            "cwd": cwd,
            "workspaceRoots": [cwd],
            "useCodexAccount": use_codex_account,
        });
        if use_codex_account && let Some(codex_home) = std::env::var_os("MAHAYANA_CODEX_HOME") {
            config["codexHome"] = serde_json::to_value(PathBuf::from(codex_home))
                .map_err(|error| error.to_string())?;
        }
        if let Ok(base_url) = std::env::var("MAHAYANA_RESPONSES_BASE_URL")
            && !base_url.trim().is_empty()
        {
            config["model"]["baseUrl"] = Value::String(base_url);
        }
        Self::create_with_config(config)
    }

    fn create_with_config(config: Value) -> Result<Self, String> {
        let config = serde_json::to_string(&config).map_err(|error| error.to_string())?;
        let config = CString::new(config).map_err(|error| error.to_string())?;
        let id = unsafe { mahayana_runtime_create(config.as_ptr()) };
        if id == 0 {
            let error = unsafe { take_json(mahayana_runtime_last_error()) }?;
            return Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("本地 Runtime 创建失败")
                .to_string());
        }
        Ok(Self(id))
    }

    fn execute(&self, command: Value) -> Result<Value, String> {
        let command = CString::new(command.to_string()).map_err(|error| error.to_string())?;
        let response = unsafe { take_json(mahayana_runtime_execute(self.0, command.as_ptr())) }?;
        unwrap_ffi(response)
    }

    fn receive(&self, timeout_ms: u64) -> Result<Option<Value>, String> {
        let response = unsafe { take_json(mahayana_runtime_receive(self.0, timeout_ms)) }?;
        let data = unwrap_ffi(response)?;
        if data.is_null() {
            Ok(None)
        } else {
            Ok(Some(data))
        }
    }

    fn resolve_approval(&self, approval_id: &str, decision: &str) -> Result<(), String> {
        let request =
            CString::new(json!({"approvalId": approval_id, "decision": decision}).to_string())
                .map_err(|error| error.to_string())?;
        let response =
            unsafe { take_json(mahayana_runtime_resolve_approval(self.0, request.as_ptr())) }?;
        unwrap_ffi(response).map(|_| ())
    }

    fn interrupt(&self, operation_id: &str) -> Result<(), String> {
        let request = CString::new(json!({"operationId": operation_id}).to_string())
            .map_err(|error| error.to_string())?;
        let response = unsafe { take_json(mahayana_runtime_interrupt(self.0, request.as_ptr())) }?;
        unwrap_ffi(response).map(|_| ())
    }
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        unsafe {
            let pointer = mahayana_runtime_close(self.0);
            mahayana_runtime_free_string(pointer);
        }
    }
}

fn interactive_chat(runtime: &RuntimeHandle, selected: Option<String>) -> Result<(), String> {
    chat_tui::run(runtime, selected)
}

fn send_and_stream(
    runtime: &RuntimeHandle,
    conversation_id: &str,
    text: &str,
) -> Result<(), String> {
    send_and_collect(runtime, conversation_id, text, true).map(|_| ())
}

fn send_and_collect(
    runtime: &RuntimeHandle,
    conversation_id: &str,
    text: &str,
    print_stream: bool,
) -> Result<String, String> {
    let accepted = runtime.execute(json!({
        "@type": "mahayana.conversation.send",
        "conversationId": conversation_id,
        "text": text,
    }))?;
    let operation_id = accepted
        .get("operationId")
        .and_then(Value::as_str)
        .ok_or_else(|| "Runtime 没有返回操作编号".to_string())?;
    let mut assistant = String::new();
    let mut streamed_output = false;
    let mut usage_summary = None;
    loop {
        let Some(event) = runtime.receive(30_000)? else {
            continue;
        };
        if std::env::var("MAHAYANA_TRACE_EVENTS").as_deref() == Ok("1") {
            eprintln!("[mahayana:event] {}", redact_secrets(&event));
        }
        match event.get("@type").and_then(Value::as_str) {
            Some("mahayana.message.delta")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assistant.push_str(delta);
                if print_stream {
                    print!("{delta}");
                    io::stdout().flush().map_err(|error| error.to_string())?;
                    streamed_output |= !delta.is_empty();
                }
            }
            Some("mahayana.message.completed")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                if event.get("message").and_then(|value| value.get("role"))
                    != Some(&Value::String("user".into()))
                {
                    if let Some(text) = event
                        .get("message")
                        .and_then(|value| value.get("text"))
                        .and_then(Value::as_str)
                    {
                        merge_completed_text(&mut assistant, text);
                        if should_print_completed_text(print_stream, streamed_output, text) {
                            print!("{text}");
                            io::stdout().flush().map_err(|error| error.to_string())?;
                            streamed_output = true;
                        }
                    }
                    if print_stream {
                        println!();
                    }
                }
            }
            Some("mahayana.approval.requested")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                handle_approval(runtime, &event)?;
            }
            Some("mahayana.model.usage.updated")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                usage_summary = format_usage_summary(&event);
            }
            Some("mahayana.operation.completed")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                if print_stream && let Some(usage) = usage_summary {
                    eprintln!("{usage}");
                }
                return Ok(assistant);
            }
            Some("mahayana.operation.failed")
                if event.get("operationId").and_then(Value::as_str) == Some(operation_id) =>
            {
                return Err(event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("操作失败")
                    .to_string());
            }
            _ => {}
        }
    }
}

fn merge_completed_text(assistant: &mut String, completed: &str) {
    if !completed.is_empty() || assistant.is_empty() {
        *assistant = completed.to_string();
    }
}

fn should_print_completed_text(print_stream: bool, streamed_output: bool, completed: &str) -> bool {
    print_stream && !streamed_output && !completed.is_empty()
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    #[cfg(feature = "codex-compat")]
    use super::Cli;
    use super::merge_completed_text;
    use super::should_print_completed_text;
    use super::should_run_embedded_agent_cli;
    #[cfg(feature = "codex-compat")]
    use clap::CommandFactory;
    #[cfg(feature = "codex-compat")]
    use std::collections::BTreeSet;
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[cfg(feature = "codex-compat")]
    #[test]
    fn embedded_help_uses_mahayana_command_name() {
        let help = codex_cli::multitool_command()
            .render_long_help()
            .to_string();

        assert!(help.contains("Usage: mahayana"));
        assert!(!help.contains("Usage: codex"));
    }

    #[cfg(feature = "codex-compat")]
    #[test]
    fn every_visible_embedded_command_is_listed_by_mahayana_help() {
        let product_commands = Cli::command()
            .get_subcommands()
            .filter(|command| !command.is_hide_set())
            .map(|command| command.get_name().to_string())
            .collect::<BTreeSet<_>>();
        let missing_commands = codex_cli::multitool_command()
            .get_subcommands()
            .filter(|command| !command.is_hide_set())
            .map(|command| command.get_name().to_string())
            .filter(|command| !product_commands.contains(command))
            .collect::<Vec<_>>();

        assert!(
            missing_commands.is_empty(),
            "mahayana help is missing embedded commands: {missing_commands:?}"
        );
    }

    #[test]
    fn help_for_embedded_commands_routes_to_the_full_command_tree() {
        assert!(should_run_embedded_agent_cli(&args(&["help", "exec"])));
        assert!(should_run_embedded_agent_cli(&args(&["help", "review"])));
        assert!(!should_run_embedded_agent_cli(&args(&["help"])));
        assert!(!should_run_embedded_agent_cli(&args(&["help", "login"])));
        assert!(!should_run_embedded_agent_cli(&args(&["--help"])));
    }

    #[test]
    fn empty_completion_does_not_erase_collected_tool_result() {
        let mut assistant = r#"{"ok":true}"#.to_string();

        merge_completed_text(&mut assistant, "");

        assert_eq!(assistant, r#"{"ok":true}"#);
    }

    #[test]
    fn non_empty_completion_remains_authoritative() {
        let mut assistant = "partial".to_string();

        merge_completed_text(&mut assistant, "complete");

        assert_eq!(assistant, "complete");
    }

    #[test]
    fn completed_only_mcp_results_are_printed_once() {
        assert!(should_print_completed_text(true, false, "tool result"));
        assert!(!should_print_completed_text(true, true, "tool result"));
        assert!(!should_print_completed_text(false, false, "tool result"));
        assert!(!should_print_completed_text(true, false, ""));
    }
}

fn format_usage_summary(event: &Value) -> Option<String> {
    let usage = event.get("usage")?;
    let last = usage.get("last")?;
    let total = last.get("totalTokens").and_then(Value::as_i64)?;
    let input = last
        .get("inputTokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let cached = last
        .get("cachedInputTokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let output = last
        .get("outputTokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let reasoning = last
        .get("reasoningOutputTokens")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Some(format!(
        "本次模型用量：{total} tokens（输入 {input}，缓存输入 {cached}，输出 {output}，推理 {reasoning}）"
    ))
}

fn handle_approval(runtime: &RuntimeHandle, event: &Value) -> Result<(), String> {
    let approval_id = event
        .get("approvalId")
        .and_then(Value::as_str)
        .ok_or_else(|| "审批事件缺少编号".to_string())?;
    println!(
        "\n审批请求：{}\n{}",
        event
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("大乘操作"),
        event.get("details").cloned().unwrap_or(Value::Null)
    );
    print!("允许？[y] 本次 / [a] 本会话 / [n] 拒绝 / [c] 取消：");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let decision = match read_line()?.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => "accept",
        "a" | "always" => "acceptForSession",
        "c" | "cancel" => "cancel",
        _ => "decline",
    };
    runtime.resolve_approval(approval_id, decision)
}

fn print_conversations(response: &Value) -> Result<(), String> {
    let conversations = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Runtime 没有返回联系人列表".to_string())?;
    print_conversation_rows(conversations);
    Ok(())
}

fn print_conversation_rows(conversations: &[Value]) {
    for (index, conversation) in conversations.iter().enumerate() {
        let marker = if conversation
            .get("pinned")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "★"
        } else {
            " "
        };
        println!(
            "{:>2}. {marker} {:<24} {}",
            index + 1,
            conversation
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("未命名"),
            conversation
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
}

fn print_history(response: &Value) -> Result<(), String> {
    let messages = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Runtime 没有返回消息历史".to_string())?;
    for message in messages {
        println!(
            "[{}] {}",
            message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn read_line() -> Result<String, String> {
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| error.to_string())?;
    Ok(value)
}

fn print_json(value: &Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn unwrap_ffi(response: Value) -> Result<Value, String> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    } else {
        Err(response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Runtime 调用失败")
            .to_string())
    }
}

unsafe fn take_json(pointer: *mut c_char) -> Result<Value, String> {
    if pointer.is_null() {
        return Err("Runtime 返回了空指针".into());
    }
    let source = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|error| error.to_string())?
        .to_string();
    unsafe { mahayana_runtime_free_string(pointer) };
    serde_json::from_str(&source).map_err(|error| error.to_string())
}
