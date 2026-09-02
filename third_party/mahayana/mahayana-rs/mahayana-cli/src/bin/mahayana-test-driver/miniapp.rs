use mahayana_mcp_runtime::{McpTransport, NativeMcpClient};
use mahayana_plugin_runtime::PluginInstaller;
use mahayana_product::MahayanaProductClient;
use mahayana_test_driver_protocol::TestDriverError;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use url::Url;

const TEST_ACCOUNT_TOKEN_ENV: &str = "MAHAYANA_TEST_ACCOUNT_TOKEN";

pub(super) fn open_miniapp(
    product: &MahayanaProductClient,
    root: &Path,
    params: &Value,
    correlation_id: &str,
) -> Result<Value, TestDriverError> {
    let (plugin_id, platform) = miniapp_identity(params)?;
    let receipt = require_installed(root, plugin_id)?;
    let marketplace = marketplace_plugin(product, plugin_id, platform)?;
    let bot_endpoint = required_string(&marketplace, "botEndpoint", "marketplace_bot_missing")?;
    let bot = authenticated_mcp_client(bot_endpoint)?;
    let tool_result = bot
        .call_tool("home", json!({}))
        .map_err(|error| mcp_error("miniapp_open_failed", "home", bot_endpoint, error))?;
    Ok(json!({
        "miniAppId": plugin_id,
        "conversationId": format!("miniapp:{plugin_id}"),
        "receipt": receipt,
        "toolCalls": [tool_call_evidence("bot", bot_endpoint, "home", json!({}), correlation_id)],
        "toolResult": tool_result,
    }))
}

pub(super) fn chat_miniapp(
    product: &MahayanaProductClient,
    root: &Path,
    params: &Value,
    correlation_id: &str,
) -> Result<Value, TestDriverError> {
    let (plugin_id, platform) = miniapp_identity(params)?;
    let _receipt = require_installed(root, plugin_id)?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TestDriverError::new("invalid_params", "miniapp.chat requires message"))?;
    let marketplace = marketplace_plugin(product, plugin_id, platform)?;
    let bot_endpoint = required_string(&marketplace, "botEndpoint", "marketplace_bot_missing")?;
    let bot = authenticated_mcp_client(bot_endpoint)?;
    let bot_arguments = json!({"message": message});
    let bot_result = bot
        .call_tool("chat", bot_arguments.clone())
        .map_err(|error| mcp_error("miniapp_chat_failed", "chat", bot_endpoint, error))?;
    let dispatch = structured_content(&bot_result)?;
    let mut tool_calls = vec![tool_call_evidence(
        "bot",
        bot_endpoint,
        "chat",
        bot_arguments,
        correlation_id,
    )];

    if dispatch
        .get("requiresApproval")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(json!({
            "miniAppId": plugin_id,
            "conversationId": format!("miniapp:{plugin_id}"),
            "message": message,
            "toolCalls": tool_calls,
            "botResult": bot_result,
            "dispatch": dispatch,
            "executionStatus": "approval_required",
        }));
    }

    let (runtime_endpoint, runtime_tool, runtime_arguments) = dispatch_execution(dispatch)?;
    let runtime = authenticated_mcp_client(runtime_endpoint)?;
    let plugin_result = runtime
        .call_tool(runtime_tool, runtime_arguments.clone())
        .map_err(|error| {
            mcp_error(
                "miniapp_runtime_call_failed",
                runtime_tool,
                runtime_endpoint,
                error,
            )
        })?;
    tool_calls.push(tool_call_evidence(
        "runtime",
        runtime_endpoint,
        runtime_tool,
        runtime_arguments,
        correlation_id,
    ));

    Ok(json!({
        "miniAppId": plugin_id,
        "conversationId": format!("miniapp:{plugin_id}"),
        "message": message,
        "toolCalls": tool_calls,
        "botResult": bot_result,
        "dispatch": dispatch,
        "pluginResult": plugin_result,
        "executionStatus": "completed",
    }))
}

pub(super) fn describe_actions(
    product: &MahayanaProductClient,
    root: &Path,
    params: &Value,
    correlation_id: &str,
) -> Result<Value, TestDriverError> {
    let (plugin_id, platform) = miniapp_identity(params)?;
    let receipt = require_installed(root, plugin_id)?;
    let marketplace = marketplace_plugin(product, plugin_id, platform)?;
    let bot_endpoint = required_string(&marketplace, "botEndpoint", "marketplace_bot_missing")?;
    let bot = authenticated_mcp_client(bot_endpoint)?;
    let bot_tools = bot
        .list_tools()
        .map_err(|error| mcp_error("actions_describe_failed", "tools/list", bot_endpoint, error))?;

    let commands = marketplace
        .get("commands")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let runtime_endpoint = marketplace_runtime_endpoint(&marketplace)?;
    let runtime = authenticated_mcp_client(runtime_endpoint)?;
    let runtime_tools = runtime.list_tools().map_err(|error| {
        mcp_error(
            "actions_describe_failed",
            "tools/list",
            runtime_endpoint,
            error,
        )
    })?;

    Ok(json!({
        "miniAppId": plugin_id,
        "receipt": receipt,
        "actions": commands,
        "botTools": bot_tools,
        "runtimeTools": runtime_tools,
        "toolCalls": [
            tool_call_evidence("bot", bot_endpoint, "tools/list", json!({}), correlation_id),
            tool_call_evidence("runtime", runtime_endpoint, "tools/list", json!({}), correlation_id),
        ],
    }))
}

