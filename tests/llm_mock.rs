#![cfg(any(target_os = "linux", target_os = "macos"))]

use organizer::{
    llm::{
        ChatOptions, chat_with_provider,
        provider::{LlmError, LlmProvider, LlmRequest},
    },
    tree::{parse_normalized_tree, relative_paths},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::VecDeque, fs, path::Path, process::Command};

struct Mock {
    replies: RefCell<VecDeque<std::result::Result<Value, LlmError>>>,
    requests: RefCell<Vec<LlmRequest>>,
}
impl Mock {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: RefCell::new(replies.into_iter().map(Ok).collect()),
            requests: RefCell::new(vec![]),
        }
    }
}
impl LlmProvider for Mock {
    fn complete_json(&self, request: &LlmRequest) -> std::result::Result<Value, LlmError> {
        self.requests.borrow_mut().push(request.clone());
        self.replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected LLM call")
    }
}
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("Papers")).unwrap();
    fs::write(temp.path().join("Papers/CRUSH.txt"), "C to Rust research").unwrap();
    fs::write(temp.path().join("Papers/ASLR.txt"), "Security research").unwrap();
    organizer::sync::init(temp.path()).unwrap();
    temp
}
fn ids(root: &Path) -> std::collections::HashMap<String, String> {
    let tree =
        parse_normalized_tree(&fs::read_to_string(root.join(".organizer/structure.tree")).unwrap())
            .unwrap();
    relative_paths(&tree)
        .unwrap()
        .into_iter()
        .map(|(id, path)| (path, id))
        .collect()
}
fn editor(edits: Value, scope: Value) -> Value {
    json!({"schema_version":1,"edits":edits,"migration_intent": {"kind":"redistribute_existing_files","scope_dir_ids":scope}})
}
fn structure(edits: Value) -> Value {
    json!({"schema_version":1,"edits":edits,"migration_intent":{"kind":"structure_only","scope_dir_ids":[]}})
}
fn options() -> ChatOptions {
    ChatOptions {
        prompt: "按主题整理科研资料".into(),
        classify: true,
        ..ChatOptions::default()
    }
}

