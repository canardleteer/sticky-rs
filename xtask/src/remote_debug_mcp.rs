//! stdio MCP for `remote-debug --mcp` (rmcp, not clap-mcp).
//!
//! Tool names are the clap leaves. This process is a ConnectRPC client;
//! GATT lives in the owner. Never a MAC.

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use remote_debug_host::DEFAULT_ADV_NAME;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation, ListPromptsResult,
    ListResourcesResult, PaginatedRequestParams, Prompt, PromptMessage, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, Role, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::remote_debug::{
    run, BrokerTarget, ConnectArgs, GetSnapshotArgs, InjectButtonArgs, InjectTouchArgs,
    ListTargetsArgs, LogsArgs, RebootArgs, RemoteDebugCommand, RemoteDebugState, SnapshotAckArgs,
};

/// Initialize / discover instructions for a desk sit.
pub const INSTRUCTIONS: &str = "\
You are a client of the sticky-rs remote-debug owner (encrypted GATT after \
DisplayOnly pair). This process does not own GATT. Tools are clap leaves: \
connect, status, list-targets, inject-touch, inject-button, get-snapshot, \
snapshot-ack, snapshot-clear, reboot, disconnect, logs. Not remote-debug_*.

Stay on splash for pair (Ferris never shows the PIN). Do not ask the operator \
to pair from a phone. Do not run monitor during auto-PIN. No --remember unless \
the human asked. Never a MAC, serial, PIN, or snapshot planes in git.

Loop: connect (returns pairing) → poll status until connected or pair failed \
→ use → disconnect. After a fresh flash, retry connect on \
le-connection-abort-by-local or CDC busy.

inject-touch --page uses page pixels for the last compose hold (gray4 \
hit-test inverse). --phase down/move/up (unset = tap). Do not use UART p0=. \
Wait ~2–3 s after inject for compose. Do not tap Wi-Fi START unless asked.

get-snapshot writes a page-space .png (open the png field; portrait \
480×800 or landscape 800×480). Sibling .bw / .red are packed SSD1677 \
planes; .red is the second gray4 plane, not pigment. Structured JSON \
omits those bytes and adds png. Tap --page at snapshot expect, not a \
guessed landscape centre. A leftover arm is SnapshotBusy; \
snapshot-clear then retry. Ack when done.

Read sticky-rs://remote-debug/pickup before the next sit. Prompts: desk-sit, \
targets-walk, after-failed-snapshot.";

/// Pickup card (same facts as the skill page, no client names).
const PICKUP: &str = "\
# Pickup

Stay on splash. Do not ask the operator to pair from a phone.
No monitor during auto-PIN. No --remember unless asked. Never a MAC.

1. status — no owner until connect (endpoint file missing).
2. connect → pairing. Poll status until connected (or pair failed).
3. inject-button page-down / page-up, or inject-touch --page at expect.
   --phase down / move / up (unset = tap). Wait ~2–3 s. Do not tap START.
4. get-snapshot — LAST DRAW plus scene / hold / target_step / expect.
   Open the png field (page space, same origin as --page). .bw /
   .red are SSD1677 planes (.red = gray4 plane 1, not pigment);
   JSON omits those bytes. Ack when done. SnapshotBusy →
   snapshot-clear, then retry.
5. Targets: seven page-downs to scene=7(targets). Tap --page at expect.
   Slides use --phase. After id 6, target_step=0. last_log may be
   target show id=0. logs shows the Target / Scene ring (oldest first).
6. list-targets shows advertise names the owner holds. disconnect when
   the sit is over (empty map also shuts the owner down).
";

const TOOLS: &str = "\
# Tools

