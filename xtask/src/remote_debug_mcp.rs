//! Server instructions, prompts, and resources for `remote-debug --mcp`.
//!
//! clap-mcp 0.1.0 advertises these on initialize / `prompts/list` /
//! `resources/list`. Tool argv still comes from clap leaves. Never a MAC.

use clap_mcp::content::{CustomPrompt, CustomResource, PromptContent, ResourceContent};
use clap_mcp::{ClapMcpServeOptions, Implementation};
use rmcp::model::{PromptMessage, Role};

/// Initialize / discover instructions for a desk sit.
pub const INSTRUCTIONS: &str = "\
You are a client of the sticky-rs remote-debug broker (encrypted GATT after \
DisplayOnly pair). This process does not own GATT. Tools are clap leaves: \
connect, status, inject-touch, inject-button, get-snapshot, snapshot-ack, \
snapshot-clear, reboot, disconnect. Not remote-debug_*.

Stay on splash for pair (Ferris never shows the PIN). Do not ask the operator \
to pair from a phone. Do not run monitor during auto-PIN. No --remember unless \
the human asked. Never a MAC, serial, PIN, or snapshot planes in git.

Loop: connect (returns pairing) → poll status until connected or pair failed \
→ use → disconnect. After a fresh flash, retry connect on \
le-connection-abort-by-local or CDC busy.

inject-touch --page uses page pixels for the last compose hold (gray4 \
hit-test inverse). --phase down/move/up (unset = tap). Do not use UART p0=. \
Wait ~2–3 s after inject for compose. Do not tap Wi-Fi START unless asked.

get-snapshot writes .bw/.red plus a page-space .png (portrait 480×800 or \
landscape 800×480). Tap --page at snapshot expect, not a guessed landscape \
centre. A leftover arm is SnapshotBusy; snapshot-clear then retry. Ack when \
done.

Read sticky-rs://remote-debug/pickup before the next sit. Prompts: desk-sit, \
targets-walk, after-failed-snapshot.";

/// Pickup card (same facts as the skill page, no client names).
const PICKUP: &str = "\
# Pickup

Stay on splash. Do not ask the operator to pair from a phone.
No monitor during auto-PIN. No --remember unless asked. Never a MAC.

1. status — no broker until connect.
2. connect → pairing. Poll status until connected (or pair failed).
3. inject-button page-down / page-up, or inject-touch --page at expect.
   --phase down / move / up (unset = tap). Wait ~2–3 s. Do not tap START.
4. get-snapshot — LAST DRAW plus scene / hold / target_step / expect.
   PNG is page space (same origin as --page). Ack when done.
   SnapshotBusy → snapshot-clear, then retry.
5. Targets: seven page-downs to scene=7. Tap --page at expect. Slides use
   --phase. After id 6, target_step=0. last_log may be target show id=0.
6. disconnect when the sit is over.
";

const TOOLS: &str = "\
# Tools

| Tool | How to use it |
| --- | --- |
| connect | Starts the broker if needed. Returns pairing. BlueZ Connect, not Pair(). UART auto-PIN unless pin. |
| status | pairing / connected / disconnected / pair failed. last_log is Target / Scene only. |
| inject-touch | Default x/y are framebuffer. page=true treats x/y as page pixels. phase down/move/up; unset is a tap. Not UART p0=. |
| inject-button | key ok / page-up / page-down. down true is a short press. Wait for compose. |
| get-snapshot | Arms LAST DRAW. Writes snap-<hex>.bw/.red/.png. scene / hold / step / expect on the line. |
| snapshot-ack | Release the armed nonce. |
| snapshot-clear | Abort with no nonce. Use after a failed get. |
| reboot | Software-reset the embedded MCU (not this host). Re-pairs unless no_reconnect. |
| disconnect | Drop GATT and stop the broker. Unknown units lose the BlueZ bond. |

Structured fields: ok, message, nonce, connected, phase, last_log, scene, \
hold, target_step, target_expect_x, target_expect_y. message must not include \
a MAC.
";

const SNAPSHOT: &str = "\
# Snapshot PNG

.bw / .red stay pre-rotation 800×480 controller planes.

.png is rematerialized in **page** space for Snapshot.hold:

