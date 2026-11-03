use super::*;
use crate::error::TreeError;

fn directory(id: &str, parent: Option<&str>, name: &str) -> TreeNode {
    TreeNode {
        id: id.into(),
        parent_id: parent.map(str::to_owned),
        name: name.into(),
        kind: NodeKind::Directory,
        description: None,
    }
}

#[test]
fn validator_rejects_bad_ids_names_and_descriptions() {
    let valid = directory("d", None, "safe");
    for id in ["", "bad.id", "空", "two ids", "@id"] {
        let mut node = valid.clone();
        node.id = id.into();
        assert!(
            validate_tree(&TreeSpec {
                version: 1,
                nodes: vec![node]
            })
            .is_err(),
            "{id:?}"
        );
    }
    for name in ["", ".", "..", "a/b", "a\\b", "\0", ".organizer"] {
        let mut node = valid.clone();
        node.name = name.into();
        assert!(
            validate_tree(&TreeSpec {
                version: 1,
                nodes: vec![node]
            })
            .is_err(),
            "{name:?}"
        );
    }
    for description in ["a\nb", "a\rb", "a\tb", " leading", "trailing "] {
        let mut node = valid.clone();
        node.description = Some(description.into());
        assert!(
            render_tree(&TreeSpec {
                version: 1,
                nodes: vec![node]
            })
            .is_err()
        );
    }
}

#[test]
fn validator_checks_parent_references_types_and_cycles() {
    let cases = [
        vec![directory("a", Some("missing"), "a")],
        vec![directory("a", Some("a"), "a")],
        vec![
            directory("a", Some("b"), "a"),
            directory("b", Some("a"), "b"),
        ],
        vec![directory("a", None, "a"), directory("a", None, "b")],
        vec![directory("a", None, "same"), directory("b", None, "same")],
        vec![
            TreeNode {
                kind: NodeKind::File,
                ..directory("a", None, "file")
            },
            directory("b", Some("a"), "child"),
        ],
    ];
    for nodes in cases {
        let tree = TreeSpec { version: 1, nodes };
        assert!(validate_tree(&tree).is_err(), "{tree:?}");
        assert!(render_tree(&tree).is_err());
        assert!(relative_paths(&tree).is_err());
        assert!(diff(&TreeSpec::default(), &tree).is_err());
    }
    assert_eq!(
        validate_tree(&TreeSpec {
            version: 2,
            nodes: vec![]
        }),
        Err(TreeError::UnsupportedVersion(2))
    );
}

#[test]
fn arbitrary_ast_order_has_no_effect() {
    let original = parse_tree("!tree 1\nroot/ @r\n  sub/ @s\n    file @f\n").unwrap();
    let mut reversed = original.clone();
    reversed.nodes.reverse();
    assert_eq!(
        relative_paths(&original).unwrap(),
        relative_paths(&reversed).unwrap()
    );
    assert_eq!(
        render_tree(&original).unwrap(),
        render_tree(&reversed).unwrap()
    );
    assert!(diff(&original, &reversed).unwrap().is_empty());
}

#[test]
fn large_flat_and_deep_asts_use_iterative_traversal() {
    let flat = TreeSpec {
        version: 1,
        nodes: (0..10_000)
            .map(|i| directory(&format!("id_{i}"), None, &format!("node_{i}")))
            .collect(),
    };
    validate_tree(&flat).unwrap();
    assert!(diff(&flat, &flat).unwrap().is_empty());
    let mut deep = TreeSpec::default();
    for i in 0..1_000 {
        let parent = (i > 0).then(|| format!("id_{}", i - 1));
        deep.nodes
            .push(directory(&format!("id_{i}"), parent.as_deref(), "d"));
    }
    deep.nodes.reverse();
    validate_tree(&deep).unwrap();
    let rendered = render_tree(&deep).unwrap();
    assert_eq!(
        render_tree(&parse_tree(&rendered).unwrap()).unwrap(),
        rendered
    );
}

#[test]
fn generated_id_cannot_steal_an_explicit_id_later_in_the_document() {
    let first = parse_tree("!tree 1\na\n").unwrap();
    let source = format!("!tree 1\na\nb @{}\n", first.nodes[0].id);
    let tree = parse_tree(&source).unwrap();
    assert_ne!(tree.nodes[0].id, tree.nodes[1].id);
    assert_eq!(tree.nodes[1].id, first.nodes[0].id);
}

#[test]
fn generated_id_cannot_reuse_a_baseline_id_now_at_another_path() {
    let mut old = parse_tree("!tree 1\na\n").unwrap();
    old.nodes[0].name = "b".into();
    let draft = parse_tree_with_base("!tree 1\na\n", &old).unwrap();
    assert_ne!(old.nodes[0].id, draft.nodes[0].id);
    assert_eq!(compare_trees(&old, &draft).unwrap().unresolved.len(), 2);
}

#[test]
fn conflicting_implicit_inheritance_requires_an_explicit_decision() {
    let old = parse_tree("!tree 1\na @f\n").unwrap();
    assert!(parse_tree_with_base("!tree 1\na\nb @f\n", &old).is_err());
    let new = parse_tree_with_base("!tree 1\na @new\nb @f\n", &old).unwrap();
    assert_eq!(compare_trees(&old, &new).unwrap().unresolved.len(), 1);
}

#[cfg(windows)]
#[test]
fn windows_rejects_device_names_and_invalid_segments() {
    for name in [
        "CON",
        "nul.txt",
        "COM1",
        "LPT9",
        "a:b",
        "trailing.",
        "trailing ",
        ".ORGANIZER",
    ] {
        assert!(super::validate::validate_name(name).is_err(), "{name}");
    }
}
