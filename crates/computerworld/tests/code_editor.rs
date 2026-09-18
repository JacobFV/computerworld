//! Visual Studio Code, driven the way a person drives it: by pointer and keyboard on the
//! rendered screen. Every assertion that matters is about the machine — the file on its
//! disk, the command its shell ran, the commit its git holds — not about the editor's
//! own idea of what happened.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, WorldDefinition};
use serde_json::{json, Value};

const W: u32 = 1280;
const H: u32 = 800;

const MAIN_PY: &str = "import sys\n\n\ndef main():\n    print(\"hello from python\")\n\n\nmain()\n";
const RUN_SH: &str = "#!/bin/bash\necho \"hello from bash\"\n";

fn definition() -> WorldDefinition {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({
        "alice-mac": "virtual-macos-golden-gate",
        "bob-windows": "virtual-windows-11",
        "carol-ubuntu": "virtual-ubuntu-24",
    });
    for computer in &mut d.computers {
        if matches!(
            computer.id.as_str(),
            "alice-mac" | "bob-windows" | "carol-ubuntu"
        ) {
            if !computer.installed_apps.iter().any(|a| a == "code") {
                computer.installed_apps.push("code".into());
            }
            for (path, content) in [
                ("project/main.py", MAIN_PY),
                ("project/run.sh", RUN_SH),
                ("project/README.md", "# Project\n\nTODO: write docs\n"),
                (
                    "project/src/app.js",
                    "// TODO: ship it\nconsole.log('app');\n",
                ),
            ] {
                computer.initial_files.insert(path.into(), content.into());
            }
        }
    }
    d
}
struct Desk {
    world: World,
    actor: String,
    machine: &'static str,
}
impl Desk {
    fn new(machine: &'static str, user: &str) -> Self {
        let mut world = World::new(definition(), 11).unwrap();
        let mut config = EnvironmentConfig::desktop(user, machine);
        config.observations.push("pixels.v1".into());
        let actor = world.environment(config).unwrap();
        Self {
            world,
            actor,
            machine,
        }
    }
    fn try_act(&mut self, family: &str, op: &str, payload: Value) -> Result<Value, String> {
        let result = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, self.machine, payload)],
            )
            .unwrap();
        let outcome = &result.outcomes[0];
        if outcome.success {
            Ok(outcome.value.clone())
        } else {
            Err(format!("{:?}", outcome.error))
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> Value {
        self.try_act(family, op, payload)
            .unwrap_or_else(|e| panic!("{family}.{op} failed: {e}"))
    }
    fn launch(&mut self) {
        self.act("application.v1", "launch", json!({"kind": "code"}));
    }
    fn key(&mut self, key: &str) {
        self.act("keyboard.v1", "key", json!({"key": key}));
    }
    fn typed(&mut self, text: &str) {
        self.act("keyboard.v1", "type", json!({"text": text}));
    }
    /// Screen bounds of the control whose target ends with `target`.
    fn find(&self, target: &str) -> Option<cw_scene::Rect> {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        let suffix = format!(":content:{target}");
        scene
            .nodes
            .iter()
            .rev()
            .find(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|i| i == target || i.ends_with(&suffix))
            })
            .map(|n| n.transform.bounds(n.bounds))
    }
    fn locate(&self, prefix: &str) -> (String, cw_scene::Rect) {
        let scene = self.world.scene(&self.actor, W, H).unwrap();
        let needle = format!(":content:{prefix}");
        scene
            .nodes
            .iter()
            .rev()
            .find_map(|n| {
                let i = n.interaction.as_deref()?;
                let at = i.find(&needle)?;
                Some((i[at + 9..].to_owned(), n.transform.bounds(n.bounds)))
            })
            .unwrap_or_else(|| panic!("no control starting {prefix}"))
    }
    fn click_at(&mut self, x: i32, y: i32) {
        self.act(
            "pointer.v1",
            "click",
            json!({"x": x, "y": y, "width": W, "height": H}),
        );
    }
    fn click(&mut self, target: &str) {
        let r = self
            .find(target)
            .unwrap_or_else(|| panic!("nothing on screen is {target}"));
        self.click_at(r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    }
    fn code(&self) -> Value {
        let session = self.world.interfaces().session(&self.actor).unwrap();
        let desktop = serde_json::to_value(&session.machines[self.machine].desktop).unwrap();
        desktop["windows"]
            .as_object()
            .unwrap()
            .values()
            .find(|w| w["state"]["app"] == "code")
            .expect("a Visual Studio Code window")["state"]
            .clone()
    }
    fn file(&mut self, path: &str) -> String {
        let value = self.act("filesystem.v1", "read", json!({"path": path}));
        value["content"].as_str().unwrap().to_owned()
    }
    fn shell(&mut self, command: &str) -> Value {
        self.act("terminal.v1", "execute", json!({"command": command}))
    }
    fn transcript(&self) -> Vec<Value> {
        let code = self.code();
        let term = code["term"].as_u64().unwrap_or(0) as usize;
        code["terminals"][term]["transcript"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

#[test]
fn opens_the_project_folder_and_a_file_from_the_explorer_by_pointer() {
    let mut d = Desk::new("alice-mac", "alice");
    d.launch();
    let code = d.code();
    assert_eq!(code["folder"], "/Users/alice/project");
    let entries: Vec<&str> = code["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    for want in ["main.py", "run.sh", "README.md", "src/", "src/app.js"] {
        assert!(entries.contains(&want), "{want} not listed: {entries:?}");
    }
    // The title bar's command center names the workspace.
    assert!(d.find("code:cmd:workbench.action.quickOpen").is_some());
    // A single click opens a preview; the text is the file's own.
    d.click("code:tree:main.py");
    let code = d.code();
    assert_eq!(code["tabs"][0]["doc"]["text"], MAIN_PY);
    assert_eq!(code["tabs"][0]["preview"], true);
    // A folder expands in place.
    d.click("code:tree:src");
    assert!(d.find("code:tree:src/app.js").is_some());
    // A double click on the tab keeps it.
    let r = d.find("code:tab:0").unwrap();
    d.act(
        "pointer.v1",
        "double_click",
        json!({"x": r.x + 40, "y": r.y + 10, "width": W, "height": H}),
    );
    assert_eq!(d.code()["tabs"][0]["preview"], false);
}

#[test]
fn typing_and_saving_changes_the_file_on_the_machine() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:README.md");
    // Click into the text, go to the end, type, save with the keyboard.
    let (_, r) = d.locate("code:editor:");
    d.click_at(r.x + 20, r.y + 5);
    d.key("Ctrl+End");
    d.typed("More text.");
    d.key("Enter");
    d.typed("Done");
    assert!(d.code()["tabs"][0]["doc"]["text"]
        .as_str()
        .unwrap()
        .ends_with("More text.\nDone"));
    // Not written yet: the disk still has the old text, and the tab is dirty.
    assert_eq!(
        d.file("/home/carol/project/README.md"),
        "# Project\n\nTODO: write docs\n"
    );
    d.key("Ctrl+s");
    let on_disk = d.file("/home/carol/project/README.md");
    assert_eq!(on_disk, "# Project\n\nTODO: write docs\nMore text.\nDone");
    assert_eq!(d.code()["tabs"][0]["saved"], on_disk);
    // Undo, then save again: the file follows.
    d.key("Ctrl+z");
    d.key("Ctrl+z");
    d.key("Ctrl+s");
    assert!(!d.file("/home/carol/project/README.md").contains("Done"));
}

#[test]
fn a_drag_selects_text_and_copy_paste_use_the_machine_clipboard() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:run.sh");
    let (_, r) = d.locate("code:editor:");
    // Line 2 is `echo "hello from bash"`; drag across its first four characters.
    let (cw, rh) = (9, 19);
    let y = r.y + rh + rh / 2;
    d.act(
        "pointer.v1",
        "down",
        json!({"x": r.x + 1, "y": y, "width": W, "height": H}),
    );
    d.act(
        "pointer.v1",
        "up",
        json!({"x": r.x + 4 * cw + 1, "y": y, "width": W, "height": H}),
    );
    let code = d.code();
    let doc = &code["tabs"][0]["doc"];
    let (a, b) = (
        doc["anchor"].as_u64().unwrap() as usize,
        doc["cursor"].as_u64().unwrap() as usize,
    );
    assert_eq!(&RUN_SH[a.min(b)..a.max(b)], "echo");
    d.key("Ctrl+c");
    let session = d.world.interfaces().session(&d.actor).unwrap();
    assert_eq!(
        session.machines["carol-ubuntu"]
            .desktop
            .clipboard_text
            .as_deref(),
        Some("echo")
    );
    d.key("End");
    d.key("Ctrl+v");
    assert!(d.code()["tabs"][0]["doc"]["text"]
        .as_str()
        .unwrap()
        .contains("\"hello from bash\"echo"));
}

#[test]
fn run_executes_the_file_in_the_integrated_terminal_with_the_machines_shell() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    // A shell script runs with the machine's bash, and its output is real.
    d.click("code:tree:run.sh");
    d.click("code:cmd:workbench.action.terminal.runActiveFile");
    let transcript = d.transcript();
    let last = transcript.last().expect("the run reached the terminal");
    assert_eq!(last["command"], "bash run.sh");
    assert_eq!(last["exit_code"], 0);
    assert_eq!(last["stdout"], "hello from bash\n");
    assert!(last["prompt"]
        .as_str()
        .unwrap()
        .ends_with("/home/carol/project$"));
    // The Python file runs with python3 through F5. Where this machine's shell has no
    // python3 the shell's own `command not found` and 127 are what shows.
    d.click("code:tree:main.py");
    let (_, r) = d.locate("code:editor:");
    d.click_at(r.x + 5, r.y + 5);
    d.key("F5");
    let last = d.transcript().last().cloned().unwrap();
    assert_eq!(last["command"], "python3 main.py");
    match last["exit_code"].as_i64().unwrap() {
        0 => assert_eq!(last["stdout"], "hello from python\n"),
        127 => assert!(
            last["stderr"].as_str().unwrap().contains("not found"),
            "{last}"
        ),
        other => panic!("python3 exited {other}: {last}"),
    }
    let code = d.code();
    assert_eq!(code["last_run"][0], "python3 main.py");
    assert_eq!(code["panel_open"], true);
    // The session's `cd` moved the session, not the machine's shell.
    d.click("code:terminal");
    d.typed("cd src && pwd");
    d.key("Enter");
    let last = d.transcript().last().cloned().unwrap();
    assert_eq!(last["stdout"], "/home/carol/project/src\n");
    assert_eq!(d.shell("pwd")["stdout"], "/home/carol\n");
    d.typed("ls");
    d.key("Enter");
    assert_eq!(d.transcript().last().unwrap()["stdout"], "app.js\n");
}