| Tool | How to use it |
| --- | --- |
| connect | Starts the owner if needed. Returns pairing. BlueZ Connect, not Pair(). UART auto-PIN unless pin. `--name` is advertise `target`. |
| status | pairing / connected / disconnected / pair failed. last_log is Target / Scene only. last_scene / hold / expect is the last snapshot cache (stale after inject). |
| logs | Drain the Target / Scene ring (oldest first, never a PIN or MAC). Optional limit (default 16). |
| list-targets | Advertise names the owner currently tracks (never a MAC). |
| inject-touch | Default x/y are framebuffer. page=true treats x/y as page pixels. phase down/move/up; unset is a tap. Not UART p0=. |
| inject-button | key ok / page-up / page-down. down true is a short press (JSON bool). CLI uses --release for the up edge. Wait for compose. |
| get-snapshot | Arms LAST DRAW. JSON png is the page-space image to open. Sibling .bw / .red are SSD1677 planes (.red = gray4 plane 1, not pigment) and are omitted from JSON. scene / hold / step / expect on the line. |
| snapshot-ack | Release the armed nonce. |
| snapshot-clear | Abort with no nonce. Use after a failed get. |
| reboot | Software-reset the embedded MCU (not this host). Re-pairs unless no_reconnect. |
| disconnect | Drop one GATT session. Empty map also shuts the owner down. Unknown units lose the BlueZ bond. |

Structured output is the generated ConnectRPC `*Response` JSON, plus \
host-only `png` on get-snapshot. `message` must not include a MAC.
";

const SNAPSHOT: &str = "\
# Snapshot PNG

.bw / .red stay pre-rotation 800×480 SSD1677 planes (48 KiB
each). `.red` is the second gray4 plane (controller name), not
a red pigment. Open the `png` field for ink. Structured
`get-snapshot` JSON drops plane bytes and adds that path.

.png is rematerialized in **page** space for Snapshot.hold:

- hold 0 Portrait0 / 1 Portrait180 → 480×800
- hold 2 Landscape0 / 3 Landscape180 → 800×480

Page (0,0) is the top-left of the card as composed (same origin as \
inject-touch --page and target expect). Gray4 planes undo the Seeed OTP \
180° write so text is upright. Do not guess a landscape centre from the \
raw 800×480 `.bw` dump.

After get-snapshot, ack. A second get while armed is SnapshotBusy.
";

const DESK_SIT: &str = "\
Run a sticky-rs remote-debug desk sit.

Stay on splash. Do not ask anyone to pair from a phone. No monitor. \
No --remember. Never a MAC.

1. Read resource sticky-rs://remote-debug/pickup.
2. status. If no broker, connect (expect pairing).
3. Poll status until connected. Retry connect after flash if the first \
sit is le-connection-abort-by-local or CDC busy.
4. get-snapshot. Confirm Ferris only (no six-digit boxes). Open png \
(page space). Ack.
5. disconnect when done.

Read sticky-rs://remote-debug/snapshot if the PNG aspect looks wrong.
";

const TARGETS_WALK: &str = "\
Walk Scene::Targets over remote-debug (no monitor).

1. Seven inject-button page-down shorts. Wait ~2–3 s each. Do not tap \
Wi-Fi START.
2. get-snapshot. scene should be 7. Note hold, target_step, expect.
3. For a dot: inject-touch page=true at expect (one tap). Wait. Snapshot.
4. For a slide: --phase down at one inset, move at mid and the other \
inset, up. Wait. Snapshot.
5. After id 6, target_step is 0. last_log may be target show id=0 \
(target loop was the previous line).
6. Ack each get. disconnect when done.

Tap snapshot expect, not a guessed landscape centre. Read \
sticky-rs://remote-debug/snapshot.
";

const AFTER_FAILED: &str = "\
The last get-snapshot failed (frame: Version, timeout, or SnapshotBusy).

1. snapshot-clear (no nonce).
2. get-snapshot again.
3. If it still fails, disconnect, cargo build -p xtask, connect, retry. \
Host notify must be FIFO.
4. Ack a successful get.

Do not stack another get while armed.
";