pub(super) fn invoke_action(
    product: &MahayanaProductClient,
    root: &Path,
    params: &Value,
    correlation_id: &str,
) -> Result<Value, TestDriverError> {
    let (plugin_id, platform) = miniapp_identity(params)?;
    let _receipt = require_installed(root, plugin_id)?;
    let action = params
        .get("action")
        .or_else(|| params.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TestDriverError::new("invalid_params", "actions.invoke requires action"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let marketplace = marketplace_plugin(product, plugin_id, platform)?;
    let command = marketplace
        .get("commands")
        .and_then(Value::as_array)
        .and_then(|commands| {
            commands
                .iter()
                .find(|command| command.get("name").and_then(Value::as_str) == Some(action))
        })
        .ok_or_else(|| {
            TestDriverError::new(
                "action_not_discoverable",
                format!("{action} is not a marketplace-declared action for {plugin_id}"),
            )
        })?;
    let approval = command
        .get("approval")
        .and_then(Value::as_str)
        .unwrap_or("none");
    if approval != "none" && params.get("approval").and_then(Value::as_str) != Some("confirmed") {
        return Err(TestDriverError::new(
            "approval_required",
            format!("{plugin_id}:{action} requires {approval} approval"),
        )
        .with_details(json!({
            "miniAppId": plugin_id,
            "action": action,
            "approval": approval,
        })));
    }

    let bot_endpoint = required_string(&marketplace, "botEndpoint", "marketplace_bot_missing")?;
    let bot = authenticated_mcp_client(bot_endpoint)?;
    let bot_arguments = json!({"arguments": arguments});
    let bot_result = bot
        .call_tool(action, bot_arguments.clone())
        .map_err(|error| mcp_error("action_route_failed", action, bot_endpoint, error))?;
    let dispatch = structured_content(&bot_result)?;
    let (runtime_endpoint, runtime_tool, runtime_arguments) = dispatch_execution(dispatch)?;
    let runtime = authenticated_mcp_client(runtime_endpoint)?;
    let plugin_result = runtime
        .call_tool(runtime_tool, runtime_arguments.clone())
        .map_err(|error| {
            mcp_error(
                "action_runtime_call_failed",
                runtime_tool,
                runtime_endpoint,
                error,
            )
        })?;

    Ok(json!({
        "miniAppId": plugin_id,
        "action": action,
        "approval": approval,
        "toolCalls": [
            tool_call_evidence("bot", bot_endpoint, action, bot_arguments, correlation_id),
            tool_call_evidence("runtime", runtime_endpoint, runtime_tool, runtime_arguments, correlation_id),
        ],
        "botResult": bot_result,
        "dispatch": dispatch,
        "pluginResult": plugin_result,
        "executionStatus": "completed",
    }))
}

fn marketplace_plugin(
    product: &MahayanaProductClient,
    plugin_id: &str,
    platform: &str,
) -> Result<Value, TestDriverError> {
    let listing = product
        .marketplace_browse(Some(plugin_id), Some(platform))
        .map_err(|error| {
            TestDriverError::new("product_backend_error", error.to_string()).with_details(json!({
                "operation": "marketplace.search",
                "pluginId": plugin_id,
                "platform": platform,
            }))
        })?;
    listing
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("pluginId").and_then(Value::as_str) == Some(plugin_id))
        })
        .cloned()
        .ok_or_else(|| {
            TestDriverError::new(
                "marketplace_plugin_not_found",
                format!("approved marketplace plugin {plugin_id} was not found for {platform}"),
            )
        })
}

fn marketplace_runtime_endpoint(marketplace: &Value) -> Result<&str, TestDriverError> {
    marketplace
        .get("surfaces")
        .and_then(Value::as_array)
        .and_then(|surfaces| {
            surfaces.iter().find_map(|surface| {
                let kind = surface.get("kind").and_then(Value::as_str)?;
                if kind != "mcp-http" {
                    return None;
                }
                surface.get("url").and_then(Value::as_str)
            })
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TestDriverError::new(
                "miniapp_runtime_missing",
                "marketplace entry has no discoverable MCP HTTP runtime",
            )
        })
}

