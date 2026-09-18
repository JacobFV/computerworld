use super::*;
use crate::{AppState, DesktopState, NativeApp, TerminalEntry};

const W: u64 = 7;

fn workspace() -> Code {
    let (mut app, effects) = Code::launch("", W, 0);
    assert!(effects.is_empty());
    let effects = app.attach("/home/alice", "/home/alice/.local/share/Trash/files", W);
    assert!(
        matches!(&effects[0], AppEffect::ReadFiles { tag, paths, .. }
        if tag == "settings" && paths[0] == "/home/alice/.config/Code/User/settings.json")
    );
    assert!(
        matches!(&effects[1], AppEffect::ListTree { path, .. } if path == "/home/alice/project")
    );
    let more = app.tree_listed(
        W,
        "/home/alice/project",
        TREE_DEPTH,
        Ok(vec![
            "README.md".into(),
            "main.py".into(),
            "src/".into(),
            "src/app.js".into(),
            "src/util.py".into(),
            ".git/".into(),
            ".git/state.json".into(),
        ]),
    );
    // A newly opened folder asks git about itself.
    assert!(
        matches!(&more[0], AppEffect::ShellRun { tag, command, cwd, .. }
        if tag == "git:status" && command == "git status" && cwd == "/home/alice/project")
    );
    app
}
fn open(app: &mut Code, rel: &str, text: &str) {
    let effects = app.click(W, &format!("code:tree:{rel}"), 0).unwrap();
    let AppEffect::ReadFiles { paths, tag, .. } = &effects[0] else {
        panic!("opening a file reads it: {effects:?}");
    };
    assert_eq!(tag, "open");
    let path = paths[0].clone();
    app.files_read(W, "open", vec![(path, Ok(text.to_owned()))]);
}

#[test]
fn a_missing_project_folder_leaves_the_welcome_page() {
    let (mut app, _) = Code::launch("", W, 0);
    app.attach("/Users/alice", "/Users/alice/.Trash", W);
    assert_eq!(app.platform, Platform::Mac);
    assert_eq!(app.settings.font_size, 12);
    assert!(app
        .settings_path()
        .contains("Library/Application Support/Code/User"));
    app.tree_listed(
        W,
        "/Users/alice/project",
        TREE_DEPTH,
        Err("folder not found".into()),
    );
    assert!(app.workspace().is_none());
    assert!(
        app.notice.is_none(),
        "a missing default folder is not an error"
    );
    // Explicit folders that fail are reported.
    let (mut app, effects) = Code::launch("/nowhere", W, 0);
    assert!(matches!(&effects[0], AppEffect::ListTree { path, .. } if path == "/nowhere"));
    app.tree_listed(W, "/nowhere", TREE_DEPTH, Err("folder not found".into()));
    assert!(app.notice.as_deref().unwrap().contains("folder not found"));
}

#[test]
fn the_explorer_is_the_real_listing_sorted_with_folders_first() {
    let mut app = workspace();
    let rows: Vec<String> = app.tree_rows().into_iter().map(|r| r.0).collect();
    // .git is excluded, folders come first, and collapsed folders hide their children.
    assert_eq!(rows, vec!["src", "main.py", "README.md"]);
    app.click(W, "code:tree:src", 0).unwrap();
    let rows: Vec<(String, usize, bool)> = app.tree_rows();
    assert_eq!(rows[1], ("src/app.js".into(), 1, false));
    app.click(W, "code:tree:src", 0).unwrap();
    assert_eq!(app.tree_rows().len(), 3);
    assert!(app.click(W, "code:tree:gone.txt", 0).is_err());
}

#[test]
fn single_clicks_open_previews_and_double_clicks_or_edits_pin() {
    let mut app = workspace();
    open(&mut app, "main.py", "print('hi')\n");
    assert_eq!(app.tabs.len(), 1);
    assert!(app.tabs[0].preview);
    open(&mut app, "README.md", "# Readme\n");
    // The preview was replaced, not added to.
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].name(), "README.md");
    app.activate(W, "code:tab:0", 0).unwrap();
    assert!(!app.tabs[0].preview);
    open(&mut app, "main.py", "print('hi')\n");
    assert_eq!(app.tabs.len(), 2);
    // Typing into a preview pins it.
    app.text_effects(W, "x").unwrap();
    assert!(!app.tabs[1].preview);
}

#[test]
fn editing_and_saving_writes_the_file_and_marks_it_clean_only_when_written() {
    let mut app = workspace();
    open(&mut app, "main.py", "def main():\n    pass\n");
    app.key(W, "Ctrl+End", 0).unwrap();
    app.text_effects(W, "main()").unwrap();
    assert!(app.tabs[0].dirty());
    assert!(app.modified());
    let effects = app.key(W, "Ctrl+s", 0).unwrap();
    let AppEffect::WriteFile { path, content, .. } = &effects[0] else {
        panic!("save writes: {effects:?}");
    };
    assert_eq!(path, "/home/alice/project/main.py");
    assert_eq!(content, "def main():\n    pass\nmain()");
    // Still dirty until the machine says the write happened.
    assert!(app.tabs[0].dirty());
    app.written(W, path, content);
    assert!(!app.tabs[0].dirty());
    // Undo returns to the text on disk, and the tab is dirty again.
    app.key(W, "Ctrl+z", 0).unwrap();
    assert_eq!(app.tabs[0].doc.text, "def main():\n    pass\n");
    assert!(app.tabs[0].dirty());
    app.key(W, "Ctrl+Shift+Z", 0).unwrap();
    assert!(!app.tabs[0].dirty());
}