/// ConnectRPC `*Response` JSON object for MCP `outputSchema`.
///
/// `serde_json::Value` is schemars `AnyValue` (boolean `true`). Some MCP
/// clients drop `tools/list` unless `outputSchema.type` is the literal
/// `"object"`. `additionalProperties` stays true so real response fields
/// (`message`, `targets`, `snapshot`, `png`) are not rejected.
#[derive(serde::Serialize)]
#[serde(transparent)]
struct RemoteDebugToolOutput(Value);

impl JsonSchema for RemoteDebugToolOutput {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RemoteDebugResponse".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

fn default_name() -> String {
    DEFAULT_ADV_NAME.to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConnectParams {
    pin: Option<u32>,
    port: Option<String>,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default)]
    remember: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NameParams {
    #[serde(default = "default_name")]
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InjectTouchParams {
    x: u16,
    y: u16,
    slot: Option<u8>,
    phase: Option<String>,
    #[serde(default)]
    page: bool,
    #[serde(default = "default_name")]
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InjectButtonParams {
    key: String,
    #[serde(default = "default_true")]
    down: bool,
    #[serde(default = "default_name")]
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NonceParams {
    nonce: Option<u64>,
    #[serde(default = "default_name")]
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LogsParams {
    limit: Option<u32>,
    #[serde(default = "default_name")]
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RebootParams {
    #[serde(default)]
    no_reconnect: bool,
    pin: Option<u32>,
    port: Option<String>,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default)]
    remember: bool,
}

/// stdio MCP handler. Clone so rmcp can share the session.
#[derive(Clone)]
pub struct RemoteDebugMcp {
    state: Arc<Mutex<RemoteDebugState>>,
}

impl RemoteDebugMcp {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RemoteDebugState::new())),
        }
    }

    fn dispatch(&self, cmd: RemoteDebugCommand) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        run(cmd, self.state.as_ref())
            .map(|value| Json(RemoteDebugToolOutput(value)))
            .map_err(|message| McpError::invalid_params(message, None))
    }
}

