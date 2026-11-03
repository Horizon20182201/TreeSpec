use organizer::tree::*;

#[test]
fn parses_unicode_spaces_comments_and_json_names() {
    let tree = parse_tree(
        r#"# document comment

!tree 1
科研资料/ @root # 语义说明
  "2026 @ Research"/ @year
    "C#"/ @csharp # language
      "say \"hello\".txt" @quote # 文件注释
    "a # b.txt" @hash
    normal name.txt @normal
"#,
    )
    .unwrap();
    let paths = relative_paths(&tree).unwrap();
    assert_eq!(
        paths["quote"],
        "科研资料/2026 @ Research/C#/say \"hello\".txt"
    );
    assert_eq!(paths["hash"], "科研资料/2026 @ Research/a # b.txt");
    assert_eq!(tree.nodes[0].description.as_deref(), Some("语义说明"));
    assert_eq!(tree.nodes[3].description.as_deref(), Some("文件注释"));
    assert!(
        diff(&tree, &parse_tree(&render_tree(&tree).unwrap()).unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn canonical_render_is_sorted_lf_and_idempotent() {
    let source = "!tree 1\r\nz.txt @z\r\nb/ @b\r\n  f @f\r\na/ @a\r\nA.txt @upper\r\n";
    let normalized = render_tree(&parse_tree(source).unwrap()).unwrap();
    assert_eq!(
        normalized,
        "!tree 1\na/ @a\nb/ @b\n  f @f\nA.txt @upper\nz.txt @z\n"
    );
    assert_eq!(
        render_tree(&parse_tree(&normalized).unwrap()).unwrap(),
        normalized
    );
}

#[test]
fn comments_do_not_change_parentage() {
    let tree = parse_tree("# preamble\n!tree 1\nroot/ @r\n # ignored indentation\n\n  file @f\n  # ignored\nnext/ @n\n").unwrap();
    assert_eq!(tree.nodes[1].parent_id.as_deref(), Some("r"));
    assert_eq!(tree.nodes[2].parent_id, None);
}

#[test]
fn missing_ids_are_unique_deterministic_and_persisted() {
    let source = "!tree 1\n资料/\n  子目录/\n    a.txt\nb.txt\n";
    let tree = parse_tree(source).unwrap();
    assert_eq!(tree, parse_tree(source).unwrap());
    let ids: std::collections::HashSet<_> = tree.nodes.iter().map(|node| &node.id).collect();
    assert_eq!(ids.len(), tree.nodes.len());
    assert!(tree.nodes[0].id.starts_with("d_"));
    assert!(tree.nodes[2].id.starts_with("f_"));
    assert_eq!(
        tree,
        parse_normalized_tree(&render_tree(&tree).unwrap()).unwrap()
    );
    assert!(parse_normalized_tree(source).is_err());
}

#[test]
fn baseline_matching_uses_path_and_kind_only() {
    let old = parse_tree("!tree 1\na/ @dir\n  file @file\nchange_kind @was_file\n").unwrap();
    let new =
        parse_tree_with_base("!tree 1\na/\n  file\nchange_kind/\nsimilar_file\n", &old).unwrap();
    assert_eq!(new.nodes[0].id, "dir");
    assert_eq!(new.nodes[1].id, "file");
    assert_ne!(new.nodes[2].id, "was_file");
    assert_ne!(new.nodes[3].id, "file");
}

#[test]
fn descendant_without_id_does_not_inherit_after_parent_rename() {
    let old = parse_tree("!tree 1\na/ @d\n  file @f\n").unwrap();
    let new = parse_tree_with_base("!tree 1\nb/ @d\n  file\n", &old).unwrap();
    assert_ne!(new.nodes[1].id, "f");
    assert_eq!(compare_trees(&old, &new).unwrap().unresolved.len(), 2);
}

#[test]
fn same_names_in_different_directories_are_valid() {
    let tree = parse_tree("!tree 1\na/ @a\n  x @x\nb/ @b\n  x @y\n").unwrap();
    assert_eq!(tree.nodes.len(), 4);
}

#[test]
fn rejects_malformed_documents_with_line_numbers() {
    let cases = [
        "",
        "# only comments\n",
        "!tree 2\n",
        " !tree 1\n",
        "file\n!tree 1\n",
        "!tree 1\n!tree 1\n",
        "!tree 1\n  orphan\n",
        "!tree 1\na/\n   bad\n",
        "!tree 1\na/\n    skipped\n",
        "!tree 1\nfile\n  child\n",
        "!tree 1\na @same\nb @same\n",
        "!tree 1\nsame/ @a\nsame @b\n",
        "!tree 1\na/\n  x\n  x\n",
        "!tree 1\na/\n\tbad\n",
        "!tree 1\n# tab\t\n",
        "!tree 1\na @\n",
        "!tree 1\na @bad.id\n",
        "!tree 1\na @id @other\n",
        "!tree 1\na @id extra\n",
        "!tree 1\n\"unterminated\n",
        "!tree 1\n\"bad\\q\"\n",
        "!tree 1\n\"bad\\uD800\"\n",
        "!tree 1\n\"name\"junk\n",
        "!tree 1\n\"name\"@id\n",
        "!tree 1\nna\"me\n",
        "!tree 1\n@only_id\n",
        "!tree 1\na\rb\n",
    ];
    for source in cases {
        let error = parse_tree(source).unwrap_err().to_string();
        assert!(error.starts_with("line "), "{source:?}: {error}");
    }
    assert!(
        parse_tree("!tree 1\n\na @bad!\n")
            .unwrap_err()
            .to_string()
            .starts_with("line 3:")
    );
}

#[test]
fn rejects_path_traversal_separators_nul_and_reserved_metadata() {
    for name in [
        "",
        ".",
        "..",
        "a/b",
        "/absolute",
        "a\\b",
        "\0",
        ".organizer",
    ] {
        let quoted = serde_json::to_string(name).unwrap();
        assert!(
            parse_tree(&format!("!tree 1\n{quoted} @id\n")).is_err(),
            "{name:?}"
        );
    }
    for source in [
        "!tree 1\n../ @id\n",
        "!tree 1\na/b @id\n",
        "!tree 1\na// @id\n",
        "!tree 1\nroot/\n  .organizer/\n",
    ] {
        assert!(parse_tree(source).is_err());
    }
}

#[test]
fn json_escapes_and_syntax_like_names_round_trip() {
    let names = [
        "#hash",
        "@id",
        "!tree 1",
        " leading",
        "trailing ",
        "a # b",
        "a @ b",
        "a\"b",
        "😀",
        "C#",
        "mail@example.com",
    ];
    for (i, name) in names.iter().enumerate() {
        #[cfg(windows)]
        if name.ends_with(' ') {
            // Trailing spaces are valid on Unix but rejected by the Windows validator.
            continue;
        }
        let source = format!(
            "!tree 1\n{} @n{i} #\n",
            serde_json::to_string(name).unwrap()
        );
        let tree = parse_tree(&source).unwrap();
        assert_eq!(tree.nodes[0].name, *name);
        assert_eq!(tree.nodes[0].description.as_deref(), Some(""));
        assert_eq!(tree, parse_tree(&render_tree(&tree).unwrap()).unwrap());
    }
    let tree = parse_tree("!tree 1\n\"\\u4e2d\\u6587 \\uD83D\\uDE00\" @id\n").unwrap();
    assert_eq!(tree.nodes[0].name, "中文 😀");
}

#[cfg(not(windows))]
#[test]
fn escaped_control_characters_in_unix_filenames_round_trip() {
    let tree = parse_tree("!tree 1\n\"line\\nwith\\ttab\" @id\n").unwrap();
    assert_eq!(tree.nodes[0].name, "line\nwith\ttab");
    assert_eq!(tree, parse_tree(&render_tree(&tree).unwrap()).unwrap());
}

#[test]
fn empty_tree_is_valid() {
    assert_eq!(parse_tree("!tree 1").unwrap(), TreeSpec::default());
    assert_eq!(render_tree(&TreeSpec::default()).unwrap(), "!tree 1\n");
}