#[test]
fn directory_rename_never_calls_mapper_or_reads_body() {
    let temp = fixture();
    let root = temp.path();
    let ids = ids(root);
    let mock = Mock::new(vec![structure(
        json!([{"op":"rename_node","id":ids["Papers"],"name":"Research"}]),
    )]);
    let baseline = fs::read(root.join(".organizer/structure.tree")).unwrap();
    let database = fs::read(root.join(".organizer/state/index.sqlite")).unwrap();
    let report = chat_with_provider(
        root,
        &mock,
        &ChatOptions {
            prompt: "把 Papers 改名为 Research".into(),
            ..ChatOptions::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(mock.requests.borrow().len(), 1);
    assert!(
        !serde_json::to_string(&*mock.requests.borrow())
            .unwrap()
            .contains("C to Rust research")
    );
    assert_eq!(
        fs::read(root.join(".organizer/structure.tree")).unwrap(),
        baseline
    );
    assert_eq!(
        fs::read(root.join(".organizer/state/index.sqlite")).unwrap(),
        database
    );
    assert!(root.join("Papers/CRUSH.txt").exists());
    assert!(!root.join("Research").exists());
    assert!(report.unresolved.is_empty());
    let draft = parse_normalized_tree(&fs::read_to_string(report.draft).unwrap()).unwrap();
    assert_eq!(
        relative_paths(&draft).unwrap()[&ids["Papers/CRUSH.txt"]],
        "Research/CRUSH.txt"
    );
}

#[test]
fn mapper_place_keep_review_and_omissions_preserve_files() {
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("Papers/third.txt"), "three").unwrap();
    fs::write(root.join("Papers/fourth.txt"), "four").unwrap();
    organizer::sync::sync(root, false).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![
        editor(json!([]), json!([ids["Papers"]])),
        json!({"schema_version":1,"assignments":[
            {"file_id":ids["Papers/CRUSH.txt"],"decision":"place","target_dir_id":ids["Papers"],"reason":"same topic"},
            {"file_id":ids["Papers/ASLR.txt"],"decision":"keep","reason":"keep"},
            {"file_id":ids["Papers/third.txt"],"decision":"review","reason":"uncertain"}
        ]}),
    ]);
    let report = chat_with_provider(root, &mock, &options(), None).unwrap();
    assert_eq!(report.unresolved.len(), 2);
    let draft = parse_normalized_tree(&fs::read_to_string(&report.draft).unwrap()).unwrap();
    assert_eq!(draft.nodes.len(), 5);
    let output = Command::new(env!("CARGO_BIN_EXE_organizer"))
        .current_dir(root)
        .args([
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            report.draft.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("acknowledge-review"));
}

#[test]
fn unknown_duplicate_and_out_of_scope_assignments_fail_without_draft() {
    for variant in 0..3 {
        let temp = fixture();
        let root = temp.path();
        let ids = ids(root);
        let good = json!({"file_id":ids["Papers/CRUSH.txt"],"decision":"keep","reason":"keep"});
        let assignments = match variant {
            0 => json!([{"file_id":"invented","decision":"keep","reason":"wrong"}]),
            1 => json!([good, good]),
            _ => {
                json!([{"file_id":ids["Papers/CRUSH.txt"],"decision":"place","target_dir_id":"outside","reason":"wrong"}])
            }
        };
        let mock = Mock::new(vec![
            editor(json!([]), json!([ids["Papers"]])),
            json!({"schema_version":1,"assignments":assignments}),
        ]);
        assert!(chat_with_provider(root, &mock, &options(), None).is_err());
        assert_eq!(
            fs::read_dir(root.join(".organizer/drafts"))
                .unwrap()
                .count(),
            0
        );
    }
}

struct Classifier {
    requests: RefCell<Vec<LlmRequest>>,
    paper_id: String,
}
impl LlmProvider for Classifier {
    fn complete_json(&self, request: &LlmRequest) -> std::result::Result<Value, LlmError> {
        self.requests.borrow_mut().push(request.clone());
        if request.task == "tree_editor" {
            return Ok(editor(
                json!([
                    {"op":"add_directory","parent_id":self.paper_id,"name":"C2Rust","description":"Rust migration"},
                    {"op":"add_directory","parent_id":self.paper_id,"name":"Security","description":"Security"}
                ]),
                json!([self.paper_id]),
            ));
        }
        let directories = request.input["data"]["candidate_directories"]
            .as_array()
            .unwrap();
        let assignments: Vec<_> = request.input["data"]["files"].as_array().unwrap().iter().map(|file| {
            let target = if file["path"].as_str().unwrap().contains("CRUSH") { "C2Rust" } else { "Security" };
            let directory = directories.iter().find(|dir| dir["path"].as_str().unwrap().ends_with(target)).unwrap();
            json!({"file_id":file["id"],"decision":"place","target_dir_id":directory["id"],"reason":"classified from supplied metadata"})
        }).collect();
        Ok(json!({"schema_version":1,"assignments":assignments}))
    }
}

#[test]
fn mock_chat_to_plan_dry_run_apply_end_to_end_preserves_ids_and_content() {
    let temp = fixture();
    let root = temp.path();
    let original = ids(root);
    let provider = Classifier {
        requests: RefCell::new(vec![]),
        paper_id: original["Papers"].clone(),
    };
    let report = chat_with_provider(root, &provider, &options(), None).unwrap();
    assert!(report.unresolved.is_empty());
    assert_eq!(provider.requests.borrow().len(), 2);
    assert_eq!(report.context_bytes_read, 0);
    let plan = root.join(".organizer/plans/llm.json");
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_organizer"))
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run(&[
        "plan",
        "--from",
        ".organizer/structure.tree",
        "--to",
        report.draft.to_str().unwrap(),
        "--out",
        plan.to_str().unwrap(),
    ]);
    let operations: Value = serde_json::from_slice(&fs::read(&plan).unwrap()).unwrap();
    assert_eq!(operations["operations"].as_array().unwrap().len(), 4);
    run(&["apply", plan.to_str().unwrap(), "--dry-run"]);
    assert!(root.join("Papers/CRUSH.txt").exists());
    run(&["apply", plan.to_str().unwrap(), "--yes"]);
    let current = ids(root);
    assert_eq!(
        current["Papers/C2Rust/CRUSH.txt"],
        original["Papers/CRUSH.txt"]
    );
    assert_eq!(
        current["Papers/Security/ASLR.txt"],
        original["Papers/ASLR.txt"]
    );
    assert_eq!(
        fs::read_to_string(root.join("Papers/C2Rust/CRUSH.txt")).unwrap(),
        "C to Rust research"
    );
    let git = Command::new("git")
        .arg("-C")
        .arg(root.join(".organizer"))
        .args(["ls-files"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(git.stdout).unwrap().trim(),
        "structure.tree"
    );
}

#[test]
fn malformed_json_or_schema_retries_once_but_transport_and_semantic_errors_do_not() {
    for schema_error in [false, true] {
        let temp = fixture();
        let root = temp.path();
        let mock = Mock::new(vec![structure(json!([]))]);
        mock.replies.borrow_mut().push_front(if schema_error {
            Ok(json!({"schema_version":1,"extra":"forbidden"}))
        } else {
            Err(LlmError::InvalidJson)
        });
        chat_with_provider(
            root,
            &mock,
            &ChatOptions {
                prompt: "keep".into(),
                ..ChatOptions::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(mock.requests.borrow().len(), 2);
        assert!(mock.requests.borrow()[1].repair);
    }
    let temp = fixture();
    let root = temp.path();
    let mock = Mock::new(vec![]);
    mock.replies
        .borrow_mut()
        .push_back(Err(LlmError::Http(401)));
    assert!(chat_with_provider(root, &mock, &options(), None).is_err());
    assert_eq!(mock.requests.borrow().len(), 1);
    let mock = Mock::new(vec![structure(
        json!([{"op":"rename_node","id":"invented","name":"X"}]),
    )]);
    assert!(chat_with_provider(root, &mock, &options(), None).is_err());
    assert_eq!(mock.requests.borrow().len(), 1);
    let mock = Mock::new(vec![json!({}), json!({})]);
    assert!(chat_with_provider(root, &mock, &options(), None).is_err());
    assert_eq!(mock.requests.borrow().len(), 2);
    assert_eq!(
        fs::read_dir(root.join(".organizer/drafts"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn forbidden_edits_file_moves_nonempty_removal_and_classification_escalation_are_rejected() {
    for scenario in 0..7 {
        let temp = fixture();
        let root = temp.path();
        let ids = ids(root);
        let response = match scenario {
            0 => structure(json!([{"op":"delete_file","id":ids["Papers/CRUSH.txt"]}])),
            1 => {
                structure(json!([{"op":"move_node","id":ids["Papers/CRUSH.txt"],"parent_id":null}]))
            }
            2 => structure(json!([{"op":"remove_empty_directory_declaration","id":ids["Papers"]}])),
            3 => structure(json!([{"op":"rename_node","id":ids["Papers"],"name":"../escape"}])),
            4 => structure(json!([{"op":"rename_node","id":ids["Papers"],"name":".git"}])),
            5 => editor(json!([]), json!([ids["Papers"]])),
            _ => {
                structure(json!([{"op":"move_node","id":ids["Papers"],"parent_id":ids["Papers"]}]))
            }
        };
        let mock = Mock::new(vec![response.clone(), response]);
        assert!(
            chat_with_provider(
                root,
                &mock,
                &ChatOptions {
                    prompt: "organize".into(),
                    ..ChatOptions::default()
                },
                None
            )
            .is_err()
        );
        assert!(root.join("Papers/CRUSH.txt").exists());
        assert_eq!(
            fs::read_dir(root.join(".organizer/drafts"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn new_directory_references_are_tool_assigned_and_nonempty_declarations_cannot_disappear() {
    let temp = fixture();
    let root = temp.path();
    let ids = ids(root);
    let mock = Mock::new(vec![structure(json!([
        {"op":"add_directory","parent_id":ids["Papers"],"name":"New","local_ref":"$new"},
        {"op":"add_directory","parent_id":"$new","name":"Nested","local_ref":"$child"},
        {"op":"set_description","id":"$child","description":"new nested folder"},
        {"op":"rename_node","id":"$new","name":"Organized"}
    ]))]);
    let report = chat_with_provider(
        root,
        &mock,
        &ChatOptions {
            prompt: "create directories".into(),
            ..ChatOptions::default()
        },
        None,
    )
    .unwrap();
    let tree = parse_normalized_tree(&fs::read_to_string(report.draft).unwrap()).unwrap();
    assert!(tree.nodes.iter().all(|node| !node.id.starts_with('$')));
    assert!(
        relative_paths(&tree)
            .unwrap()
            .values()
            .any(|path| path == "Papers/Organized/Nested")
    );
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|node| node.kind == organizer::tree::NodeKind::File)
            .count(),
        2
    );
}

#[test]
fn scope_excludes_unrelated_nodes_and_ancestors_are_read_only() {
    let temp = fixture();
    let root = temp.path();
    fs::create_dir_all(root.join("Elsewhere/Nested")).unwrap();
    fs::write(root.join("Elsewhere/private.txt"), "not requested").unwrap();
    organizer::sync::sync(root, false).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![structure(json!([]))]);
    let opts = ChatOptions {
        prompt: "keep papers".into(),
        scope_ids: vec![ids["Papers"].clone()],
        ..ChatOptions::default()
    };
    chat_with_provider(root, &mock, &opts, None).unwrap();
    assert!(
        !serde_json::to_string(&*mock.requests.borrow())
            .unwrap()
            .contains("Elsewhere")
    );
    let mock = Mock::new(vec![structure(
        json!([{"op":"rename_node","id":ids["Elsewhere"],"name":"Changed"}]),
    )]);
    let opts = ChatOptions {
        prompt: "attempt outside scope".into(),
        scope_ids: vec![ids["Elsewhere/Nested"].clone()],
        ..ChatOptions::default()
    };
    assert!(chat_with_provider(root, &mock, &opts, None).is_err());
    let requests = mock.requests.borrow();
    assert!(
        requests[0].input["data"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == ids["Elsewhere"] && node["editable"] == false)
    );
}

#[test]
fn default_context_skips_sensitive_and_protected_candidates_and_never_reads_bodies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("Papers")).unwrap();
    fs::write(root.join("Papers/plain.txt"), "BODY_MARKER").unwrap();
    for name in [".env", "id_rsa", "key.pem"] {
        fs::write(root.join("Papers").join(name), "SECRET_BODY").unwrap();
    }
    fs::create_dir_all(root.join("Papers/project/.git")).unwrap();
    fs::write(root.join("Papers/project/hidden.txt"), "GIT_BODY").unwrap();
    organizer::sync::init(root).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![
        editor(json!([]), json!([ids["Papers"]])),
        json!({"schema_version":1,"assignments":[]}),
    ]);
    let report = chat_with_provider(root, &mock, &options(), None).unwrap();
    let requests = serde_json::to_string(&*mock.requests.borrow()).unwrap();
    for text in [
        ".env",
        "id_rsa",
        "key.pem",
        "hidden.txt",
        "project",
        "BODY_MARKER",
        "SECRET_BODY",
        "GIT_BODY",
    ] {
        assert!(!requests.contains(text), "leaked {text}");
    }
    assert_eq!(report.context_bytes_read, 0);
    let draft = parse_normalized_tree(&fs::read_to_string(report.draft).unwrap()).unwrap();
    assert_eq!(draft.nodes.len(), 6);
}

#[test]
fn excerpts_are_bounded_and_binary_pdf_keys_are_not_sent() {
    let temp = fixture();
    let root = temp.path();
    for index in 0..20 {
        fs::write(
            root.join(format!("Papers/large{index:02}.txt")),
            "x".repeat(10_000),
        )
        .unwrap();
    }
    fs::write(root.join("Papers/0binary.txt"), b"\x00BINARY_SECRET").unwrap();
    fs::write(
        root.join("Papers/0disguised.txt"),
        "-----BEGIN PRIVATE KEY-----\nKEY_SECRET",
    )
    .unwrap();
    fs::write(root.join("Papers/0paper.pdf"), "PDF_SECRET").unwrap();
    organizer::sync::sync(root, false).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![
        editor(json!([]), json!([ids["Papers"]])),
        json!({"schema_version":1,"assignments":[]}),
    ]);
    let report = chat_with_provider(
        root,
        &mock,
        &ChatOptions {
            read_content: true,
            ..options()
        },
        None,
    )
    .unwrap();
    assert!(report.context_bytes_read > 4096 && report.context_bytes_read <= 65536);
    let requests = mock.requests.borrow();
    let encoded = serde_json::to_string(&*requests).unwrap();
    for secret in ["BINARY_SECRET", "KEY_SECRET", "PDF_SECRET"] {
        assert!(!encoded.contains(secret));
    }
    let files = requests[1].input["data"]["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .all(|f| f["context"].as_str().is_none_or(|text| text.len() <= 4096))
    );
    assert!(
        files
            .iter()
            .filter_map(|f| f["context"].as_str())
            .map(str::len)
            .sum::<usize>()
            <= 65536
    );
}

struct Mutating {
    root: std::path::PathBuf,
    response: Value,
}
impl LlmProvider for Mutating {
    fn complete_json(&self, _: &LlmRequest) -> std::result::Result<Value, LlmError> {
        fs::write(self.root.join("Papers/CRUSH.txt"), "changed while waiting").unwrap();
        Ok(self.response.clone())
    }
}
#[test]
fn changes_during_provider_call_prevent_publishing_stale_draft() {
    let temp = fixture();
    let root = temp.path();
    let mock = Mutating {
        root: root.into(),
        response: structure(json!([])),
    };
    assert!(
        chat_with_provider(
            root,
            &mock,
            &ChatOptions {
                prompt: "keep".into(),
                ..ChatOptions::default()
            },
            None
        )
        .is_err()
    );
    assert_eq!(
        fs::read_dir(root.join(".organizer/drafts"))
            .unwrap()
            .count(),
        0
    );
}

struct ReplacingContext<'a> {
    root: &'a Path,
    outside: &'a Path,
    variant: u8,
    response: Value,
}
impl LlmProvider for ReplacingContext<'_> {
    fn complete_json(&self, request: &LlmRequest) -> std::result::Result<Value, LlmError> {
        assert_eq!(request.task, "tree_editor", "unsafe context reached mapper");
        let source = self.root.join("Papers/CRUSH.txt");
        match self.variant {
            0 => {
                fs::remove_file(&source).unwrap();
                std::os::unix::fs::symlink(self.outside.join("CRUSH.txt"), source).unwrap();
            }
            1 => {
                fs::rename(self.root.join("Papers"), self.root.join("Original")).unwrap();
                std::os::unix::fs::symlink(self.outside, self.root.join("Papers")).unwrap();
            }
            _ => {
                let replacement = self.root.join("replacement.txt");
                fs::write(&replacement, "C to Rust research").unwrap();
                fs::rename(replacement, source).unwrap();
            }
        }
        Ok(self.response.clone())
    }
}

#[test]
fn replaced_context_file_or_symlink_ancestor_is_rejected_before_mapper() {
    for variant in 0..3 {
        let temp = fixture();
        let root = temp.path();
        let outside = tempfile::tempdir().unwrap();
        for name in ["CRUSH.txt", "ASLR.txt"] {
            fs::write(outside.path().join(name), "OUTSIDE_SECRET").unwrap();
        }
        let ids = ids(root);
        let provider = ReplacingContext {
            root,
            outside: outside.path(),
            variant,
            response: editor(json!([]), json!([ids["Papers"]])),
        };
        assert!(
            chat_with_provider(
                root,
                &provider,
                &ChatOptions {
                    read_content: true,
                    ..options()
                },
                None,
            )
            .is_err()
        );
        assert_eq!(
            fs::read_dir(root.join(".organizer/drafts"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("CRUSH.txt")).unwrap(),
            "OUTSIDE_SECRET"
        );
    }
}

#[test]
fn mapper_collision_never_publishes_or_overwrites_files() {
    let temp = fixture();
    let root = temp.path();
    fs::create_dir(root.join("Papers/Archive")).unwrap();
    fs::write(root.join("Papers/Archive/ASLR.txt"), "existing target").unwrap();
    organizer::sync::sync(root, false).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![
        editor(json!([]), json!([ids["Papers"]])),
        json!({"schema_version":1,"assignments":[
            {"file_id":ids["Papers/ASLR.txt"],"decision":"place","target_dir_id":ids["Papers/Archive"],"reason":"collision"}
        ]}),
    ]);
    assert!(chat_with_provider(root, &mock, &options(), None).is_err());
    assert_eq!(
        fs::read_dir(root.join(".organizer/drafts"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_to_string(root.join("Papers/ASLR.txt")).unwrap(),
        "Security research"
    );
    assert_eq!(
        fs::read_to_string(root.join("Papers/Archive/ASLR.txt")).unwrap(),
        "existing target"
    );
}

#[test]
fn removing_empty_declaration_still_requires_m2_resolution() {
    let temp = fixture();
    let root = temp.path();
    fs::create_dir(root.join("Empty")).unwrap();
    organizer::sync::sync(root, false).unwrap();
    let ids = ids(root);
    let mock = Mock::new(vec![structure(json!([
        {"op":"remove_empty_directory_declaration","id":ids["Empty"]}
    ]))]);
    let report = chat_with_provider(root, &mock, &options(), None).unwrap();
    assert!(!report.unresolved.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_organizer"))
        .current_dir(root)
        .args([
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            report.draft.to_str().unwrap(),
            "--acknowledge-review",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(plan["operations"].as_array().unwrap().is_empty());
    assert!(!plan["unresolved"].as_array().unwrap().is_empty());
    assert!(root.join("Empty").is_dir());
}

#[test]
fn editing_generated_draft_requires_review_acknowledgement() {
    let temp = fixture();
    let root = temp.path();
    let mock = Mock::new(vec![structure(json!([]))]);
    let report = chat_with_provider(root, &mock, &options(), None).unwrap();
    assert!(report.unresolved.is_empty());
    let source = fs::read_to_string(&report.draft)
        .unwrap()
        .replace("Papers/", "Research/");
    fs::write(&report.draft, source).unwrap();
    for acknowledged in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_organizer"));
        command.current_dir(root).args([
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            report.draft.to_str().unwrap(),
        ]);
        if acknowledged {
            command.arg("--acknowledge-review");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.success(),
            acknowledged,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(root.join("Papers/CRUSH.txt").is_file());
}

#[test]
fn output_collisions_and_unsafe_paths_fail_before_llm_calls() {
    let temp = fixture();
    let root = temp.path();
    let mock = Mock::new(vec![]);
    let existing = root.join(".organizer/drafts/existing.tree");
    fs::write(&existing, "retain").unwrap();
    for path in [
        existing,
        root.join("file.tree"),
        root.join(".organizer/drafts/../structure.tree"),
    ] {
        assert!(chat_with_provider(root, &mock, &options(), Some(&path)).is_err());
    }
    assert!(mock.requests.borrow().is_empty());
}

#[test]
fn review_marker_cannot_be_silently_lost_and_acknowledgement_keeps_m2_validation() {
    let temp = fixture();
    let root = temp.path();
    let ids = ids(root);
    let mock = Mock::new(vec![
        editor(json!([]), json!([ids["Papers"]])),
        json!({"schema_version":1,"assignments":[]}),
    ]);
    let report = chat_with_provider(root, &mock, &options(), None).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_organizer"))
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        !run(&["normalize", report.draft.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        run(&[
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            report.draft.to_str().unwrap(),
            "--acknowledge-review"
        ])
        .status
        .success()
    );
    fs::remove_file(report.review).unwrap();
    assert!(
        !run(&[
            "plan",
            "--from",
            ".organizer/structure.tree",
            "--to",
            report.draft.to_str().unwrap(),
            "--acknowledge-review"
        ])
        .status
        .success()
    );
}

#[test]
fn missing_configuration_and_privacy_switches_leave_manual_commands_usable() {
    let temp = fixture();
    let root = temp.path();
    let run = |args: &[&str], vars: &[(&str, &str)]| {
        Command::new(env!("CARGO_BIN_EXE_organizer"))
            .current_dir(root)
            .args(args)
            .env_remove("ORGANIZER_LLM_MODEL")
            .env_remove("ORGANIZER_LLM_API_KEY")
            .env_remove("ORGANIZER_LLM_BASE_URL")
            .envs(vars.iter().copied())
            .output()
            .unwrap()
    };
    let missing = run(&["chat", "organize"], &[]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("manually"));
    let disabled = run(&["chat", "organize"], &[("ORGANIZER_LLM_DISABLED", "1")]);
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("disabled"));
    let local = run(
        &["chat", "organize"],
        &[
            ("ORGANIZER_LLM_MODEL", "test"),
            ("ORGANIZER_LLM_API_KEY", "secret"),
            ("ORGANIZER_LLM_LOCAL_ONLY", "1"),
        ],
    );
    assert!(!local.status.success());
    assert!(!String::from_utf8_lossy(&local.stderr).contains("secret"));
    let content = run(
        &["chat", "organize", "--classify", "--read-content"],
        &[
            ("ORGANIZER_LLM_MODEL", "test"),
            ("ORGANIZER_LLM_BASE_URL", "http://127.0.0.1:1/v1"),
            ("ORGANIZER_LLM_NO_CONTENT", "1"),
        ],
    );
    assert!(!content.status.success());
    assert!(String::from_utf8_lossy(&content.stderr).contains("prohibits --read-content"));
    assert!(
        run(&["status"], &[("ORGANIZER_LLM_DISABLED", "1")])
            .status
            .success()
    );
    assert_eq!(
        fs::read_dir(root.join(".organizer/drafts"))
            .unwrap()
            .count(),
        0
    );
}