#[test]
fn crlf_files_keep_their_line_endings_on_save() {
    let mut app = workspace();
    open(&mut app, "README.md", "a\r\nb\r\n");
    assert_eq!(app.tabs[0].doc.text, "a\nb\n");
    assert!(app.tabs[0].crlf);
    app.text_effects(W, "z").unwrap();
    let effects = app.key(W, "Meta+s", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::WriteFile { content, .. } if content == "za\r\nb\r\n")
    );
}

#[test]
fn closing_a_dirty_editor_asks_first() {
    let mut app = workspace();
    open(&mut app, "main.py", "x");
    app.text_effects(W, "y").unwrap();
    app.key(W, "Ctrl+w", 0).unwrap();
    let dialog = app.dialog.clone().expect("a save prompt");
    assert!(dialog.message.contains("main.py"));
    assert_eq!(app.tabs.len(), 1);
    // Don't Save closes without writing.
    let effects = app.click(W, "code:dialog:1", 0).unwrap();
    assert!(effects.is_empty());
    assert!(app.tabs.is_empty());
}

#[test]
fn workspace_search_reads_the_files_and_results_open_at_the_line() {
    let mut app = workspace();
    app.key(W, "Ctrl+Shift+F", 0).unwrap();
    assert_eq!(app.view, View::Search);
    let effects = app.text_effects(W, "TODO").unwrap();
    let AppEffect::ReadFiles { tag, paths, .. } = &effects[0] else {
        panic!("search reads the workspace: {effects:?}");
    };
    assert_eq!(tag, "search");
    assert_eq!(paths.len(), 4, "every file but .git's: {paths:?}");
    app.files_read(
        W,
        "search",
        vec![
            (
                "/home/alice/project/main.py".into(),
                Ok("x = 1\n# TODO fix\n# todo lower\n".into()),
            ),
            (
                "/home/alice/project/src/app.js".into(),
                Ok("// nothing\n".into()),
            ),
            ("/home/alice/project/README.md".into(), Err("binary".into())),
        ],
    );
    // Case-insensitive by default.
    assert_eq!(app.search.results.len(), 1);
    assert_eq!(app.search.total(), 2);
    assert_eq!(app.search.skipped, 1);
    let hit = &app.search.results[0].hits[0];
    assert_eq!((hit.line, &hit.preview[hit.start..hit.end]), (2, "TODO"));
    // Match Case narrows it; the toggle searches again.
    let effects = app.click(W, "code:search:case", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadFiles { .. }));
    app.files_read(
        W,
        "search",
        vec![(
            "/home/alice/project/main.py".into(),
            Ok("x = 1\n# TODO fix\n# todo lower\n".into()),
        )],
    );
    assert_eq!(app.search.total(), 1);
    // A result opens the file with the caret on the match.
    let effects = app.click(W, "code:search-result:0:0", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadFiles { tag, .. } if tag == "open"));
    app.files_read(
        W,
        "open",
        vec![(
            "/home/alice/project/main.py".into(),
            Ok("x = 1\n# TODO fix\n# todo lower\n".into()),
        )],
    );
    assert_eq!(app.tabs[0].doc.selected_text(), "TODO");
    // An invalid regular expression is said so.
    app.click(W, "code:search:regex", 0).unwrap();
    app.focus = Focus::Search;
    app.text_effects(W, "(").unwrap();
    assert!(app.search.error.is_some());
}

#[test]
fn replace_all_in_files_writes_every_file_that_matched() {
    let mut app = workspace();
    app.search.query = "old".into();
    app.search.replace = "new".into();
    app.search.results = vec![FileHits {
        path: "main.py".into(),
        hits: vec![Hit::default()],
    }];
    let effects = app.click(W, "code:search-replace-all", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadFiles { tag, .. } if tag == "replace"));
    let effects = app.files_read(
        W,
        "replace",
        vec![(
            "/home/alice/project/main.py".into(),
            Ok("old = old\n".into()),
        )],
    );
    assert!(
        matches!(&effects[0], AppEffect::WriteFile { content, .. } if content == "new = new\n")
    );
}

#[test]
fn run_saves_then_executes_the_file_and_tracebacks_become_problems() {
    let mut app = workspace();
    open(&mut app, "main.py", "print(x)\n");
    app.text_effects(W, "#").unwrap();
    let effects = app.key(W, "F5", 0).unwrap();
    // Saved first, then a terminal session is started and the file run in it.
    let AppEffect::WriteFile { path, content, .. } = &effects[0] else {
        panic!("saved first")
    };
    app.written(W, path, content);
    assert!(
        matches!(&effects[1], AppEffect::ShellRun { tag, command, .. } if tag == "prompt:0" && command.is_empty())
    );
    let AppEffect::ShellRun {
        tag, command, cwd, ..
    } = &effects[2]
    else {
        panic!("run executes: {effects:?}");
    };
    assert_eq!(
        (tag.as_str(), command.as_str(), cwd.as_str()),
        ("run:0", "python3 main.py", "/home/alice/project")
    );
    assert!(app.panel_open && app.panel == PanelTab::Terminal);
    let err = "Traceback (most recent call last):\n  File \"/home/alice/project/main.py\", line 1, in <module>\n    #print(x)\nNameError: name 'x' is not defined\n";
    let more = app.shell_ran(
        W,
        "run:0",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "alice@box:~/project$",
                "python3 main.py",
                "",
                err,
                1,
            )),
            cwd: "/home/alice/project".into(),
            prompt: "alice@box:~/project$".into(),
            clear: false,
        },
    );
    // The command changed nothing it knows of, but the tree, git and clean editors are
    // refreshed as a file watcher would.
    assert!(more.iter().any(|e| matches!(e, AppEffect::ListTree { .. })));
    assert_eq!(app.terminals[0].transcript.len(), 1);
    assert_eq!(app.terminals[0].transcript[0].exit_code, 1);
    assert_eq!(app.last_run, Some(("python3 main.py".into(), 1)));
    let problems = app.problems_all();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].line, 1);
    assert!(problems[0].message.starts_with("NameError"));
    assert_eq!(app.counts(), (1, 0));
    // A problem opens its file at its line.
    app.click(W, "code:tab-close:0", 0).unwrap();
    let effects = app.click(W, "code:problem:0", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadFiles { .. }));
    // Missing interpreter: the shell's own 127 is shown, and nothing is blamed.
    app.shell_ran(
        W,
        "run:0",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "$",
                "python3 main.py",
                "",
                "python3: command not found\n",
                127,
            )),
            cwd: "/home/alice/project".into(),
            prompt: "$".into(),
            clear: false,
        },
    );
    assert!(app.problems_all().is_empty());
    assert_eq!(app.last_run, Some(("python3 main.py".into(), 127)));
}

