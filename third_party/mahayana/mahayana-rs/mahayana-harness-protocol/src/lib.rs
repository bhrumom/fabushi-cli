//! Stable, presentation-neutral JSON protocol for Mahayana Harness.
//!
//! Electron, native mobile, CLI and headless hosts can all submit the same
//! request envelope. The protocol intentionally exposes product concepts and
//! opaque JSON payloads rather than Rust implementation details.

use mahayana_core::BuildProfile;
use mahayana_harness::{HarnessError, HarnessResult, MahayanaHarness, PluginManifest};
use mahayana_harness_advanced::{
    AdvancedHarnessServices, BundleDefinition, ExtensionDefinition, HookRegistration,
    PermissionPreset, PresetDefinition, QuestionOption,
};
use mahayana_harness_services::{
    CommandRecord, ContextFragment, HarnessServices, PromptSection, SkillRecord, TeamMember,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const HARNESS_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRequest {
    pub request_id: String,
    pub operation: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessResponse {
    pub request_id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub error: Option<HarnessProtocolError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Clone)]
pub struct HarnessApi {
    harness: MahayanaHarness,
    services: HarnessServices,
    advanced: AdvancedHarnessServices,
}

impl HarnessApi {
    pub fn new(build_profile: BuildProfile) -> Self {
        let harness = MahayanaHarness::new(build_profile);
        Self {
            services: HarnessServices::new(harness.clone()),
            advanced: AdvancedHarnessServices::new(harness.clone()),
            harness,
        }
    }

    pub fn from_parts(harness: MahayanaHarness, services: HarnessServices) -> HarnessResult<Self> {
        if !harness.shares_state_with(&services.harness()) {
            return Err(HarnessError::InvalidConfig(
                "HarnessApi parts must share one MahayanaHarness runtime".into(),
            ));
        }
        Ok(Self {
            advanced: AdvancedHarnessServices::new(harness.clone()),
            harness,
            services,
        })
    }

    pub fn harness(&self) -> &MahayanaHarness {
        &self.harness
    }

    pub fn services(&self) -> &HarnessServices {
        &self.services
    }

    pub fn advanced(&self) -> &AdvancedHarnessServices {
        &self.advanced
    }

    pub fn execute_json(&self, input: &str) -> String {
        let request = match serde_json::from_str::<HarnessRequest>(input) {
            Ok(request) => request,
            Err(error) => {
                return serde_json::to_string(&HarnessResponse {
                    request_id: String::new(),
                    ok: false,
                    result: Value::Null,
                    error: Some(HarnessProtocolError {
                        code: "invalid_request".into(),
                        message: error.to_string(),
                    }),
                })
                .unwrap_or_else(|_| "{\"ok\":false}".into());
            }
        };
        let response = match self.execute(&request.operation, request.payload.clone()) {
            Ok(result) => HarnessResponse {
                request_id: request.request_id,
                ok: true,
                result,
                error: None,
            },
            Err(error) => HarnessResponse {
                request_id: request.request_id,
                ok: false,
                result: Value::Null,
                error: Some(error_to_protocol(error)),
            },
        };
        serde_json::to_string(&response).unwrap_or_else(|_| "{\"ok\":false}".into())
    }

    pub fn execute(&self, operation: &str, payload: Value) -> HarnessResult<Value> {
        match operation {
            "protocol.version" => Ok(json!({"version": HARNESS_PROTOCOL_VERSION})),
            "runtime.snapshot" => to_value(self.harness.snapshot()?),
            "runtime.config" => self.harness.dump_config(),
            "runtime.pollEvent" => to_value(self.harness.poll_event()?),
            "services.snapshot" => self.services.snapshot(),
            "advanced.snapshot" => self.advanced.snapshot(),

            "service.register" => {
                self.harness
                    .register_service(required_string(&payload, "name")?)?;
                Ok(Value::Null)
            }
            "service.unregister" => {
                self.harness
                    .unregister_service(required_str(&payload, "name")?)?;
                Ok(Value::Null)
            }
            "profile.register" => {
                self.harness.register_profile(from_payload(payload)?)?;
                Ok(Value::Null)
            }
            "profile.activate" => {
                self.harness
                    .activate_profile(required_str(&payload, "profileId")?)?;
                Ok(Value::Null)
            }
            "plugin.mount" => {
                self.harness
                    .mount_plugin(from_payload::<PluginManifest>(payload)?)?;
                Ok(Value::Null)
            }
            "plugin.unmount" => {
                self.harness
                    .unmount_plugin(required_str(&payload, "pluginId")?)?;
                Ok(Value::Null)
            }

            "session.create" => to_value(
                self.harness
                    .create_session(required_string(&payload, "title")?)?,
            ),
            "session.fork" => to_value(
                self.harness
                    .fork_session(required_str(&payload, "sessionId")?)?,
            ),
            "session.appendEvent" => to_value(self.harness.append_session_event(
                required_str(&payload, "sessionId")?,
                required_string(&payload, "kind")?,
                payload.get("eventPayload").cloned().unwrap_or(Value::Null),
            )?),
            "session.transcript" => Ok(json!({
                "transcript": self
                    .harness
                    .transcript(required_str(&payload, "sessionId")?)?
            })),
            "session.search" => to_value(self.services.search_sessions(
                required_str(&payload, "query")?,
                optional_usize(&payload, "limit").unwrap_or(20),
            )?),
            "session.readEvents" => to_value(self.advanced.session_events(
                required_str(&payload, "sessionId")?,
                payload.get("afterSequence").and_then(Value::as_u64),
                optional_usize(&payload, "limit").unwrap_or(100),
            )?),
            "session.lineage" => to_value(
                self.advanced
                    .session_lineage(required_str(&payload, "sessionId")?)?,
            ),
            "session.relatedEvents" => to_value(self.advanced.related_events(
                required_str(&payload, "sessionId")?,
                optional_str(&payload, "kind"),
                optional_str(&payload, "agentId"),
                optional_usize(&payload, "limit").unwrap_or(100),
            )?),
            "session.title" => to_value(
                self.advanced
                    .suggest_title(required_str(&payload, "sessionId")?)?,
            ),

            "agent.spawn" => to_value(self.harness.spawn_agent(
                required_string(&payload, "name")?,
                required_string(&payload, "preset")?,
                required_string(&payload, "sessionId")?,
            )?),
            "goal.create" => to_value(self.harness.set_goal(
                required_str(&payload, "sessionId")?,
                required_string(&payload, "text")?,
            )?),
            "goal.update" => to_value(self.harness.update_goal_status(
                required_str(&payload, "goalId")?,
                required_string(&payload, "status")?,
            )?),
            "job.create" => to_value(
                self.harness
                    .create_job(required_string(&payload, "kind")?)?,
            ),
            "job.update" => to_value(self.harness.update_job(
                required_str(&payload, "jobId")?,
                required_string(&payload, "status")?,
                optional_string(&payload, "resultRef"),
            )?),

            "prompt.register" => {
                self.services
                    .register_prompt_section(from_payload::<PromptSection>(payload)?)?;
                Ok(Value::Null)
            }
            "prompt.assemble" => Ok(
                json!({"prompt": self.services.assemble_prompt(optional_str(&payload, "base").unwrap_or(""))?}),
            ),
            "context.inject" => {
                self.services
                    .inject_context(from_payload::<ContextFragment>(payload)?)?;
                Ok(Value::Null)
            }
            "context.list" => to_value(self.services.context_fragments()?),

            "workspace.register" => to_value(self.services.register_workspace(
                required_string(&payload, "root")?,
                required_string(&payload, "label")?,
            )?),
            "workspace.list" => to_value(self.services.workspaces()?),
            "settings.set" => to_value(self.services.set_setting(
                required_string(&payload, "key")?,
                payload.get("value").cloned().unwrap_or(Value::Null),
            )?),
            "settings.get" => to_value(self.services.get_setting(required_str(&payload, "key")?)?),

            "todo.add" => to_value(self.services.add_todo(required_string(&payload, "text")?)?),
            "todo.update" => to_value(
                self.services
                    .update_todo(required_str(&payload, "todoId")?, todo_done(&payload)?)?,
            ),
            "plan.set" => to_value(
                self.services.set_plan(
                    required_string(&payload, "sessionId")?,
                    string_array(&payload, "steps")?,
                    payload
                        .get("reviewRequired")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )?,
            ),
            "plan.exit" => to_value(
                self.services.exit_plan(
                    required_str(&payload, "sessionId")?,
                    payload
                        .get("approved")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )?,
            ),
            "agentPlan.enter" => to_value(
                self.advanced.enter_plan(
                    required_string(&payload, "agentId")?,
                    required_string(&payload, "sessionId")?,
                    string_array(&payload, "steps")?,
                    payload
                        .get("reviewRequired")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                )?,
            ),
            "agentPlan.exit" => to_value(
                self.advanced.exit_plan(
                    required_str(&payload, "agentId")?,
                    payload
                        .get("approved")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    optional_string(&payload, "reviewer"),
                )?,
            ),
            "agentPlan.get" => to_value(
                self.advanced
                    .agent_plan(required_str(&payload, "agentId")?)?,
            ),

            "schedule.create" => to_value(self.services.schedule(
                required_string(&payload, "sessionId")?,
                required_string(&payload, "instruction")?,
                required_string(&payload, "schedule")?,
            )?),
            "schedule.setEnabled" => to_value(self.services.set_schedule_enabled(
                required_str(&payload, "scheduleId")?,
                required_bool(&payload, "enabled")?,
            )?),
            "schedule.list" => to_value(self.services.list_schedules()?),
            "schedule.get" => to_value(
                self.services
                    .get_schedule(required_str(&payload, "scheduleId")?)?,
            ),
            "schedule.delete" => to_value(
                self.services
                    .delete_schedule(required_str(&payload, "scheduleId")?)?,
            ),
            "feedback.record" => to_value(self.services.record_feedback(
                optional_string(&payload, "sessionId"),
                optional_i32(&payload, "rating"),
                optional_string(&payload, "note").or_else(|| optional_string(&payload, "text")),
            )?),
            "identity.get" => to_value(self.services.identity()?),
            "identity.bindAccount" => to_value(self.services.bind_identity_account(
                Some(required_string(&payload, "accountId")?),
                optional_string(&payload, "displayName"),
            )?),

            "team.create" => to_value(
                self.services.create_team(
                    required_string(&payload, "name")?,
                    payload
                        .get("members")
                        .cloned()
                        .map(from_payload::<Vec<TeamMember>>)
                        .transpose()?
                        .unwrap_or_default(),
                )?,
            ),
            "team.addTask" => to_value(self.services.add_team_task(
                required_str(&payload, "teamId")?,
                optional_string(&payload, "assigneeAgentId").unwrap_or_else(|| "unassigned".into()),
                required_string(&payload, "text")?,
            )?),
            "team.sendMessage" => to_value(
                self.services.send_team_message(
                    required_str(&payload, "teamId")?,
                    required_string(&payload, "fromAgentId")?,
                    optional_string(&payload, "toAgentId"),
                    payload
                        .get("message")
                        .cloned()
                        .unwrap_or(Value::String(required_string(&payload, "text")?)),
                )?,
            ),

            "skill.register" => {
                self.services
                    .register_skill(from_payload::<SkillRecord>(payload)?)?;
                Ok(Value::Null)
            }
            "command.register" => {
                self.services
                    .register_command(from_payload::<CommandRecord>(payload)?)?;
                Ok(Value::Null)
            }

            "interaction.permissionPreset.register" => to_value(
                self.advanced
                    .register_permission_preset(from_payload::<PermissionPreset>(payload)?)?,
            ),
            "interaction.permissionPreset.list" => to_value(self.advanced.permission_presets()?),
            "interaction.permissionPreset.activate" => to_value(
                self.advanced
                    .activate_permission_preset(required_str(&payload, "presetId")?)?,
            ),
            "interaction.permissionPreset.current" => {
                to_value(self.advanced.active_permission_preset()?)
            }
            "interaction.question.create" => to_value(
                self.advanced.ask_question(
                    required_string(&payload, "sessionId")?,
                    optional_string(&payload, "agentId"),
                    required_string(&payload, "prompt")?,
                    payload
                        .get("options")
                        .cloned()
                        .map(from_payload::<Vec<QuestionOption>>)
                        .transpose()?
                        .unwrap_or_default(),
                    payload
                        .get("allowFreeform")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )?,
            ),
            "interaction.question.answer" => to_value(self.advanced.answer_question(
                required_str(&payload, "questionId")?,
                payload.get("answer").cloned().unwrap_or(Value::Null),
            )?),
            "interaction.question.list" => to_value(
                self.advanced.questions(
                    payload
                        .get("pendingOnly")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )?,
            ),

            "bundle.register" => to_value(
                self.advanced
                    .register_bundle(from_payload::<BundleDefinition>(payload)?)?,
            ),
            "bundle.list" => to_value(self.advanced.bundles()?),
            "preset.register" => to_value(
                self.advanced
                    .register_preset(from_payload::<PresetDefinition>(payload)?)?,
            ),
            "preset.list" => to_value(self.advanced.presets()?),
            "preset.resolve" => to_value(
                self.advanced
                    .resolve_preset(required_str(&payload, "presetId")?)?,
            ),
            "extension.register" => to_value(self.advanced.register_extension(from_payload::<
                ExtensionDefinition,
            >(
                payload
            )?)?),
            "extension.list" => to_value(self.advanced.extensions()?),
            "extension.setEnabled" => to_value(self.advanced.set_extension_enabled(
                required_str(&payload, "extensionId")?,
                required_bool(&payload, "enabled")?,
            )?),
            "hook.register" => to_value(
                self.advanced
                    .register_hook(from_payload::<HookRegistration>(payload)?)?,
            ),
            "hook.list" => to_value(self.advanced.hooks()?),
            "hook.emit" => to_value(self.advanced.emit_hook(
                required_string(&payload, "event")?,
                payload.get("eventPayload").cloned().unwrap_or(Value::Null),
            )?),
            "telemetry.record" => to_value(self.advanced.record_telemetry(
                required_string(&payload, "name")?,
                payload.get("attributes").cloned().unwrap_or(Value::Null),
            )?),
            "telemetry.poll" => to_value(
                self.advanced
                    .poll_telemetry(optional_usize(&payload, "limit").unwrap_or(100))?,
            ),

            _ => Err(HarnessError::ServiceNotFound(format!(
                "protocol operation {operation}"
            ))),
        }
    }
}

fn error_to_protocol(error: HarnessError) -> HarnessProtocolError {
    let code = match error {
        HarnessError::ApprovalRequired(_) => "approval_required",
        HarnessError::ApprovalNotFound(_) => "approval_not_found",
        HarnessError::SessionNotFound(_) => "session_not_found",
        HarnessError::AgentNotFound(_) => "agent_not_found",
        HarnessError::ToolNotFound(_) => "tool_not_found",
        HarnessError::ServiceNotFound(_) => "not_found",
        HarnessError::InvalidConfig(_) => "invalid_config",
        _ => "harness_error",
    };
    HarnessProtocolError {
        code: code.into(),
        message: error.to_string(),
    }
}

fn from_payload<T>(payload: Value) -> HarnessResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(payload).map_err(|error| HarnessError::InvalidConfig(error.to_string()))
}