#[test]
fn a_python_traceback_becomes_a_problem_that_opens_its_line() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:main.py");
    let has_python = d.shell("python3 -c \"print(1)\"")["exit_code"] == 0;
    if !has_python {
        // Without an interpreter on this machine there is no traceback to parse; the
        // Problems panel must stay empty rather than invent one.
        d.key("F5");
        assert_eq!(d.code()["problems"].as_array().unwrap().len(), 0);
        return;
    }
    let (_, r) = d.locate("code:editor:");
    d.click_at(r.x + 5, r.y + 5);
    d.key("Ctrl+End");
    d.typed("undefined_name\n");
    d.key("F5");
    let code = d.code();
    let problems = code["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1, "{code}");
    assert_eq!(problems[0]["line"], 9);
    assert!(problems[0]["message"]
        .as_str()
        .unwrap()
        .contains("NameError"));
    d.click("code:panel:problems");
    d.click("code:problem:0");
    assert_eq!(d.code()["tabs"][0]["doc"]["cursor"], MAIN_PY.len());
}

#[test]
fn search_across_the_workspace_and_open_a_result() {
    let mut d = Desk::new("bob-windows", "bob");
    d.launch();
    d.click("code:activity:search");
    d.typed("todo");
    let code = d.code();
    let results = code["search"]["results"].as_array().unwrap();
    let paths: Vec<&str> = results
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["README.md", "src/app.js"]);
    // Match Case: the files say TODO, not todo.
    d.click("code:search:case");
    assert_eq!(d.code()["search"]["results"].as_array().unwrap().len(), 0);
    d.click("code:search:regex");
    d.key("Backspace");
    d.key("Backspace");
    d.key("Backspace");
    d.key("Backspace");
    d.typed("TODO: (write|ship)");
    assert_eq!(d.code()["search"]["results"].as_array().unwrap().len(), 2);
    d.click("code:search-result:1:0");
    let code = d.code();
    let active = code["active"].as_u64().unwrap() as usize;
    assert!(code["tabs"][active]["path"]
        .as_str()
        .unwrap()
        .ends_with("/project/src/app.js"));
    // The match is selected in the editor it opened; a second result in the same, now
    // loaded, file selects that one.
    let doc = &code["tabs"][active]["doc"];
    assert_ne!(doc["cursor"], doc["anchor"], "{doc}");
    d.click("code:search-result:0:0");
    let code = d.code();
    let active = code["active"].as_u64().unwrap() as usize;
    let doc = &code["tabs"][active]["doc"];
    let text = doc["text"].as_str().unwrap();
    let (a, b) = (
        doc["anchor"].as_u64().unwrap() as usize,
        doc["cursor"].as_u64().unwrap() as usize,
    );
    assert_eq!(&text[a.min(b)..a.max(b)], "TODO: write");
}