#[test]
fn the_terminal_is_a_session_with_its_own_directory_and_history() {
    let mut app = workspace();
    let effects = app.key(W, "Ctrl+`", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::ShellRun { command, .. } if command.is_empty()));
    app.shell_ran(
        W,
        "prompt:0",
        ShellOutcome {
            entry: None,
            cwd: "/home/alice/project".into(),
            prompt: "alice@box:~/project$".into(),
            clear: false,
        },
    );
    assert_eq!(app.terminals[0].prompt, "alice@box:~/project$");
    app.text_effects(W, "cd src").unwrap();
    let effects = app.key(W, "Enter", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ShellRun { tag, command, cwd, .. }
        if tag == "term:0" && command == "cd src" && cwd == "/home/alice/project")
    );
    app.shell_ran(
        W,
        "term:0",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "alice@box:~/project$",
                "cd src",
                "",
                "",
                0,
            )),
            cwd: "/home/alice/project/src".into(),
            prompt: "alice@box:~/project/src$".into(),
            clear: false,
        },
    );
    assert_eq!(app.terminals[0].cwd, "/home/alice/project/src");
    app.key(W, "ArrowUp", 0).unwrap();
    assert_eq!(app.terminals[0].input, "cd src");
    app.key(W, "Ctrl+c", 0).unwrap();
    assert!(app.terminals[0].input.is_empty());
    // clear wipes the screen.
    app.shell_ran(
        W,
        "term:0",
        ShellOutcome {
            entry: Some(TerminalEntry::new("$", "clear", "", "", 0)),
            cwd: "/home/alice/project/src".into(),
            prompt: "$".into(),
            clear: true,
        },
    );
    assert!(app.terminals[0].transcript.is_empty());
}

#[test]
fn the_command_palette_and_quick_open_match_fuzzily() {
    let mut app = workspace();
    app.key(W, "Ctrl+Shift+P", 0).unwrap();
    assert_eq!(app.quick.as_ref().unwrap().value, ">");
    app.text_effects(W, "tog term").unwrap();
    let items = app.quick_items();
    assert_eq!(items[0].label, "View: Toggle Terminal");
    let effects = app.key(W, "Enter", 0).unwrap();
    assert!(app.quick.is_none());
    assert!(matches!(&effects[0], AppEffect::ShellRun { .. }));
    // Commands that cannot run now are not offered.
    app.key(W, "Ctrl+Shift+P", 0).unwrap();
    assert!(!app
        .quick_items()
        .iter()
        .any(|i| i.label == "File: Revert File"));
    app.key(W, "Escape", 0).unwrap();
    app.key(W, "Ctrl+p", 0).unwrap();
    app.text_effects(W, "util").unwrap();
    let items = app.quick_items();
    assert_eq!(items[0].label, "util.py");
    assert_eq!(items[0].detail, "src");
    let effects = app.key(W, "Enter", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ReadFiles { paths, .. } if paths[0] == "/home/alice/project/src/util.py")
    );
    assert!(!app.tabs[0].preview, "Quick Open pins");
    // Go to line through the same box.
    app.files_read(
        W,
        "open",
        vec![(
            "/home/alice/project/src/util.py".into(),
            Ok("a\nb\nc\n".into()),
        )],
    );
    app.key(W, "Ctrl+g", 0).unwrap();
    app.text_effects(W, "3").unwrap();
    assert!(app.quick_items()[0].label.contains("line 3"));
    app.key(W, "Enter", 0).unwrap();
    assert_eq!(app.tabs[0].doc.position(), (2, 0));
}

#[test]
fn find_and_replace_in_the_editor() {
    let mut app = workspace();
    open(&mut app, "main.py", "foo bar foo\nfoo");
    app.key(W, "Ctrl+f", 0).unwrap();
    assert_eq!(app.focus, Focus::Find);
    app.text_effects(W, "foo").unwrap();
    assert_eq!(app.tabs[0].doc.selection(), (0, 3));
    app.key(W, "Enter", 0).unwrap();
    assert_eq!(app.tabs[0].doc.selection(), (8, 11));
    app.key(W, "Shift+Enter", 0).unwrap();
    assert_eq!(app.tabs[0].doc.selection(), (0, 3));
    app.key(W, "Ctrl+h", 0).unwrap();
    assert_eq!(app.focus, Focus::Replace);
    app.text_effects(W, "baz").unwrap();
    app.key(W, "Enter", 0).unwrap();
    assert_eq!(app.tabs[0].doc.text, "baz bar foo\nfoo");
    app.click(W, "code:find:replace-all", 0).unwrap();
    assert_eq!(app.tabs[0].doc.text, "baz bar baz\nbaz");
    app.key(W, "Escape", 0).unwrap();
    assert!(app.find.is_none());
}