fn to_value<T: Serialize>(value: T) -> HarnessResult<Value> {
    serde_json::to_value(value).map_err(|error| HarnessError::InvalidConfig(error.to_string()))
}

fn required_str<'a>(payload: &'a Value, key: &str) -> HarnessResult<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| HarnessError::InvalidConfig(format!("{key} is required")))
}

fn optional_str<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn required_string(payload: &Value, key: &str) -> HarnessResult<String> {
    required_str(payload, key).map(str::to_string)
}

fn optional_string(payload: &Value, key: &str) -> Option<String> {
    optional_str(payload, key).map(str::to_string)
}

fn required_bool(payload: &Value, key: &str) -> HarnessResult<bool> {
    payload
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| HarnessError::InvalidConfig(format!("{key} is required")))
}

fn todo_done(payload: &Value) -> HarnessResult<bool> {
    if let Some(done) = payload.get("done").and_then(Value::as_bool) {
        return Ok(done);
    }
    match required_str(payload, "status")? {
        "done" | "completed" | "complete" => Ok(true),
        "pending" | "open" | "todo" => Ok(false),
        status => Err(HarnessError::InvalidConfig(format!(
            "unsupported todo status: {status}"
        ))),
    }
}

fn optional_i32(payload: &Value, key: &str) -> Option<i32> {
    payload
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn optional_usize(payload: &Value, key: &str) -> Option<usize> {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn string_array(payload: &Value, key: &str) -> HarnessResult<Vec<String>> {
    let array = payload
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::InvalidConfig(format!("{key} must be an array")))?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| HarnessError::InvalidConfig(format!("{key} must contain strings")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_protocol_round_trips_session_and_search() {
        let api = HarnessApi::new(BuildProfile::DesktopFull);
        let created: HarnessResponse = serde_json::from_str(&api.execute_json(
            r#"{"requestId":"1","operation":"session.create","payload":{"title":"protocol"}}"#,
        ))
        .unwrap();
        assert!(created.ok);
        let session_id = created.result["id"].as_str().unwrap().to_string();

        let append = HarnessRequest {
            request_id: "2".into(),
            operation: "session.appendEvent".into(),
            payload: json!({
                "sessionId": session_id,
                "kind": "user/message",
                "eventPayload": {"text": "needle protocol"}
            }),
        };
        let append: HarnessResponse =
            serde_json::from_str(&api.execute_json(&serde_json::to_string(&append).unwrap()))
                .unwrap();
        assert!(append.ok);

        let search = api
            .execute("session.search", json!({"query": "needle", "limit": 10}))
            .unwrap();
        assert_eq!(search.as_array().unwrap().len(), 1);
    }

    #[test]
    fn advanced_protocol_covers_interaction_plan_and_lineage() {
        let api = HarnessApi::new(BuildProfile::DesktopFull);
        let session = api
            .execute("session.create", json!({"title": "advanced"}))
            .unwrap();
        let session_id = session["id"].as_str().unwrap();
        let plan = api
            .execute(
                "agentPlan.enter",
                json!({
                    "agentId": "agent-a",
                    "sessionId": session_id,
                    "steps": ["inspect", "implement"],
                    "reviewRequired": true
                }),
            )
            .unwrap();
        assert_eq!(plan["mode"], "plan");
        let denied = api.execute(
            "agentPlan.exit",
            json!({"agentId": "agent-a", "approved": false}),
        );
        assert!(matches!(denied, Err(HarnessError::ApprovalRequired(_))));

        let question = api
            .execute(
                "interaction.question.create",
                json!({
                    "sessionId": session_id,
                    "prompt": "Choose",
                    "options": [{"id": "yes", "label": "Yes"}]
                }),
            )
            .unwrap();
        let question_id = question["id"].as_str().unwrap();
        let answered = api
            .execute(
                "interaction.question.answer",
                json!({"questionId": question_id, "answer": "yes"}),
            )
            .unwrap();
        assert_eq!(answered["status"], "answered");

        let child = api
            .execute("session.fork", json!({"sessionId": session_id}))
            .unwrap();
        let lineage = api
            .execute(
                "session.lineage",
                json!({"sessionId": child["id"].as_str().unwrap()}),
            )
            .unwrap();
        assert_eq!(lineage["ancestors"][0], session_id);
    }

    #[test]
    fn unknown_operation_returns_stable_error() {
        let api = HarnessApi::new(BuildProfile::DesktopFull);
        let response: HarnessResponse = serde_json::from_str(
            &api.execute_json(r#"{"requestId":"x","operation":"missing.operation"}"#),
        )
        .unwrap();
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "not_found");
    }
}