#[tool_router]
impl RemoteDebugMcp {
    #[tool(
        name = "connect",
        description = "Start pair (returns pairing; poll status until connected). BlueZ Connect, not Pair(). UART auto-PIN unless pin. Stay on splash. Never a MAC.",
        annotations(open_world_hint = true)
    )]
    fn connect(
        &self,
        Parameters(params): Parameters<ConnectParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::Connect(ConnectArgs {
            pin: params.pin,
            port: params.port,
            name: params.name,
            remember: params.remember,
            wait: false,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "status",
        description = "pairing / connected / disconnected / pair failed",
        annotations(read_only_hint = true, idempotent_hint = true)
    )]
    fn status(
        &self,
        Parameters(params): Parameters<NameParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::Status(broker_target(params.name)))
    }

    #[tool(
        name = "list-targets",
        description = "Advertise names the owner currently tracks (never a MAC)",
        annotations(read_only_hint = true, idempotent_hint = true)
    )]
    fn list_targets(&self) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::ListTargets(ListTargetsArgs {
            socket_dir: None,
        }))
    }

    #[tool(
        name = "inject-touch",
        description = "Synthetic tap. Default x/y are framebuffer. page=true treats them as page pixels. phase down/move/up; unset is a tap. Not UART p0=."
    )]
    fn inject_touch(
        &self,
        Parameters(params): Parameters<InjectTouchParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::InjectTouch(InjectTouchArgs {
            x: params.x,
            y: params.y,
            slot: params.slot,
            phase: params.phase,
            page: params.page,
            name: params.name,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "inject-button",
        description = "Short-press ok / page-up / page-down. down true is a short press. Wait for compose."
    )]
    fn inject_button(
        &self,
        Parameters(params): Parameters<InjectButtonParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::InjectButton(InjectButtonArgs {
            key: params.key,
            release: !params.down,
            name: params.name,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "get-snapshot",
        description = "Arm LAST DRAW. JSON png is the page-space image to open. Sibling .bw / .red are SSD1677 planes (.red = gray4 plane 1, not pigment)."
    )]
    fn get_snapshot(
        &self,
        Parameters(params): Parameters<NonceParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::GetSnapshot(GetSnapshotArgs {
            nonce: params.nonce,
            name: params.name,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "snapshot-ack",
        description = "Release the armed nonce",
        annotations(idempotent_hint = true)
    )]
    fn snapshot_ack(
        &self,
        Parameters(params): Parameters<NonceParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::SnapshotAck(SnapshotAckArgs {
            nonce: params.nonce,
            name: params.name,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "snapshot-clear",
        description = "Operator abort (no nonce); use after a failed get",
        annotations(idempotent_hint = true)
    )]
    fn snapshot_clear(
        &self,
        Parameters(params): Parameters<NameParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::SnapshotClear(broker_target(
            params.name,
        )))
    }

    #[tool(
        name = "reboot",
        description = "Software-reset the embedded MCU (not this host). Re-pairs unless no_reconnect.",
        annotations(destructive_hint = true, open_world_hint = true)
    )]
    fn reboot(
        &self,
        Parameters(params): Parameters<RebootParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::Reboot(RebootArgs {
            no_reconnect: params.no_reconnect,
            pin: params.pin,
            port: params.port,
            name: params.name,
            remember: params.remember,
            socket_dir: None,
        }))
    }

    #[tool(
        name = "disconnect",
        description = "Drop one GATT session. Empty map also shuts the owner down. Unknown units lose the BlueZ bond.",
        annotations(destructive_hint = true)
    )]
    fn disconnect(
        &self,
        Parameters(params): Parameters<NameParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::Disconnect(broker_target(params.name)))
    }

    #[tool(
        name = "logs",
        description = "Drain Target / Scene LogLine copies (oldest first). Never a PIN or MAC. Optional limit (default 16).",
        annotations(read_only_hint = true)
    )]
    fn logs(
        &self,
        Parameters(params): Parameters<LogsParams>,
    ) -> Result<Json<RemoteDebugToolOutput>, McpError> {
        self.dispatch(RemoteDebugCommand::Logs(LogsArgs {
            limit: params.limit,
            name: params.name,
            socket_dir: None,
        }))
    }
}

fn broker_target(name: String) -> BrokerTarget {
    BrokerTarget {
        name,
        socket_dir: None,
    }
}

#[tool_handler]
impl ServerHandler for RemoteDebugMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_instructions(INSTRUCTIONS)
        .with_server_info(
            Implementation::new("sticky-rs-remote-debug", env!("CARGO_PKG_VERSION"))
                .with_title("sticky-rs remote-debug")
                .with_website_url("https://github.com/canardleteer/sticky-rs"),
        )
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            prompts: prompts(),
            ..Default::default()
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let (description, text) = match request.name.as_str() {
            "desk-sit" => ("Pair on splash, snapshot, ack, disconnect", DESK_SIT),
            "targets-walk" => ("Score all seven targets from snapshot expect", TARGETS_WALK),
            "after-failed-snapshot" => {
                ("Clear a leftover arm and retry get-snapshot", AFTER_FAILED)
            }
            _ => {
                return Err(McpError::invalid_params(
                    format!("unknown prompt {}", request.name),
                    None,
                ));
            }
        };
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
                .with_description(description)
                .into(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: resources(),
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let body = match request.uri.as_str() {
            "sticky-rs://remote-debug/pickup" => PICKUP,
            "sticky-rs://remote-debug/tools" => TOOLS,
            "sticky-rs://remote-debug/snapshot" => SNAPSHOT,
            _ => {
                return Err(McpError::resource_not_found(
                    format!("unknown resource {}", request.uri),
                    None,
                ));
            }
        };
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(body, request.uri).with_mime_type("text/markdown")
        ])
        .into())
    }
}