#[test]
fn explorer_creates_renames_and_trashes_real_paths() {
    let mut app = workspace();
    app.click(W, "code:tree:src", 0).unwrap();
    app.click(W, "code:cmd:explorer.newFile", 0).unwrap();
    assert_eq!(app.inline.as_ref().unwrap().parent, "src");
    app.text_effects(W, "new.py").unwrap();
    let effects = app.key(W, "Enter", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::CreateFile { path, .. } if path == "/home/alice/project/src/new.py")
    );
    assert!(effects
        .iter()
        .any(|e| matches!(e, AppEffect::ListTree { .. })));
    assert!(effects
        .iter()
        .any(|e| matches!(e, AppEffect::ReadFiles { tag, .. } if tag == "open")));
    // An existing name is refused.
    app.click(W, "code:cmd:explorer.newFile", 0).unwrap();
    app.text_effects(W, "app.js").unwrap();
    assert!(app.key(W, "Enter", 0).is_err());
    // Rename moves the file, and its open editor follows.
    open(&mut app, "main.py", "x");
    app.selected = Some("main.py".into());
    app.focus = Focus::Explorer;
    app.key(W, "F2", 0).unwrap();
    app.inline.as_mut().unwrap().value = "app.py".into();
    let effects = app.key(W, "Enter", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::MovePath { from, to, .. }
        if from == "/home/alice/project/main.py" && to == "/home/alice/project/app.py"));
    assert!(app
        .tabs
        .iter()
        .any(|t| t.path == "/home/alice/project/app.py"));
    // Delete asks, then moves to the trash.
    app.selected = Some("README.md".into());
    app.focus = Focus::Explorer;
    app.key(W, "Delete", 0).unwrap();
    assert!(app.dialog.is_some());
    let effects = app.click(W, "code:dialog:0", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::TrashPath { path, trash, .. }
        if path == "/home/alice/project/README.md" && trash.ends_with("Trash/files"))
    );
}

#[test]
fn source_control_reads_git_status_stages_and_commits() {
    let mut app = workspace();
    app.shell_ran(
        W,
        "git:status",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "$",
                "git status",
                "On branch main\nM  main.py\n M src/app.js\n A notes.txt\n",
                "",
                0,
            )),
            ..ShellOutcome::default()
        },
    );
    assert_eq!(app.scm.repo, Some(true));
    assert_eq!(app.scm.branch, "main");
    assert_eq!(app.scm.staged, vec![('M', "main.py".to_owned())]);
    assert_eq!(
        app.scm.changes,
        vec![
            ('M', "src/app.js".to_owned()),
            ('U', "notes.txt".to_owned())
        ]
    );
    let effects = app.click(W, "code:scm-stage:src/app.js", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ShellRun { command, .. } if command == "git add src/app.js")
    );
    // Committing needs a message, and quotes it for the shell.
    assert!(app.run_command(W, "git.commit").is_err());
    app.focus = Focus::ScmMessage;
    app.text_effects(W, "Fix the app's bug").unwrap();
    let effects = app.key(W, "Ctrl+Enter", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::ShellRun { command, .. } if command == "git commit -m 'Fix the app'\\''s bug'")
    );
    app.shell_ran(
        W,
        "git:commit",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "$",
                "git commit",
                "[main 1234abcd] Fix\n",
                "",
                0,
            )),
            ..ShellOutcome::default()
        },
    );
    assert!(app.scm.message.is_empty());
    // A folder that is no repository offers to make one.
    app.shell_ran(
        W,
        "git:status",
        ShellOutcome {
            entry: Some(TerminalEntry::new(
                "$",
                "git status",
                "",
                "git: not a git repository\n",
                1,
            )),
            ..ShellOutcome::default()
        },
    );
    assert_eq!(app.scm.repo, Some(false));
    assert!(app.enabled("git.init").is_ok());
    assert_eq!(quote("a b", true), "'a b'");
    assert_eq!(quote("it's", true), "'it'`''s'");
    assert_eq!(quote("it's", false), "'it'\\''s'");
}

#[test]
fn settings_are_read_applied_and_written_where_vscode_keeps_them() {
    let mut app = workspace();
    let path = app.settings_path();
    app.files_read(W, "settings", vec![(
        path,
        Ok("{\n  // mine\n  \"workbench.colorTheme\": \"Default Light Modern\",\n  \"editor.fontSize\": 16,\n  \"editor.wordWrap\": \"on\",\n  \"editor.tabSize\": 2,\n}".into()),
    )]);
    assert!(!app.settings.dark);
    assert_eq!(
        (
            app.settings.font_size,
            app.settings.word_wrap,
            app.settings.tab_size
        ),
        (16, true, 2)
    );
    let effects = app.click(W, "code:settings:theme:dark", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::CreateDirectory { path, .. } if path == "/home/alice/.config/Code/User")
    );
    let AppEffect::WriteFile { path, content, .. } = &effects[1] else {
        panic!()
    };
    assert_eq!(path, &app.settings_path());
    assert!(content.contains("\"workbench.colorTheme\": \"Default Dark Modern\""));
    assert!(app.settings.dark);
    // Theme picker through the palette.
    app.key(W, "Ctrl+k", 0).unwrap();
    app.key(W, "Ctrl+t", 0).unwrap();
    // A chord's second key is not typed, even when it arrives as text.
    let effects = app.quick.clone();
    app.key(W, "Escape", 0).unwrap();
    app.key(W, "Ctrl+k", 0).unwrap();
    let effects_open = app.text_effects(W, "o").unwrap();
    assert_eq!(app.quick.as_ref().unwrap().mode, QuickMode::OpenFolder);
    // The Open Folder dialog lists the folder the workspace sits in.
    assert!(
        matches!(&effects_open[0], AppEffect::ListTree { path, depth: 1, .. } if path == "/home/alice")
    );
    app.key(W, "Escape", 0).unwrap();
    app.quick = effects;
    app.focus = Focus::Quick;
    assert_eq!(app.quick.as_ref().unwrap().mode, QuickMode::Theme);
    app.text_effects(W, "light").unwrap();
    app.key(W, "Enter", 0).unwrap();
    assert!(!app.settings.dark);
}