#[test]
fn the_command_palette_and_quick_open_run_real_commands() {
    let mut d = Desk::new("bob-windows", "bob");
    d.launch();
    d.key("Ctrl+Shift+P");
    d.typed("toggle terminal");
    d.key("Enter");
    let code = d.code();
    assert_eq!(code["panel_open"], true);
    assert_eq!(code["panel"], "terminal");
    // PowerShell's prompt in the workspace folder.
    assert_eq!(
        code["terminals"][0]["prompt"],
        "PS C:\\Users\\bob\\project>"
    );
    // Quick Open by clicking the command center in the title bar.
    d.click("code:cmd:workbench.action.quickOpen");
    d.typed("apjs");
    d.key("Enter");
    let code = d.code();
    let active = code["active"].as_u64().unwrap() as usize;
    assert!(code["tabs"][active]["path"]
        .as_str()
        .unwrap()
        .ends_with("src/app.js"));
    // The File menu drops from the title bar and its entries run.
    d.click("code:menu:file");
    assert_eq!(d.code()["menu"], "file");
    d.click("code:cmd:workbench.action.files.newUntitledFile");
    let code = d.code();
    let active = code["active"].as_u64().unwrap() as usize;
    assert_eq!(code["tabs"][active]["path"], "Untitled-1");
    assert_eq!(code["menu"], Value::Null);
}

