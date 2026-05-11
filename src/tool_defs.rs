//! Static [`ToolDef`] catalogue advertised in the
//! `initialize` reply.
//!
//! Each entry is hand-rolled. Adding a tool requires editing this
//! file and `dispatch.rs` in lockstep — the test asserts the
//! count matches.

use nexo_microapp_sdk::plugin::ToolDef;
use serde_json::json;

pub fn browser_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "browser_click".into(),
            description: "Click a DOM element. `target` is either an element \
                reference emitted by `browser_snapshot` (e.g. `@e12`) or a \
                CSS selector (e.g. `button[type=submit]`). Prefer element \
                refs — they're stable across DOM mutations within a single \
                snapshot turn.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Element ref (`@eN`) or CSS selector."
                    }
                },
                "required": ["target"]
            }),
        },
        ToolDef {
            name: "browser_current_url".into(),
            description: "Return the URL of the active page.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "browser_evaluate".into(),
            description: "Execute JavaScript in the page and return the \
                result as JSON. Use for reading DOM properties, computed \
                styles, or computing derived values without a custom tool.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "script": {
                        "type": "string",
                        "description": "JS expression evaluated in page context."
                    }
                },
                "required": ["script"]
            }),
        },
        ToolDef {
            name: "browser_fill".into(),
            description: "Type a value into an input / textarea / \
                contenteditable element. `target` follows the same rules \
                as `browser_click`. `value` replaces the element's current \
                contents — there's no append mode. For multi-step forms, \
                call once per field.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Element ref (`@eN`) or CSS selector for the input."
                    },
                    "value": {
                        "type": "string",
                        "description": "Text to type. Special keys (Enter, Tab) are NOT interpreted — use `browser_press_key` for keyboard shortcuts."
                    }
                },
                "required": ["target", "value"]
            }),
        },
        ToolDef {
            name: "browser_go_back".into(),
            description: "Navigate one step back in the browser history \
                (equivalent to clicking the back button). Returns `{ok}`.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "browser_go_forward".into(),
            description: "Navigate one step forward in the browser history. \
                Only succeeds if the user (or a prior tool call) just went \
                back. Returns `{ok}`.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "browser_navigate".into(),
            description: "Navigate the active page to a URL. Waits for the \
                main document `load` event before returning.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Absolute URL including scheme."
                    }
                },
                "required": ["url"]
            }),
        },
        ToolDef {
            name: "browser_press_key".into(),
            description: "Synthesize a keyboard event on the active \
                element. `key` is either a known named key (Enter / Tab / \
                Escape / ArrowUp / ArrowDown / ArrowLeft / ArrowRight / \
                Backspace / Delete / Home / End / PageUp / PageDown / \
                Space) or a single character.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Named key or single character."
                    }
                },
                "required": ["key"]
            }),
        },
        ToolDef {
            name: "browser_screenshot".into(),
            description: "Capture a PNG screenshot of the viewport. \
                Returns `{ok: true, png_base64: \"...\"}` so the LLM can \
                attach the image to its reply.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "browser_scroll_to".into(),
            description: "Scroll a target element into view. `target` \
                follows the same rules as `browser_click`.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Element ref (`@eN`) or CSS selector."
                    }
                },
                "required": ["target"]
            }),
        },
        ToolDef {
            name: "browser_snapshot".into(),
            description: "Capture a textual DOM tree where every \
                actionable element has a stable ref like `@e12`. Refs \
                are valid until the next DOM mutation. Use this before \
                `browser_click` / `browser_fill` so the LLM can pick \
                targets without inventing CSS selectors.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "browser_wait_for".into(),
            description: "Poll a CSS selector every 250 ms until it \
                appears in the DOM, up to `timeout_ms` (default 5000). \
                Returns `{ok: true, found: bool}`. Use before \
                `browser_click` on elements that arrive after an XHR / \
                SPA navigation.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector the element must match."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Max wait in milliseconds. Default 5000, capped at 30000."
                    }
                },
                "required": ["selector"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_tool_defs_returns_twelve_entries() {
        assert_eq!(browser_tool_defs().len(), 12);
    }

    #[test]
    fn browser_tool_defs_names_are_canonical_and_sorted() {
        let defs = browser_tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        for n in &names {
            assert!(n.starts_with("browser_"), "tool {n} violates namespace policy");
        }
        assert_eq!(
            names,
            [
                "browser_click",
                "browser_current_url",
                "browser_evaluate",
                "browser_fill",
                "browser_go_back",
                "browser_go_forward",
                "browser_navigate",
                "browser_press_key",
                "browser_screenshot",
                "browser_scroll_to",
                "browser_snapshot",
                "browser_wait_for",
            ]
        );
    }

    #[test]
    fn browser_tool_defs_all_have_object_schema() {
        for def in browser_tool_defs() {
            assert_eq!(def.input_schema["type"], "object",
                "tool {} input_schema is not an object", def.name);
        }
    }
}