#[test]
fn clipboard_and_drag_selection_go_through_the_desktop() {
    let mut d = DesktopState {
        home: "/home/alice".into(),
        ..DesktopState::default()
    };
    let (id, effects) = d.launch("code", "/work").unwrap();
    assert!(matches!(&effects[0], AppEffect::ReadFiles { tag, .. } if tag == "settings"));
    d.tree_listed(id, "/work", TREE_DEPTH, Ok(vec!["a.txt".into()]))
        .unwrap();
    d.click("code:tree:a.txt").unwrap();
    d.files_read(
        id,
        "open",
        vec![("/work/a.txt".into(), Ok("hello world".into()))],
    )
    .unwrap();
    d.click("code:editor:0:0:0:0:30").unwrap();
    // Press at column 0, release at column 5: "hello" is selected.
    let (cw, _) = render::cell(14, Platform::Linux);
    d.press_at("code:editor:0:0:0:0:30", 0, 2).unwrap();
    d.click_at("code:editor:0:0:0:0:30", 5 * cw as i32, 2)
        .unwrap();
    let code = |d: &DesktopState| match &d.windows[&id].state {
        AppState::Native(NativeApp::Code(c)) => c.clone(),
        _ => unreachable!(),
    };
    assert_eq!(code(&d).tabs[0].doc.selected_text(), "hello");
    assert!(d.key("Ctrl+c").unwrap().is_empty());
    assert_eq!(d.clipboard_text.as_deref(), Some("hello"));
    d.key("End").unwrap();
    d.key("Ctrl+v").unwrap();
    assert_eq!(code(&d).tabs[0].doc.text, "hello worldhello");
    // Cut takes it out.
    d.key("Ctrl+a").unwrap();
    d.key("Ctrl+x").unwrap();
    assert_eq!(code(&d).tabs[0].doc.text, "");
    assert_eq!(d.clipboard_text.as_deref(), Some("hello worldhello"));
    // The whole state survives a snapshot.
    let restored: DesktopState = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(d, restored);
}

#[test]
fn every_painted_control_is_a_real_target_or_announced_disabled() {
    use crate::desktop_scene::{app_content_with, DesktopTheme};
    let mut app = workspace();
    open(
        &mut app,
        "main.py",
        "import os\n\ndef main():\n    print(os.getcwd())\n",
    );
    app.text_effects(W, "#").unwrap();
    app.key(W, "Ctrl+f", 0).unwrap();
    app.menu = Some("edit".into());
    let settings = crate::SystemSettings::DEFAULT;
    for theme in [
        DesktopTheme::Macos,
        DesktopTheme::Windows,
        DesktopTheme::Ubuntu,
    ] {
        let state = AppState::Native(NativeApp::Code(app.clone()));
        let env = crate::AppEnv {
            theme,
            width: 1100,
            height: 700,
            clock_us: 0,
            settings: &settings,
            clipboard: None,
            share_to: None,
            editor: None,
            pointer: None,
            files: Default::default(),
        };
        let scene = app_content_with(&state, &env);
        let targets: Vec<&str> = scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .collect();
        for t in &targets {
            assert!(t.starts_with("code:"), "{t}");
        }
        for want in [
            "code:tab:0",
            "code:tree:main.py",
            "code:activity:scm",
            "code:cmd:undo",
            "code:find:close",
            "code:status:language",
        ] {
            assert!(targets.contains(&want), "{want} missing on {theme:?}");
        }
        assert!(targets.iter().any(|t| t.starts_with("code:editor:")));
        // Redo has nothing to redo: shown, disabled, with the reason.
        assert!(scene.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|s| s.disabled && s.label.starts_with("Redo"))));
        // Each target a click can reach is one the model accepts.
        for t in targets.iter().filter(|t| {
            t.starts_with("code:cmd:")
                || t.starts_with("code:status:")
                || t.starts_with("code:activity:")
        }) {
            let mut probe = app.clone();
            probe.menu = None;
            probe.click(W, t, 0).unwrap_or_else(|e| panic!("{t}: {e}"));
        }
    }
}

/// The text of the active editor.
fn text(app: &Code) -> String {
    app.active_tab().unwrap().doc.text.clone()
}
/// Every caret, in text order.
fn carets(app: &Code) -> Vec<usize> {
    let doc = &app.active_tab().unwrap().doc;
    let mut all: Vec<usize> = doc.all_carets().into_iter().map(|(c, _)| c).collect();
    all.sort();
    all
}

