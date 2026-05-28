//! ReAct-style agent loop that drives Claude through tool calls.
//!
//! The agent's contract is mirrored in `agent.mty` (a Mighty source
//! spec — see `bench/swe/agent.mty`) but the real driver lives here
//! in Rust so the harness can run without a full Mighty build chain
//! installed on the executor.
//!
//! Loop:
//!   1. Send `(system, history, tools)` to Claude.
//!   2. If response is text only -> append to history, ask Claude to
//!      either keep working or call `submit`.
//!   3. If response contains `tool_use` blocks -> execute each one
//!      under the workspace's capability set, append the
//!      `tool_result` blocks, loop.
//!   4. Stop on:
//!      * Claude calls `submit`
//!      * Max turn budget exhausted (default: 25 turns)
//!      * Dollar budget exhausted
//!      * Per-instance wall-clock cap (default: 5 minutes)

use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use crate::llm::{Client, ContentBlock, Message, Response, Role, Usage};
use crate::tools::{
    self, ApplyPatchArgs, ListDirArgs, ReadFileArgs, RunTestsArgs, SubmitArgs, Workspace,
    WriteFileArgs,
};

pub struct AgentRun {
    /// Echo of the instance ID — useful when the caller fans out
    /// multiple `run_agent` calls and needs to associate results.
    #[allow(dead_code)]
    pub instance_id: String,
    pub turns: u32,
    pub submitted: bool,
    pub final_summary: Option<String>,
    pub usage: Usage,
    pub cost_usd: f64,
    pub wall_time: Duration,
    pub stop_reason: StopReason,
    /// Per-turn trace. Held for debug / future `--trace-out` support.
    #[allow(dead_code)]
    pub trace: Vec<TraceEvent>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum StopReason {
    Submitted,
    TurnBudget,
    WallClock,
    DollarBudget,
    ApiError(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceEvent {
    pub turn: u32,
    pub kind: String,
    pub payload: Value,
}

pub struct AgentConfig {
    pub model: String,
    pub max_turns: u32,
    pub max_seconds: u64,
    pub max_tokens_per_call: u32,
    pub per_instance_dollar_cap: f64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "claude-opus-4-7".into(),
            max_turns: 25,
            max_seconds: 300,
            max_tokens_per_call: 4096,
            per_instance_dollar_cap: 3.0,
        }
    }
}

const SYSTEM_PROMPT: &str = r#"You are a SWE-bench agent built on Mighty.
You are dropped into a checked-out git repository at the buggy commit.
Your job: read the problem statement, explore the code, write a patch
that makes the failing tests pass without breaking the passing tests,
and call `submit` when you believe you're done.

Rules of engagement:
- Always start with `list_dir` and a few targeted `read_file` calls.
- Make minimal, focused edits — do NOT rewrite unrelated code.
- Prefer `apply_patch` over `write_file` when you have a clean diff.
- Run `run_tests` against the failing-test selector before submitting.
- If you hit the same dead end twice, change strategy.
- Be concise in your prose — the tool calls are what move the work.

Capability surface (enforced by the runtime, not by you):
  read_file  : fs.read(<repo>)
  write_file : fs.write(<repo>)
  list_dir   : fs.read(<repo>)
  apply_patch: fs.write(<repo>)
  run_tests  : cmd.exec("pytest")
  submit     : pure marker

Any tool call outside its capability is refused. Plan accordingly."#;

pub async fn run_agent(
    client: &Client,
    workspace: &Workspace,
    problem_statement: &str,
    failing_tests: &[String],
    instance_id: &str,
    config: &AgentConfig,
) -> Result<AgentRun> {
    let started = Instant::now();
    let mut history: Vec<Message> = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: format!(
                "Problem statement:\n\n{}\n\nFailing tests:\n{}\n\nStart by exploring the repo.",
                problem_statement,
                failing_tests.join("\n")
            ),
        }],
    }];
    let mut total_usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut total_cost = 0.0_f64;
    let mut trace = Vec::new();
    let mut submitted = false;
    let mut final_summary: Option<String> = None;
    let mut stop_reason = StopReason::TurnBudget;
    let tools = tools::tool_specs();
    let mut turn = 0_u32;

    while turn < config.max_turns {
        if started.elapsed() > Duration::from_secs(config.max_seconds) {
            stop_reason = StopReason::WallClock;
            break;
        }
        if total_cost > config.per_instance_dollar_cap {
            stop_reason = StopReason::DollarBudget;
            break;
        }
        turn += 1;
        let resp: Response = match client
            .messages(
                &config.model,
                SYSTEM_PROMPT,
                &history,
                &tools,
                config.max_tokens_per_call,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{e}");
                trace.push(TraceEvent {
                    turn,
                    kind: "api_error".into(),
                    payload: Value::String(msg.clone()),
                });
                stop_reason = StopReason::ApiError(msg);
                break;
            }
        };

        total_usage.input_tokens += resp.usage.input_tokens;
        total_usage.output_tokens += resp.usage.output_tokens;
        total_cost += crate::llm::cost_usd(&config.model, &resp.usage);
        trace.push(TraceEvent {
            turn,
            kind: "assistant".into(),
            payload: serde_json::json!({
                "stop_reason": resp.stop_reason,
                "blocks": resp.content.len(),
                "cum_cost": total_cost,
            }),
        });

        // Append assistant message
        history.push(Message {
            role: Role::Assistant,
            content: resp.content.clone(),
        });

        // Collect tool uses; if none, nudge once and stop if still none.
        let tool_uses: Vec<&ContentBlock> = resp
            .content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect();

        if tool_uses.is_empty() {
            // Text-only reply. Nudge once to call a tool or submit.
            if resp
                .stop_reason
                .as_deref()
                .map(|s| s == "end_turn")
                .unwrap_or(false)
                && resp
                    .content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Text { text } if text.contains("DONE")))
            {
                // Treat explicit "DONE" as submission with empty summary
                submitted = true;
                final_summary = Some("agent emitted DONE without submit() call".into());
                stop_reason = StopReason::Submitted;
                break;
            }
            history.push(Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Please call a tool to make progress, or call `submit` if you're done."
                        .into(),
                }],
            });
            continue;
        }

        // Execute each tool use; collect results.
        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for b in tool_uses {
            let (id, name, input) = match b {
                ContentBlock::ToolUse { id, name, input } => {
                    (id.clone(), name.clone(), input.clone())
                }
                _ => unreachable!(),
            };
            trace.push(TraceEvent {
                turn,
                kind: format!("tool:{name}"),
                payload: input.clone(),
            });
            let (result, is_error) = match name.as_str() {
                "read_file" => match serde_json::from_value::<ReadFileArgs>(input) {
                    Ok(args) => match tools::exec_read_file(workspace, args).await {
                        Ok(s) => (s, false),
                        Err(e) => (format!("ERROR: {e}"), true),
                    },
                    Err(e) => (format!("BAD_ARGS: {e}"), true),
                },
                "write_file" => match serde_json::from_value::<WriteFileArgs>(input) {
                    Ok(args) => match tools::exec_write_file(workspace, args).await {
                        Ok(s) => (s, false),
                        Err(e) => (format!("ERROR: {e}"), true),
                    },
                    Err(e) => (format!("BAD_ARGS: {e}"), true),
                },
                "list_dir" => match serde_json::from_value::<ListDirArgs>(input) {
                    Ok(args) => match tools::exec_list_dir(workspace, args).await {
                        Ok(s) => (s, false),
                        Err(e) => (format!("ERROR: {e}"), true),
                    },
                    Err(e) => (format!("BAD_ARGS: {e}"), true),
                },
                "apply_patch" => match serde_json::from_value::<ApplyPatchArgs>(input) {
                    Ok(args) => match tools::exec_apply_patch(workspace, args).await {
                        Ok(s) => (s, false),
                        Err(e) => (format!("ERROR: {e}"), true),
                    },
                    Err(e) => (format!("BAD_ARGS: {e}"), true),
                },
                "run_tests" => match serde_json::from_value::<RunTestsArgs>(input) {
                    Ok(args) => match tools::exec_run_tests(workspace, args).await {
                        Ok(s) => (s, false),
                        Err(e) => (format!("ERROR: {e}"), true),
                    },
                    Err(e) => (format!("BAD_ARGS: {e}"), true),
                },
                "submit" => {
                    let summary = serde_json::from_value::<SubmitArgs>(input)
                        .map(|a| a.summary)
                        .unwrap_or_default();
                    submitted = true;
                    final_summary = Some(summary.clone());
                    (format!("submission recorded: {summary}"), false)
                }
                other => (format!("UNKNOWN_TOOL: {other}"), true),
            };
            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: result,
                is_error: if is_error { Some(true) } else { None },
            });
        }
        history.push(Message {
            role: Role::User,
            content: tool_results,
        });

        if submitted {
            stop_reason = StopReason::Submitted;
            break;
        }
    }

    Ok(AgentRun {
        instance_id: instance_id.to_string(),
        turns: turn,
        submitted,
        final_summary,
        usage: Usage {
            input_tokens: total_usage.input_tokens,
            output_tokens: total_usage.output_tokens,
        },
        cost_usd: total_cost,
        wall_time: started.elapsed(),
        stop_reason,
        trace,
    })
}