- hold 0 Portrait0 / 1 Portrait180 → 480×800
- hold 2 Landscape0 / 3 Landscape180 → 800×480

Page (0,0) is the top-left of the card as composed (same origin as \
inject-touch --page and target expect). Gray4 planes undo the Seeed OTP \
180° write so text is upright. Do not guess a landscape centre from the \
raw 800×480 `.bw` dump.

After get-snapshot, ack. A second get while armed is SnapshotBusy.
";

/// Serve options: instructions, prompts, resources, identity.
#[must_use]
pub fn serve_options() -> ClapMcpServeOptions {
    let mut opts = ClapMcpServeOptions {
        instructions: Some(INSTRUCTIONS.into()),
        server_info: Some(
            Implementation::new("sticky-rs-remote-debug", env!("CARGO_PKG_VERSION"))
                .with_title("sticky-rs remote-debug")
                .with_website_url("https://github.com/canardleteer/sticky-rs"),
        ),
        ..ClapMcpServeOptions::default()
    };
    opts.custom_resources.extend(resources());
    opts.custom_prompts.extend(prompts());
    opts
}

fn resources() -> Vec<CustomResource> {
    vec![
        text_resource(
            "sticky-rs://remote-debug/pickup",
            "pickup",
            "Pickup for the next remote-debug sit",
            PICKUP,
        ),
        text_resource(
            "sticky-rs://remote-debug/tools",
            "tools",
            "Leaf tools and structured fields",
            TOOLS,
        ),
        text_resource(
            "sticky-rs://remote-debug/snapshot",
            "snapshot",
            "Page-space PNG and expect coordinates",
            SNAPSHOT,
        ),
    ]
}

fn text_resource(uri: &str, name: &str, title: &str, body: &str) -> CustomResource {
    CustomResource {
        uri: uri.into(),
        name: name.into(),
        title: Some(title.into()),
        description: Some(title.into()),
        mime_type: Some("text/markdown".into()),
        content: ResourceContent::Static(body.into()),
    }
}

fn prompts() -> Vec<CustomPrompt> {
    vec![
        user_prompt(
            "desk-sit",
            "Pair on splash, snapshot, ack, disconnect",
            DESK_SIT,
        ),
        user_prompt(
            "targets-walk",
            "Score all seven targets from snapshot expect",
            TARGETS_WALK,
        ),
        user_prompt(
            "after-failed-snapshot",
            "Clear a leftover arm and retry get-snapshot",
            AFTER_FAILED,
        ),
    ]
}

fn user_prompt(name: &str, title: &str, text: &str) -> CustomPrompt {
    CustomPrompt {
        name: name.into(),
        title: Some(title.into()),
        description: Some(title.into()),
        arguments: Vec::new(),
        content: PromptContent::Static(vec![PromptMessage::new_text(Role::User, text)]),
    }
}

const DESK_SIT: &str = "\
Run a sticky-rs remote-debug desk sit.

Stay on splash. Do not ask anyone to pair from a phone. No monitor. \
No --remember. Never a MAC.

1. Read resource sticky-rs://remote-debug/pickup.
2. status. If no broker, connect (expect pairing).
3. Poll status until connected. Retry connect after flash if the first \
sit is le-connection-abort-by-local or CDC busy.
4. get-snapshot. Confirm Ferris only (no six-digit boxes). PNG is page \
space. Ack.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_options_advertise_prompts_and_resources() {
        let opts = serve_options();
        assert!(opts
            .instructions
            .as_deref()
            .is_some_and(|s| { s.contains("inject-touch --page") && s.contains("Never a MAC") }));
        let uris: Vec<_> = opts
            .custom_resources
            .iter()
            .map(|r| r.uri.as_str())
            .collect();
        assert!(uris.contains(&"sticky-rs://remote-debug/pickup"));
        assert!(uris.contains(&"sticky-rs://remote-debug/tools"));
        assert!(uris.contains(&"sticky-rs://remote-debug/snapshot"));
        let names: Vec<_> = opts
            .custom_prompts
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert!(names.contains(&"desk-sit"));
        assert!(names.contains(&"targets-walk"));
        assert!(names.contains(&"after-failed-snapshot"));
    }
}