fn prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "desk-sit",
            Some("Pair on splash, snapshot, ack, disconnect"),
            None,
        )
        .with_title("Pair on splash, snapshot, ack, disconnect"),
        Prompt::new(
            "targets-walk",
            Some("Score all seven targets from snapshot expect"),
            None,
        )
        .with_title("Score all seven targets from snapshot expect"),
        Prompt::new(
            "after-failed-snapshot",
            Some("Clear a leftover arm and retry get-snapshot"),
            None,
        )
        .with_title("Clear a leftover arm and retry get-snapshot"),
    ]
}

fn resources() -> Vec<Resource> {
    vec![
        Resource::new("sticky-rs://remote-debug/pickup", "pickup")
            .with_title("Pickup for the next remote-debug sit")
            .with_description("Pickup for the next remote-debug sit")
            .with_mime_type("text/markdown"),
        Resource::new("sticky-rs://remote-debug/tools", "tools")
            .with_title("Leaf tools and structured fields")
            .with_description("Leaf tools and structured fields")
            .with_mime_type("text/markdown"),
        Resource::new("sticky-rs://remote-debug/snapshot", "snapshot")
            .with_title("Page-space PNG and expect coordinates")
            .with_description("Page-space PNG and expect coordinates")
            .with_mime_type("text/markdown"),
    ]
}

/// Serve stdio MCP until the client disconnects.
#[must_use]
pub fn serve() -> ExitCode {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(serve_async()),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn serve_async() -> ExitCode {
    let server = RemoteDebugMcp::new();
    match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => match running.waiting().await {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAVES: &[&str] = &[
        "connect",
        "status",
        "list-targets",
        "inject-touch",
        "inject-button",
        "get-snapshot",
        "snapshot-ack",
        "snapshot-clear",
        "reboot",
        "disconnect",
        "logs",
    ];

    #[test]
    fn tools_are_leaves_not_flash_or_serve() {
        let names: Vec<_> = RemoteDebugMcp::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        for leaf in LEAVES {
            assert!(names.iter().any(|n| n == leaf), "missing {leaf}: {names:?}");
        }
        assert_eq!(names.len(), LEAVES.len(), "names={names:?}");
        for forbidden in ["serve", "xtask", "remote-debug", "flash", "restore"] {
            assert!(
                !names.iter().any(|n| n.contains(forbidden)),
                "{forbidden} must not be an MCP tool: {names:?}"
            );
        }
    }

    #[test]
    fn output_schema_is_object_not_any_value() {
        let tools = RemoteDebugMcp::tool_router().list_all();
        let mut saw_leaf_schema = false;
        for tool in &tools {
            let Some(output) = tool.output_schema.as_ref() else {
                continue;
            };
            if LEAVES.iter().any(|leaf| tool.name.as_ref() == *leaf) {
                saw_leaf_schema = true;
            }
            assert_eq!(
                output.get("type").and_then(Value::as_str),
                Some("object"),
                "tool {} outputSchema={output:?}",
                tool.name
            );
            assert_ne!(
                output.get("title").and_then(Value::as_str),
                Some("AnyValue"),
                "tool {} still AnyValue: {output:?}",
                tool.name
            );
        }
        assert!(saw_leaf_schema, "leaf tools must advertise outputSchema");
    }

    #[test]
    fn initialize_advertises_prompts_and_resources() {
        let info = RemoteDebugMcp::new().get_info();
        assert!(info
            .instructions
            .as_deref()
            .is_some_and(|s| s.contains("inject-touch --page") && s.contains("Never a MAC")));
        let uris: Vec<_> = resources().into_iter().map(|r| r.uri).collect();
        assert!(uris.contains(&"sticky-rs://remote-debug/pickup".into()));
        assert!(uris.contains(&"sticky-rs://remote-debug/tools".into()));
        assert!(uris.contains(&"sticky-rs://remote-debug/snapshot".into()));
        let names: Vec<_> = prompts().into_iter().map(|p| p.name).collect();
        assert!(names.contains(&"desk-sit".into()));
        assert!(names.contains(&"targets-walk".into()));
        assert!(names.contains(&"after-failed-snapshot".into()));
    }
}
