use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_PRESENTATION_REFS: &[&str] = &[
    "agent_client_protocol::schema::v1",
    "agent-client-protocol",
    "ToolKind",
    "ToolCallContent",
    "ToolCallLocation",
    "ToolCallStatus",
    "ToolCallUpdate",
    "ToolCall",
    "Diff",
    "Terminal",
];

fn rust_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", root.display()));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

#[test]
fn tool_ux_has_no_acp_presentation_dependencies() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tools/tool_ux");
    let mut files = Vec::new();
    rust_files(&root, &mut files);

    let mut violations = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        for forbidden in FORBIDDEN_PRESENTATION_REFS {
            if source.contains(forbidden) {
                violations.push(format!("{} contains {forbidden}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "tool_ux must remain host-neutral; forbidden presentation references found:\n{}",
        violations.join("\n")
    );
}
