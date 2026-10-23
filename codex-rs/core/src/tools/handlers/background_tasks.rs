use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::unified_exec::background::BackgroundOptions;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::collections::BTreeMap;

pub struct BackgroundTasksHandler;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    action: String,
    process_id: Option<i32>,
    #[serde(default)]
    options: BackgroundOptions,
}

impl ToolExecutor<ToolInvocation> for BackgroundTasksHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("background_tasks")
    }
    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: "background_tasks".into(),
            description: "Manage this conversation's server-owned background tasks. action=list lists status; read returns a bounded output tail; stop cancels one task without a wake-up; adopt registers an existing exec_command session for completion wake-ups. Completion automatically resumes the conversation: do not poll this tool while waiting. The user can inspect tasks with /tasks without model calls.".into(),
            strict: false, defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::from([
                ("action".into(), JsonSchema::string(Some("list, read, stop, or adopt".into()))),
                ("process_id".into(), JsonSchema::number(Some("Required for read, stop and adopt; the exec_command session ID.".into()))),
                ("options".into(), JsonSchema::object(BTreeMap::from([
                    ("label".into(), JsonSchema::string(None)),
                    ("monitor".into(), JsonSchema::boolean(None)),
                    ("notify_pattern".into(), JsonSchema::string(None)),
                ]), None, Some(false.into()))),
            ]), Some(vec!["action".into()]), Some(false.into())),
            output_schema: None,
        })
    }
    fn handle<'a>(&'a self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'a>
    where
        ToolInvocation: 'a,
    {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = &invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "Expected function arguments".into(),
                ));
            };
            let args: Args = parse_arguments(arguments)?;
            let session = &invocation.session;
            let manager = &session.services.unified_exec_manager;
            let require_id = || {
                args.process_id.ok_or_else(|| {
                    FunctionCallError::RespondToModel("process_id is required".into())
                })
            };
            let value = match args.action.as_str() {
                "list" => {
                    let mut tasks = manager.background_task_list(session).await;
                    for task in &mut tasks {
                        task.output.clear();
                    }
                    serde_json::json!({"tasks": tasks})
                }
                "read" => {
                    let id = require_id()?.to_string();
                    let task = manager
                        .background_task_list(session)
                        .await
                        .into_iter()
                        .find(|t| t.process_id == id)
                        .ok_or_else(|| {
                            FunctionCallError::RespondToModel("Unknown background task".into())
                        })?;
                    serde_json::json!(task)
                }
                "stop" => {
                    serde_json::json!({"stopped": manager.terminate_process(require_id()?).await})
                }
                "adopt" => {
                    manager
                        .background(session, require_id()?, args.options)
                        .await
                        .map_err(FunctionCallError::RespondToModel)?;
                    serde_json::json!({"registered": true, "instruction": "End your turn while waiting; completion wakes this conversation automatically."})
                }
                _ => {
                    return Err(FunctionCallError::RespondToModel(
                        "action must be list, read, stop, or adopt".into(),
                    ));
                }
            };
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                value.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for BackgroundTasksHandler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}