#[test]
fn several_cursors_type_delete_and_undo_as_one_edit() {
    let mut app = workspace();
    open(&mut app, "main.py", "one\ntwo\nthree\n");
    app.run_command(W, "editor.action.insertCursorBelow")
        .unwrap();
    app.run_command(W, "editor.action.insertCursorBelow")
        .unwrap();
    assert_eq!(
        carets(&app),
        [0, 4, 8],
        "one caret at the start of each line"
    );
    // Typing happens at every caret, and the undo history holds it as one step.
    app.text_effects(W, "#").unwrap();
    app.text_effects(W, " ").unwrap();
    assert_eq!(text(&app), "# one\n# two\n# three\n");
    assert_eq!(carets(&app), [2, 8, 14]);
    app.run_command(W, "undo").unwrap();
    assert_eq!(text(&app), "#one\n#two\n#three\n");
    app.run_command(W, "undo").unwrap();
    assert_eq!(text(&app), "one\ntwo\nthree\n");
    app.run_command(W, "redo").unwrap();
    assert_eq!(text(&app), "#one\n#two\n#three\n");
    // Backspace at every caret, and the carets move together.
    let mut app2 = workspace();
    open(&mut app2, "main.py", "one\ntwo\nthree\n");
    app2.run_command(W, "editor.action.insertCursorBelow")
        .unwrap();
    app2.text_effects(W, "x").unwrap();
    app2.key(W, "ArrowRight", 0).unwrap();
    assert_eq!(carets(&app2), [2, 7]);
    app2.key(W, "Backspace", 0).unwrap();
    assert_eq!(text(&app2), "xne\nxwo\nthree\n");
    // Escape leaves one cursor.
    app2.key(W, "Escape", 0).unwrap();
    assert_eq!(carets(&app2).len(), 1);
}

#[test]
fn ctrl_d_adds_the_next_occurrence_and_ctrl_shift_l_takes_them_all() {
    let mut app = workspace();
    open(
        &mut app,
        "main.py",
        "value = 1\nprint(value)\nvalue += value\n",
    );
    // With nothing selected the first press selects the word at the caret.
    app.run_command(W, "editor.action.addSelectionToNextFindMatch")
        .unwrap();
    assert_eq!(app.active_tab().unwrap().doc.selected_text(), "value");
    app.run_command(W, "editor.action.addSelectionToNextFindMatch")
        .unwrap();
    assert_eq!(carets(&app).len(), 2);
    // Typing replaces every selected occurrence.
    app.text_effects(W, "total").unwrap();
    assert_eq!(text(&app), "total = 1\nprint(total)\nvalue += value\n");
    app.run_command(W, "undo").unwrap();
    assert_eq!(text(&app), "value = 1\nprint(value)\nvalue += value\n");
    // Select all occurrences: four of them, and one edit changes them all.
    let mut app = workspace();
    open(
        &mut app,
        "main.py",
        "value = 1\nprint(value)\nvalue += value\n",
    );
    app.run_command(W, "editor.action.selectHighlights")
        .unwrap();
    assert_eq!(carets(&app).len(), 4);
    app.text_effects(W, "n").unwrap();
    assert_eq!(text(&app), "n = 1\nprint(n)\nn += n\n");
    app.run_command(W, "undo").unwrap();
    assert_eq!(text(&app), "value = 1\nprint(value)\nvalue += value\n");
}

#[test]
fn alt_click_adds_a_cursor_and_a_plain_click_takes_them_away() {
    let mut app = workspace();
    open(&mut app, "main.py", "one\ntwo\nthree\n");
    let (cw, rh) = render::cell(app.settings.font_size, app.platform);
    let target = "code:editor:0:0:0:0:30";
    // A plain click puts one caret on line 2, column 1.
    app.press_at(target, cw as i32, rh as i32).unwrap();
    app.click_at(W, target, cw as i32, rh as i32, 0).unwrap();
    assert_eq!(carets(&app), [5]);
    // Alt held, the next click adds a second caret rather than moving the first.
    app.modifiers = crate::apps::imaging::MOD_ALT;
    app.press_at(target, cw as i32, 2 * rh as i32).unwrap();
    app.click_at(W, target, cw as i32, 2 * rh as i32, 0)
        .unwrap();
    assert_eq!(carets(&app), [5, 9]);
    app.text_effects(W, "!").unwrap();
    assert_eq!(text(&app), "one\nt!wo\nt!hree\n");
    // Without Alt the extra cursors go away again.
    app.modifiers = 0;
    app.press_at(target, cw as i32, 0).unwrap();
    app.click_at(W, target, cw as i32, 0, 0).unwrap();
    assert_eq!(carets(&app), [1]);
}

/// Paint the workbench and hand back its scene.
fn painted(app: &Code) -> cw_scene::Scene {
    let mut p = Painter::themed(DesktopTheme::Ubuntu, 1200, 800, 1);
    render::render(
        app,
        &mut p,
        &crate::AppEnv {
            theme: DesktopTheme::Ubuntu,
            width: 1200,
            height: 800,
            clock_us: 0,
            settings: &crate::SystemSettings::DEFAULT,
            clipboard: None,
            share_to: None,
            editor: None,
            pointer: None,
            files: Default::default(),
        },
    );
    p.scene
}
fn targets(scene: &cw_scene::Scene) -> Vec<String> {
    scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}