fn require_installed(root: &Path, plugin_id: &str) -> Result<Value, TestDriverError> {
    let installer = PluginInstaller::new(root.join("plugins")).map_err(|error| {
        TestDriverError::new(
            "plugin_runtime_unavailable",
            format!("failed to initialize Mahayana plugin installer: {error}"),
        )
    })?;
    let pointer = installer
        .active(plugin_id)
        .map_err(|error| {
            TestDriverError::new(
                "plugin_state_read_failed",
                format!("failed to read active plugin receipt: {error}"),
            )
        })?
        .ok_or_else(|| {
            TestDriverError::new(
                "plugin_not_installed",
                format!("{plugin_id} must be installed before MiniApp execution"),
            )
        })?;
    serde_json::to_value(pointer).map_err(|error| {
        TestDriverError::new(
            "plugin_state_read_failed",
            format!("failed to serialize active plugin receipt: {error}"),
        )
    })
}

fn miniapp_identity(params: &Value) -> Result<(&str, &str), TestDriverError> {
    let plugin_id = params
        .get("pluginId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TestDriverError::new("invalid_params", "pluginId is required"))?;
    let platform = params
        .get("platform")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ios");
    Ok((plugin_id, platform))
}

fn session_token() -> Result<String, TestDriverError> {
    std::env::var(TEST_ACCOUNT_TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TestDriverError::new(
                "test_account_token_missing",
                format!("{TEST_ACCOUNT_TOKEN_ENV} is required for authenticated MiniApp MCP calls"),
            )
        })
}

fn authenticated_mcp_client(endpoint: &str) -> Result<NativeMcpClient, TestDriverError> {
    let parsed = Url::parse(endpoint).map_err(|error| {
        TestDriverError::new(
            "unsafe_mcp_endpoint",
            format!("invalid MiniApp MCP endpoint: {error}"),
        )
    })?;
    if parsed.scheme() != "https" {
        return Err(TestDriverError::new(
            "unsafe_mcp_endpoint",
            "test-driver remote MiniApp MCP endpoints must use HTTPS",
        ));
    }
    let mut headers = BTreeMap::new();
    headers.insert(
        "Authorization".into(),
        format!("Bearer {}", session_token()?),
    );
    Ok(NativeMcpClient::new(McpTransport::Http {
        url: endpoint.to_string(),
        headers,
    }))
}

fn structured_content(result: &Value) -> Result<&Value, TestDriverError> {
    result.get("structuredContent").ok_or_else(|| {
        TestDriverError::new(
            "miniapp_protocol_error",
            "MiniApp MCP tool result did not include structuredContent",
        )
    })
}

fn dispatch_execution(dispatch: &Value) -> Result<(&str, &str, Value), TestDriverError> {
    let execution = dispatch.get("execution").ok_or_else(|| {
        TestDriverError::new(
            "miniapp_protocol_error",
            "MiniApp bot dispatch did not include execution metadata",
        )
    })?;
    let endpoint = required_string(execution, "endpoint", "miniapp_runtime_missing")?;
    let tool = required_string(execution, "tool", "miniapp_runtime_missing")?;
    let arguments = dispatch
        .get("arguments")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    Ok((endpoint, tool, arguments))
}

fn required_string<'a>(
    value: &'a Value,
    key: &str,
    code: &str,
) -> Result<&'a str, TestDriverError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TestDriverError::new(code, format!("missing {key}")))
}

fn tool_call_evidence(
    stage: &str,
    endpoint: &str,
    name: &str,
    arguments: Value,
    correlation_id: &str,
) -> Value {
    json!({
        "stage": stage,
        "transport": "mcp-streamable-http",
        "endpoint": endpoint,
        "name": name,
        "arguments": arguments,
        "correlationId": correlation_id,
    })
}

fn mcp_error(
    code: &str,
    tool: &str,
    endpoint: &str,
    error: impl std::fmt::Display,
) -> TestDriverError {
    TestDriverError::new(code, format!("MiniApp MCP {tool} failed: {error}")).with_details(json!({
        "tool": tool,
        "endpoint": endpoint,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miniapp_identity_defaults_to_ios() {
        let params = json!({"pluginId": "global-dharma"});
        let (plugin_id, platform) = miniapp_identity(&params).unwrap();
        assert_eq!(plugin_id, "global-dharma");
        assert_eq!(platform, "ios");
    }

    #[test]
    fn remote_mcp_must_use_https() {
        let error = authenticated_mcp_client("http://example.com/mcp").unwrap_err();
        assert_eq!(error.code, "unsafe_mcp_endpoint");
    }

    #[test]
    fn runtime_endpoint_comes_from_mcp_http_surface() {
        let marketplace = json!({
            "surfaces": [
                {"kind": "web", "url": "https://example.com/ui"},
                {"kind": "mcp-http", "url": "https://example.com/mcp"}
            ]
        });
        assert_eq!(
            marketplace_runtime_endpoint(&marketplace).unwrap(),
            "https://example.com/mcp"
        );
    }
}
