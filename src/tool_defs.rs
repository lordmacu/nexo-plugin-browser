//! Build the SDK [`ToolDef`] list advertised in the
//! `initialize` reply.
//!
//! Each entry is derived from the corresponding in-tree
//! `Browser*Tool::tool_def()` output (which returns
//! `nexo_llm::ToolDef { name, description, parameters }`),
//! converted into the wire-shape SDK [`ToolDef`]
//! (`name`, `description`, `input_schema`).
//!
//! Adding a tool requires updating two things in lockstep:
//!   1. The `Browser*Tool` impl in `tool.rs`.
//!   2. This list — the `extends.tools` allowlist in the manifest
//!      ALSO needs the new name (the host kills the subprocess
//!      otherwise).
//! The unit test below asserts the count + canonical names.

use nexo_microapp_sdk::plugin::ToolDef;

use crate::tool::{
    BrowserClickTool, BrowserCurrentUrlTool, BrowserEvaluateTool, BrowserFillTool,
    BrowserGoBackTool, BrowserGoForwardTool, BrowserNavigateTool, BrowserPressKeyTool,
    BrowserScreenshotTool, BrowserScrollToTool, BrowserSnapshotTool, BrowserWaitForTool,
};

/// 12 entries; sorted alphabetically for diff stability.
pub fn browser_tool_defs() -> Vec<ToolDef> {
    vec![
        from_llm(&BrowserClickTool::tool_def()),
        from_llm(&BrowserCurrentUrlTool::tool_def()),
        from_llm(&BrowserEvaluateTool::tool_def()),
        from_llm(&BrowserFillTool::tool_def()),
        from_llm(&BrowserGoBackTool::tool_def()),
        from_llm(&BrowserGoForwardTool::tool_def()),
        from_llm(&BrowserNavigateTool::tool_def()),
        from_llm(&BrowserPressKeyTool::tool_def()),
        from_llm(&BrowserScreenshotTool::tool_def()),
        from_llm(&BrowserScrollToTool::tool_def()),
        from_llm(&BrowserSnapshotTool::tool_def()),
        from_llm(&BrowserWaitForTool::tool_def()),
    ]
}

fn from_llm(def: &nexo_llm::ToolDef) -> ToolDef {
    ToolDef {
        name: def.name.clone(),
        description: def.description.clone(),
        // `nexo_llm::ToolDef.parameters` is the JSON Schema —
        // rename to `input_schema` for the wire shape.
        input_schema: def.parameters.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_tool_defs_returns_twelve_entries() {
        let defs = browser_tool_defs();
        assert_eq!(defs.len(), 12);
    }

    #[test]
    fn browser_tool_defs_names_are_canonical_and_sorted() {
        let defs = browser_tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        // Sorted alphabetically — guards against a refactor
        // accidentally reordering and breaking diffs.
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        // Sentinel — confirms 81.3 namespace policy
        // (`browser_*` prefix per plugin id).
        for n in &names {
            assert!(n.starts_with("browser_"), "tool {n} violates namespace policy");
        }
        // Exact list — guards against accidental drop / rename.
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
            assert_eq!(
                def.input_schema["type"], "object",
                "tool {} input_schema is not an object", def.name
            );
        }
    }
}