#[test]
fn splitting_the_editor_gives_each_group_its_own_editors() {
    let mut app = workspace();
    open(&mut app, "main.py", "print(1)\n");
    assert_eq!(app.group_count(), 1);
    app.run_command(W, "workbench.action.splitEditorRight")
        .unwrap();
    assert_eq!(app.group_count(), 2);
    assert_eq!(app.focus_group, 1, "the new group takes the focus");
    assert!(matches!(&app.layout, Slot::Split { row: true, children } if children.len() == 2));
    // Both groups show the file, and the two editors are painted side by side.
    assert_eq!(app.group_tabs(0).len(), 1);
    assert_eq!(app.group_tabs(1).len(), 1);
    let scene = painted(&app);
    let editors: Vec<String> = targets(&scene)
        .into_iter()
        .filter(|t| t.starts_with("code:editor:"))
        .collect();
    assert_eq!(editors.len(), 2, "{editors:?}");
    assert!(editors.iter().any(|t| t.starts_with("code:editor:0:")));
    assert!(editors.iter().any(|t| t.starts_with("code:editor:1:")));
    // The two views are one document, as they are in VS Code: typing in the focused
    // group shows up in the other, and so does undo.
    app.active_mut().unwrap().doc.set(0, false);
    app.text_effects(W, "# ").unwrap();
    let other = app.group_tabs(0)[0];
    assert_eq!(app.tabs[other].doc.text, "# print(1)\n");
    assert!(app.tabs[other].dirty());
    app.run_command(W, "undo").unwrap();
    assert_eq!(app.tabs[other].doc.text, "print(1)\n");
    // Opening another file only touches the focused group.
    open(&mut app, "README.md", "# hi\n");
    assert_eq!(app.group_tabs(0).len(), 1);
    assert_eq!(app.group_tabs(1).len(), 2);
    // Focusing a group brings back its editor.
    app.run_command(W, "workbench.action.focusFirstEditorGroup")
        .unwrap();
    assert_eq!(app.focus_group, 0);
    assert_eq!(
        app.active_tab().unwrap().path,
        "/home/alice/project/main.py"
    );
    app.run_command(W, "workbench.action.focusNextGroup")
        .unwrap();
    assert_eq!(app.focus_group, 1);
    assert_eq!(app.active_tab().unwrap().name(), "README.md");
    // Move it to the other group: it leaves this one and lands there.
    app.run_command(W, "workbench.action.moveEditorToPreviousGroup")
        .unwrap();
    assert_eq!(app.focus_group, 0);
    assert_eq!(app.active_tab().unwrap().name(), "README.md");
    assert_eq!(app.group_tabs(0).len(), 2);
    assert_eq!(app.group_tabs(1).len(), 1);
    // Closing the last editor of a group closes the group with it.
    app.go_to_group(1);
    let only = app.group_tabs(1)[0];
    app.close_tab(only, true).unwrap();
    assert_eq!(app.group_count(), 1);
    assert_eq!(app.layout, Slot::Leaf(0));
    assert_eq!(app.group_tabs(0).len(), 2);
    // A third group splits downwards under the second.
    app.run_command(W, "workbench.action.splitEditorDown")
        .unwrap();
    assert!(matches!(&app.layout, Slot::Split { row: false, .. }));
    assert_eq!(app.group_count(), 2);
}

#[test]
fn the_minimap_is_a_real_map_that_scrolls_the_editor() {
    let mut app = workspace();
    let long: String = (0..200).map(|i| format!("line {i}\n")).collect();
    open(&mut app, "main.py", &long);
    let scene = painted(&app);
    let map = targets(&scene)
        .into_iter()
        .find(|t| t.starts_with("code:minimap:"))
        .expect("the minimap is painted");
    assert!(app.drags(&map));
    assert_eq!(app.active_tab().unwrap().scroll, 0);
    // Pressing half way down the map scrolls the editor to that part of the file.
    app.pointer(&map, crate::PointerPhase::Down, 0, 300)
        .unwrap();
    let scrolled = app.active_tab().unwrap().scroll;
    assert!(scrolled > 100, "scrolled to row {scrolled}");
    // Dragging back up moves it again, and the view no longer follows the caret.
    app.pointer(&map, crate::PointerPhase::Move, 0, 40).unwrap();
    assert!(app.active_tab().unwrap().scroll < scrolled);
    assert!(!app.active_tab().unwrap().follow);
}

#[test]
fn right_clicking_opens_the_explorers_and_the_editors_own_menus() {
    let mut app = workspace();
    open(&mut app, "main.py", "def helper():\n    pass\n");
    // The Explorer's menu, on the row that was pressed.
    app.button = 2;
    app.press_at("code:tree:src/app.js", 0, 0).unwrap();
    assert_eq!(app.context, Some(ContextKind::Explorer));
    assert_eq!(app.selected.as_deref(), Some("src/app.js"));
    let scene = painted(&app);
    let shown = targets(&scene);
    for id in [
        "explorer.newFile",
        "renameFile",
        "deleteFile",
        "copyFilePath",
        "revealFileInOS",
        "openInIntegratedTerminal",
    ] {
        assert!(
            shown.iter().any(|t| t == &format!("code:cmd:{id}")),
            "{id} is not in the menu: {shown:?}"
        );
    }
    // Copy Path really copies the machine path.
    let effects = app.click(W, "code:cmd:copyFilePath", 0).unwrap();
    assert!(matches!(&effects[0], AppEffect::CopyText { text, .. }
        if text == "/home/alice/project/src/app.js"));
    assert_eq!(app.context, None, "the menu closes when an entry is chosen");
    // Reveal in the file manager opens it on the containing folder.
    app.button = 2;
    app.press_at("code:tree:src/app.js", 0, 0).unwrap();
    let effects = app.click(W, "code:cmd:revealFileInOS", 0).unwrap();
    assert!(
        matches!(&effects[0], AppEffect::Launch { kind, argument, .. }
        if kind == "files" && argument == "/home/alice/project/src")
    );
    // Open in Integrated Terminal starts a shell in that folder.
    app.button = 2;
    app.press_at("code:tree:src/app.js", 0, 0).unwrap();
    let effects = app
        .click(W, "code:cmd:openInIntegratedTerminal", 0)
        .unwrap();
    assert!(matches!(&effects[0], AppEffect::ShellRun { cwd, .. }
        if cwd == "/home/alice/project/src"));
    assert_eq!(app.terminals[app.term].cwd, "/home/alice/project/src");
    // The editor's own menu, at the caret the right click placed.
    app.button = 2;
    app.press_at("code:editor:0:0:0:0:30", 0, 0).unwrap();
    assert_eq!(app.context, Some(ContextKind::Editor));
    let shown = targets(&painted(&app));
    for id in [
        "editor.action.clipboardCopyAction",
        "editor.action.revealDefinition",
        "workbench.action.showCommands",
    ] {
        assert!(shown.iter().any(|t| t == &format!("code:cmd:{id}")), "{id}");
    }
    // Clicking away closes it.
    app.click(W, "code:menu-close", 0).unwrap();
    assert_eq!(app.context, None);
}