#[test]
fn explorer_file_operations_act_on_the_disk() {
    let mut d = Desk::new("alice-mac", "alice");
    d.launch();
    d.click("code:cmd:explorer.newFile");
    d.typed("notes.txt");
    d.key("Enter");
    assert_eq!(d.file("/Users/alice/project/notes.txt"), "");
    let code = d.code();
    assert!(code["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e == "notes.txt"));
    // The new file opened; type into it and save.
    d.typed("remember");
    d.key("Meta+s");
    assert_eq!(d.file("/Users/alice/project/notes.txt"), "remember");
    // Rename with F2 in the explorer.
    d.click("code:tree:notes.txt");
    d.key("F2");
    for _ in 0.."notes.txt".len() {
        d.key("Backspace");
    }
    d.typed("todo.txt");
    d.key("Enter");
    assert_eq!(d.file("/Users/alice/project/todo.txt"), "remember");
    assert!(d
        .try_act(
            "filesystem.v1",
            "read",
            json!({"path": "/Users/alice/project/notes.txt"})
        )
        .is_err());
    // Delete moves it to the trash, after asking.
    d.click("code:tree:todo.txt");
    d.key("Delete");
    d.click("code:dialog:0");
    assert!(d
        .try_act(
            "filesystem.v1",
            "read",
            json!({"path": "/Users/alice/project/todo.txt"})
        )
        .is_err());
    let trash = d.shell("ls /Users/alice/.local/share/Trash/files");
    assert!(trash["stdout"].as_str().unwrap().contains("todo.txt"));
}

#[test]
fn source_control_initialises_stages_and_commits_with_the_machines_git() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:activity:scm");
    assert_eq!(d.code()["scm"]["repo"], false);
    d.click("code:cmd:git.init");
    let code = d.code();
    assert_eq!(code["scm"]["repo"], true);
    assert_eq!(code["scm"]["branch"], "main");
    let changes = code["scm"]["changes"].as_array().unwrap().len();
    assert_eq!(changes, 4, "{}", code["scm"]);
    d.click("code:scm-stage:README.md");
    assert_eq!(d.code()["scm"]["staged"].as_array().unwrap().len(), 1);
    d.click("code:scm-message");
    d.typed("Add the readme");
    d.key("Ctrl+Enter");
    let log = d.shell("git -C /home/carol/project log");
    assert!(
        log["stdout"].as_str().unwrap().contains("Add the readme"),
        "{log}"
    );
    let code = d.code();
    assert_eq!(code["scm"]["staged"].as_array().unwrap().len(), 0);
    assert_eq!(code["scm"]["changes"].as_array().unwrap().len(), 3);
    assert_eq!(code["scm"]["message"], "");
    // Editing a committed file shows up as a change once it is saved.
    d.click("code:activity:explorer");
    d.click("code:tree:README.md");
    d.typed("x");
    d.key("Ctrl+s");
    let code = d.code();
    assert!(code["scm"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c[0] == "M" && c[1] == "README.md"));
}

#[test]
fn source_control_unstages_and_discards_through_the_machines_git() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:activity:scm");
    d.click("code:cmd:git.init");
    d.click("code:cmd:git.stageAll");
    d.click("code:scm-message");
    d.typed("First");
    d.key("Ctrl+Enter");
    assert_eq!(d.code()["scm"]["changes"].as_array().unwrap().len(), 0);
    // Change one file and add another, then stage both.
    d.act(
        "filesystem.v1",
        "write",
        json!({"path": "/home/carol/project/README.md",
                                           "content": "# Project\n\nchanged\n"}),
    );
    d.act(
        "filesystem.v1",
        "write",
        json!({"path": "/home/carol/project/NOTES.md",
                                           "content": "notes\n"}),
    );
    d.click("code:cmd:git.refresh");
    d.click("code:cmd:git.stageAll");
    let code = d.code();
    assert_eq!(
        code["scm"]["staged"].as_array().unwrap().len(),
        2,
        "{}",
        code["scm"]
    );
    // Unstage one: it goes back to Changes, and the file keeps its new text.
    d.click("code:scm-unstage:README.md");
    let code = d.code();
    let staged: Vec<String> = code["scm"]["staged"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c[1].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(staged, ["NOTES.md"]);
    assert!(code["scm"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c[0] == "M" && c[1] == "README.md"));
    assert!(d.file("/home/carol/project/README.md").contains("changed"));
    // Unstage the rest, then discard the change: the machine's file comes back.
    d.click("code:cmd:git.unstageAll");
    assert_eq!(d.code()["scm"]["staged"].as_array().unwrap().len(), 0);
    d.click("code:scm-discard:README.md");
    assert!(!d.code()["dialog"].is_null(), "discarding asks first");
    d.click("code:dialog:1");
    assert!(
        d.file("/home/carol/project/README.md").contains("changed"),
        "Cancel kept it"
    );
    d.click("code:scm-discard:README.md");
    d.click("code:dialog:0");
    assert_eq!(
        d.file("/home/carol/project/README.md"),
        "# Project\n\nTODO: write docs\n"
    );
    // An untracked file is deleted instead, and says so before it is.
    d.click("code:scm-discard:NOTES.md");
    let dialog = d.code()["dialog"].clone();
    assert!(
        dialog["message"]
            .as_str()
            .unwrap()
            .contains("delete NOTES.md"),
        "{dialog}"
    );
    d.click("code:dialog:0");
    assert!(d
        .try_act(
            "filesystem.v1",
            "read",
            json!({"path": "/home/carol/project/NOTES.md"})
        )
        .is_err());
    assert_eq!(d.code()["scm"]["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn settings_persist_to_the_users_settings_json() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.key("Ctrl+,");
    d.click("code:settings:theme:light");
    let saved = d.file("/home/carol/.config/Code/User/settings.json");
    assert!(saved.contains("Default Light Modern"), "{saved}");
    assert_eq!(d.code()["settings"]["dark"], false);
    // A new window reads them back.
    d.act(
        "application.v1",
        "launch",
        json!({"kind": "code", "argument": "/home/carol/project"}),
    );
    let session = d.world.interfaces().session(&d.actor).unwrap();
    let desktop = serde_json::to_value(&session.machines["carol-ubuntu"].desktop).unwrap();
    let light = desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .filter(|w| w["state"]["app"] == "code")
        .all(|w| w["state"]["settings"]["dark"] == false);
    assert!(light);
}

#[test]
fn the_window_chrome_is_visual_studio_codes_on_every_desktop() {
    for (machine, user) in [
        ("alice-mac", "alice"),
        ("bob-windows", "bob"),
        ("carol-ubuntu", "carol"),
    ] {
        let mut d = Desk::new(machine, user);
        d.launch();
        // Window buttons, the command center and the layout toggles are all live.
        for target in [
            "close",
            "minimize",
            "maximize",
            "code:cmd:workbench.action.quickOpen",
            "code:cmd:workbench.action.togglePanel",
            "code:activity:explorer",
            "code:status:problems",
        ] {
            let scene = d.world.scene(&d.actor, W, H).unwrap();
            assert!(
                scene.nodes.iter().any(|n| n
                    .interaction
                    .as_deref()
                    .is_some_and(|i| i.ends_with(target))),
                "{machine}: {target}"
            );
        }
        let menu = d.find("code:menu:file").is_some();
        assert_eq!(
            menu,
            machine != "alice-mac",
            "{machine}: menu bar in the title bar"
        );
        d.click("code:cmd:workbench.action.togglePanel");
        assert_eq!(d.code()["panel_open"], true);
        // The whole state survives a snapshot.
        let snapshot = d.world.snapshot();
        let mut restored = World::new(definition(), 11).unwrap();
        restored.restore(&snapshot).unwrap();
        assert_eq!(
            restored.state_hash().unwrap(),
            d.world.state_hash().unwrap(),
            "{machine}"
        );
    }
}

#[test]
fn open_folder_browses_the_machine_and_opens_what_is_chosen() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.shell("mkdir -p /home/carol/other/deep && echo hi > /home/carol/other/notes.txt");
    d.launch();
    d.key("Ctrl+k");
    d.key("Ctrl+o");
    let code = d.code();
    assert_eq!(code["quick"]["mode"], "open_folder");
    assert_eq!(code["quick"]["value"], "/home/carol/");
    // The dialog lists the folders really there.
    assert!(d.find("code:quick:0").is_some());
    let names: Vec<&str> = code["browse_entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"other/") && names.contains(&"project/"),
        "{names:?}"
    );
    // Typing narrows it and Tab completes, as the simple file dialog does.
    d.typed("oth");
    d.key("Tab");
    assert_eq!(d.code()["quick"]["value"], "/home/carol/other/");
    d.click("code:quick-ok");
    let code = d.code();
    assert_eq!(code["folder"], "/home/carol/other");
    let entries: Vec<&str> = code["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    assert_eq!(entries, vec!["deep/", "notes.txt"]);
    // Closing the folder leaves the Welcome page, whose Open Folder is live.
    d.key("Ctrl+k");
    d.typed("f");
    assert_eq!(d.code()["folder"], Value::Null);
    assert!(d
        .find("code:cmd:workbench.action.files.openFolder")
        .is_some());
}

#[test]
fn splitting_the_editor_paints_two_groups_of_one_document() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:main.py");
    // Split from the command palette, as a person would.
    d.key("Ctrl+Shift+P");
    d.typed("split editor right");
    d.key("Enter");
    // Two editors are on screen, side by side, and the right one has the focus.
    let scene = d.world.scene(&d.actor, W, H).unwrap();
    let mut editors: Vec<(String, cw_scene::Rect)> = scene
        .nodes
        .iter()
        .filter_map(|n| {
            let i = n.interaction.as_deref()?;
            let at = i.find(":content:code:editor:")?;
            Some((i[at + 9..].to_owned(), n.transform.bounds(n.bounds)))
        })
        .collect();
    editors.sort_by_key(|(_, r)| r.x);
    assert_eq!(editors.len(), 2, "{editors:?}");
    assert!(editors[0].1.x + editors[0].1.width as i32 <= editors[1].1.x);
    assert!(editors[0].0.starts_with("code:editor:0:"));
    assert!(editors[1].0.starts_with("code:editor:1:"));
    let code = d.code();
    assert_eq!(code["focus_group"], 1);
    assert_eq!(code["tabs"].as_array().unwrap().len(), 2);
    // Typing in the right-hand view changes the left-hand one too: it is one file.
    let r = editors[1].1;
    d.click_at(r.x + 1, r.y + 1);
    d.typed("# split");
    d.key("Enter");
    let code = d.code();
    for tab in code["tabs"].as_array().unwrap() {
        assert!(tab["doc"]["text"].as_str().unwrap().starts_with("# split"));
    }
    // Saving writes that one file once.
    d.key("Ctrl+s");
    assert!(d
        .file("/home/carol/project/main.py")
        .starts_with("# split\nimport sys"));
    // Clicking in the left-hand view moves the focus back to its group.
    let r = editors[0].1;
    d.click_at(r.x + 1, r.y + 1);
    assert_eq!(d.code()["focus_group"], 0);
}

#[test]
fn several_cursors_change_every_occurrence_at_once() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:main.py");
    let (_, r) = d.locate("code:editor:");
    // The caret goes into `main` on the `def main():` line (the fourth row).
    let (cw, rh) = (9, 19);
    d.click_at(r.x + 5 * cw + 1, r.y + 3 * rh + rh / 2);
    // Ctrl+D selects the word, again adds the call below it.
    d.key("Ctrl+d");
    d.key("Ctrl+d");
    let carets = d.code()["tabs"][0]["doc"]["carets"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(carets, 1, "the primary caret plus one more");
    d.typed("start");
    let text = d.code()["tabs"][0]["doc"]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.contains("def start():"), "{text}");
    assert!(text.contains("\nstart()\n"), "{text}");
    assert!(!text.contains("main"), "{text}");
    // One undo takes back both edits, as VS Code does.
    d.key("Ctrl+z");
    assert_eq!(d.code()["tabs"][0]["doc"]["text"], MAIN_PY);
    d.key("Ctrl+Shift+Z");
    d.key("Ctrl+s");
    let on_disk = d.file("/home/carol/project/main.py");
    assert!(on_disk.contains("def start():") && on_disk.contains("\nstart()\n"));
}

#[test]
fn right_clicking_the_explorer_opens_its_menu_and_its_entries_act() {
    let mut d = Desk::new("carol-ubuntu", "carol");
    d.launch();
    d.click("code:tree:src");
    let r = d.find("code:tree:src/app.js").expect("the file is listed");
    let (x, y) = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    for op in ["down", "up"] {
        d.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H, "button": 2}),
        );
    }
    let code = d.code();
    assert_eq!(code["context"], "explorer");
    assert_eq!(code["selected"], "src/app.js");
    assert!(d.find("code:cmd:copyFilePath").is_some());
    assert!(d.find("code:cmd:openInIntegratedTerminal").is_some());
    // Copy Path puts the machine's own path on the machine's clipboard.
    d.click("code:cmd:copyFilePath");
    let session = d.world.interfaces().session(&d.actor).unwrap();
    assert_eq!(
        session.machines["carol-ubuntu"]
            .desktop
            .clipboard_text
            .as_deref(),
        Some("/home/carol/project/src/app.js")
    );
    assert_eq!(d.code()["context"], Value::Null);
    // Open in Integrated Terminal really runs the machine's shell in that folder.
    for op in ["down", "up"] {
        d.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": W, "height": H, "button": 2}),
        );
    }
    d.click("code:cmd:openInIntegratedTerminal");
    d.typed("pwd");
    d.key("Enter");
    let out = d.transcript();
    let last = out.last().expect("the shell answered");
    assert_eq!(last["command"], "pwd");
    assert_eq!(last["stdout"], "/home/carol/project/src\n");
}