#[test]
fn go_to_definition_finds_where_the_workspace_defines_the_name() {
    let mut app = workspace();
    open(&mut app, "main.py", "from util import helper\n\nhelper()\n");
    // The caret on the call.
    app.active_mut()
        .unwrap()
        .doc
        .set("from util import helper\n\nhel".len(), false);
    let effects = app
        .run_command(W, "editor.action.revealDefinition")
        .unwrap();
    let AppEffect::ReadFiles { tag, paths, .. } = &effects[0] else {
        panic!("it reads the workspace: {effects:?}");
    };
    assert_eq!(tag, "definition:helper");
    assert!(paths.iter().any(|p| p.ends_with("src/util.py")));
    let files = vec![
        (
            "/home/alice/project/README.md".to_owned(),
            Ok("helper\n".to_owned()),
        ),
        (
            "/home/alice/project/src/util.py".to_owned(),
            Ok("import os\n\n\ndef helper():\n    return 1\n".to_owned()),
        ),
    ];
    app.files_read(W, "definition:helper", files);
    // The editor opened the file where it is defined, with the name selected.
    let tab = app.active_tab().unwrap();
    assert_eq!(tab.path, "/home/alice/project/src/util.py");
    assert_eq!(tab.reveal, Some((4, 5, 6)));
    // A name nothing defines says so instead of pretending.
    app.files_read(
        W,
        "definition:nowhere",
        vec![(
            "/home/alice/project/README.md".to_owned(),
            Ok("text\n".to_owned()),
        )],
    );
    assert_eq!(
        app.notice.as_deref(),
        Some("No definition found for 'nowhere'")
    );
    assert_eq!(definition_in("class Widget:\n", "Widget"), Some((1, 7)));
    assert_eq!(definition_in("let total = 1;\n", "total"), Some((1, 5)));
    assert_eq!(definition_in("run() {\n  :\n}\n", "run"), Some((1, 1)));
    assert_eq!(definition_in("print(helper)\n", "helper"), None);
}

#[test]
fn tabs_are_tab_stops_and_whitespace_can_be_seen() {
    assert_eq!(render::columns("\tx", 4), 5);
    assert_eq!(render::columns("ab\tx", 4), 5);
    assert_eq!(render::byte_at_column("\tx", 4, 4), 1);
    assert_eq!(
        render::byte_at_column("\tx", 1, 4),
        0,
        "the near half of a tab"
    );
    let mut app = workspace();
    open(&mut app, "main.py", "def f():\n\treturn 1\n");
    // A click at column 4 of the second line is before the `return`, past the tab.
    let target = "code:editor:0:0:0:0:30";
    let (cw, rh) = render::cell(app.settings.font_size, app.platform);
    app.press_at(target, 4 * cw as i32, rh as i32).unwrap();
    app.click_at(W, target, 4 * cw as i32, rh as i32, 0)
        .unwrap();
    assert_eq!(app.active_tab().unwrap().doc.cursor, "def f():\n\t".len());
    // Whitespace is invisible until it is asked for.
    let shown = |app: &Code| {
        painted(app).nodes.iter().any(|n| {
            matches!(&n.primitive,
            cw_scene::Primitive::Text { text, .. } if text.contains('→'))
        })
    };
    assert!(!shown(&app));
    let effects = app
        .run_command(W, "editor.action.toggleRenderWhitespace")
        .unwrap();
    assert!(app.settings.render_whitespace);
    assert!(shown(&app), "the tab is drawn as an arrow");
    // And it is saved where VS Code saves it.
    let written = effects.iter().find_map(|e| match e {
        AppEffect::WriteFile { content, .. } => Some(content.clone()),
        _ => None,
    });
    assert!(written
        .unwrap()
        .contains("\"editor.renderWhitespace\": \"all\""));
}

#[test]
fn a_preview_tab_title_is_italic_and_a_pinned_one_upright() {
    let mut app = workspace();
    open(&mut app, "main.py", "print('hi')\n");
    let title_style = |app: &Code| {
        let mut p = crate::desktop_scene::Painter::new(1100, 700);
        let env = crate::AppEnv {
            theme: crate::desktop_scene::DesktopTheme::Ubuntu,
            width: 1100,
            height: 700,
            clock_us: 0,
            settings: &crate::SystemSettings::DEFAULT,
            clipboard: None,
            share_to: None,
            files: Default::default(),
            editor: None,
            pointer: None,
        };
        render::render(app, &mut p, &env);
        // The tree row and breadcrumb name the file too; only the tab is italic.
        p.scene
            .nodes
            .iter()
            .filter(|n| n.painted_text() == Some("main.py"))
            .filter_map(|n| n.primitive.text_style())
            .filter(|s| s.italic)
            .count()
    };
    assert!(app.tabs[0].preview);
    assert_eq!(title_style(&app), 1);
    app.activate(W, "code:tab:0", 0).unwrap();
    assert_eq!(title_style(&app), 0);
}
