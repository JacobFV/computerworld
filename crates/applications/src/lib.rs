//! Serializable desktop applications. Effects are requests to the environment,
//! never ambient filesystem access or subprocess execution.
pub mod apps;
pub mod cursor;
pub mod desktop_scene;
mod effects;
mod files;
mod input;
mod registered;
mod state;
mod terminal;
pub mod web_app;
pub use apps::{AppEnv, FilesEnv, NativeApp};
pub use cursor::CursorKind;
pub use desktop_scene::scroll::{Scroll, ScrollBar};
pub use effects::*;
use files::free_name;
pub use files::*;
pub use input::*;
use input::{parent_folder, TERMINAL_LINE};
pub use registered::*;
pub use state::*;
pub use terminal::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-tab back/forward depth, and file manager tabs per window.
const HISTORY_LIMIT: usize = 64;
/// Retained shell collections. A long session must not grow a snapshot without limit.
const BOOKMARK_LIMIT: usize = 64;
const DOWNLOAD_LIMIT: usize = 64;
const NOTICE_LIMIT: usize = 64;
/// Virtual desktops. More than this is a filing system, not a workspace switcher.
const WORKSPACE_LIMIT: u32 = 8;
const TAB_LIMIT: usize = 16;
/// Terminal scrollback bounds. A session runs as long as the actor likes; the snapshot
/// must not grow with it, so the transcript keeps the newest entries and truncates
/// oversized streams instead of holding everything a command ever printed.
const TRANSCRIPT_LIMIT: usize = 200;
const STREAM_LIMIT: usize = 4096;
/// Everything a file manager retains grows with use, so each of these is capped the
/// way the transcript is: a search query, a rename buffer, the clipboard and the
/// recent-files list must never make a snapshot a function of session length.
const FIELD_LIMIT: usize = 64;
const CLIPBOARD_LIMIT: usize = 32;
const RECENT_LIMIT: usize = 16;
const STAR_LIMIT: usize = 64;
/// The folders a desktop session gives a user's home on first login, per platform:
/// what `xdg-user-dirs-update` creates on Ubuntu, what a new macOS account and a new
/// Windows profile hold. Names only; a file manager lists the ones that really exist.
pub fn standard_folders(theme: desktop_scene::DesktopTheme) -> &'static [&'static str] {
    use desktop_scene::DesktopTheme::*;
    match theme {
        Ubuntu => &[
            "Desktop",
            "Documents",
            "Downloads",
            "Music",
            "Pictures",
            "Public",
            "Templates",
            "Videos",
        ],
        Macos => &[
            "Desktop",
            "Documents",
            "Downloads",
            "Movies",
            "Music",
            "Pictures",
            "Public",
        ],
        Windows => &[
            "Desktop",
            "Documents",
            "Downloads",
            "Music",
            "Pictures",
            "Videos",
        ],
        Ios | Android => &[],
    }
}
/// Explorer's pinned Quick access folders, in the order its Home page shows them.
pub const QUICK_ACCESS: [&str; 6] = [
    "Desktop",
    "Downloads",
    "Documents",
    "Pictures",
    "Music",
    "Videos",
];
/// What Explorer's Gallery collects: image files, by extension, the way it decides.
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
/// `pointer.v1`'s `modifiers` names as bits: `ctrl`, `alt` (Option), `shift`, `meta`
/// (Command). Unknown names are refused.
pub fn modifier_bits(names: &[&str]) -> Result<u8, String> {
    names.iter().try_fold(0u8, |bits, name| {
        Ok(bits
            | match name.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => apps::imaging::MOD_CTRL,
                "alt" | "option" => apps::imaging::MOD_ALT,
                "shift" => apps::imaging::MOD_SHIFT,
                "meta" | "cmd" | "command" | "super" => apps::imaging::MOD_META,
                other => return Err(format!("unknown modifier {other}")),
            })
    })
}
pub fn is_image(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".heic", ".tif", ".tiff",
    ]
    .iter()
    .any(|ext| name.ends_with(ext))
}
/// Lines a terminal may be scrolled back by. The view clamps to the output it actually
/// has; this only stops a stored offset growing without bound.
const SCROLL_LIMIT: usize = 4096;
/// Text the clipboard holds. A copy of a whole large file is refused, not truncated.
const TEXT_CLIPBOARD_LIMIT: usize = 1 << 20;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documents_open_in_the_application_that_owns_their_kind() {
        assert_eq!(opener("Budget.xlsx"), "spreadsheet");
        assert_eq!(opener("Inventory.db"), "database");
        assert_eq!(opener("Documents/Parts/bracket.FCStd.json"), "freecad");
        assert_eq!(opener("bracket.STEP"), "freecad");
        assert_eq!(opener("mesh.stl"), "freecad");
        assert_eq!(
            opener("Documents/KiCad/sensor-node/sensor-node.kicad_pro"),
            "kicad"
        );
        assert_eq!(opener("sensor-node.kicad_sch"), "kicad");
        assert_eq!(opener("sensor-node.kicad_pcb"), "kicad");
        assert_eq!(opener("notes.txt"), "editor");
        assert_eq!(opener("Parts/"), "editor");
    }
    #[test]
    fn soft_wrapped_rows_break_after_a_space_and_clicks_land_on_the_row_painted() {
        let text = "the quick brown fox jumps\nshort";
        let rows = editor_rows(text, 10);
        let painted: Vec<&str> = rows.iter().map(|(a, b)| &text[*a..*b]).collect();
        assert_eq!(painted, ["the quick ", "brown fox ", "jumps", "short"]);
        // A word longer than the row breaks at the edge.
        let long = editor_rows("abcdefghijkl", 5);
        assert_eq!(long, vec![(0, 5), (5, 10), (10, 12)]);
        // Unwrapped, a row is a line.
        assert_eq!(editor_rows(text, 0).len(), 2);
        // Row 1, column 2 is the "o" of "brown".
        let at = caret_for_point_wrapped(text, 0, 10, 2 * 8, 18);
        assert_eq!(&text[at..at + 1], "o");
        // The same point with a scrolled view one row down is on row 2.
        let at = caret_for_point_wrapped(text, 1, 10, 0, 18);
        assert_eq!(&text[at..], "jumps\nshort");
        // The caret at a soft wrap sits at the start of the next row.
        assert_eq!(editor_caret_cell(text, 10, 10), (1, 0));
        // At the end of a hard line it stays on that line.
        assert_eq!(editor_caret_cell(text, 25, 10), (2, 5));
        assert_eq!(editor_caret_cell(text, text.len(), 10), (3, 5));
        // Clicking past the end of a soft-wrapped row stays on that row.
        let at = caret_for_point_wrapped(text, 0, 10, 40 * 8, 0);
        assert_eq!(editor_caret_cell(text, at, 10).0, 0);
    }
    #[test]
    fn up_and_down_keep_the_column() {
        let mut d = DesktopState::default();
        let (window, _) = d.launch("editor", "").unwrap();
        d.focus(window).unwrap();
        d.text("abcdef\nxy\nlonger line").unwrap();
        d.key("ArrowUp").unwrap();
        let cursor = |d: &DesktopState| match &d.windows[&window].state {
            AppState::Editor { cursor, .. } => *cursor,
            _ => unreachable!(),
        };
        // From column 11 of line 3 to the end of the two-character line 2.
        assert_eq!(cursor(&d), "abcdef\nxy".len());
        d.key("ArrowUp").unwrap();
        assert_eq!(cursor(&d), 2);
        d.key("ArrowDown").unwrap();
        assert_eq!(cursor(&d), "abcdef\nxy".len());
    }
    #[test]
    fn unicode_editor_effects_and_focus() {
        let mut d = DesktopState::default();
        let (editor, _) = d.launch("editor", "/note").unwrap();
        d.file_loaded(editor, "café".into()).unwrap();
        d.key("Backspace").unwrap();
        d.text("ø").unwrap();
        assert_eq!(
            d.key("Ctrl+s").unwrap(),
            vec![AppEffect::WriteFile {
                window: editor,
                path: "/note".into(),
                content: "cafø".into()
            }]
        );
        let (terminal, _) = d.launch("terminal", "").unwrap();
        d.text("pwd").unwrap();
        assert_eq!(
            d.key("Enter").unwrap(),
            vec![AppEffect::Execute {
                window: terminal,
                command: "pwd".into()
            }]
        );
        d.close(terminal).unwrap();
        assert_eq!(d.focused, Some(editor));
    }
    #[test]
    fn terminal_transcript_is_legible_bounded_and_clearable() {
        assert_eq!(
            shell_prompt("alice", "workstation", "/home/alice", "posix"),
            "alice@workstation:/home/alice$"
        );
        assert_eq!(
            shell_prompt("alice", "win", "/C:/Users/alice", "powershell"),
            r"PS C:\Users\alice>"
        );
        // bash abbreviates the home folder, and only the home folder.
        for (cwd, want) in [
            ("/home/alice", "alice@box:~$"),
            ("/home/alice/src", "alice@box:~/src$"),
            ("/home/alicea", "alice@box:/home/alicea$"),
            ("/tmp", "alice@box:/tmp$"),
        ] {
            assert_eq!(
                shell_prompt_at("alice", "box", cwd, "/home/alice", "posix"),
                want
            );
        }
        for (cwd, want) in [
            ("/Users/alice", "alice@mac ~ %"),
            ("/Users/alice/src", "alice@mac src %"),
            ("/tmp", "alice@mac tmp %"),
            ("/", "alice@mac / %"),
        ] {
            assert_eq!(
                shell_prompt_at("alice", "mac", cwd, "/Users/alice", "zsh"),
                want
            );
        }
        assert_eq!(
            shell_prompt_at("bob", "win", "/C:/Users/bob", "/C:/Users/bob", "powershell"),
            r"PS C:\Users\bob>"
        );
        let mut d = DesktopState {
            prompt: "alice@box:/home/alice$".into(),
            ..DesktopState::default()
        };
        let (id, _) = d.launch("terminal", "").unwrap();
        d.terminal_output(
            id,
            TerminalEntry::new("alice@box:/home/alice$", "cd /tmp", "", "", 0),
        )
        .unwrap();
        // The machine moved, so the pending prompt moves with it.
        d.prompt = "alice@box:/tmp$".into();
        d.terminal_output(
            id,
            TerminalEntry::new("alice@box:/tmp$", "nope", "", "nope: not found", 127),
        )
        .unwrap();
        let page = d.page();
        let ids: Vec<&str> = page.elements.iter().map(element_id).collect();
        assert!(ids.contains(&"terminal-entry:1"));
        assert!(matches!(
            page.elements.iter().find(|e| element_id(e) == "terminal-entry:1"),
            Some(cw_protocol::PageElement::Group { children, .. })
                if children.iter().any(|c| matches!(c, cw_protocol::PageElement::Text { text, .. } if text == "[exit 127]"))
        ));
        assert!(matches!(
            page.elements.last(),
            Some(cw_protocol::PageElement::Input { label, .. }) if label == "alice@box:/tmp$"
        ));
        // Recall history survives a clear; only the frame goes.
        d.text("echo hi").unwrap();
        d.key("Enter").unwrap();
        d.terminal_clear(id).unwrap();
        match &d.windows[&id].state {
            AppState::Terminal {
                transcript,
                history,
                ..
            } => {
                assert!(transcript.is_empty());
                assert_eq!(history, &vec!["echo hi".to_string()]);
            }
            _ => panic!("not a terminal"),
        }
        for i in 0..TRANSCRIPT_LIMIT + 10 {
            d.terminal_output(id, TerminalEntry::new("$", format!("echo {i}"), "", "", 0))
                .unwrap();
        }
        match &d.windows[&id].state {
            AppState::Terminal { transcript, .. } => {
                assert_eq!(transcript.len(), TRANSCRIPT_LIMIT);
                assert_eq!(transcript[0].command, "echo 10");
                // Oversized streams are cut with a stated byte count, never silently.
                let long = TerminalEntry::new("$", "yes", &"x".repeat(STREAM_LIMIT + 100), "", 0);
                assert!(long.stdout.ends_with("[… 100 bytes truncated]"));
            }
            _ => panic!("not a terminal"),
        }
    }
    fn element_id(e: &cw_protocol::PageElement) -> &str {
        use cw_protocol::PageElement as E;
        match e {
            E::Text { id, .. }
            | E::Group { id, .. }
            | E::Input { id, .. }
            | E::Button { id, .. }
            | E::Heading { id, .. } => id,
            _ => "",
        }
    }
    #[test]
    fn snapshot_restores_cursor_and_pending_edit() {
        let mut d = DesktopState::default();
        d.launch("editor", "/note").unwrap();
        d.text("😀hello").unwrap();
        d.key("Home").unwrap();
        d.key("Delete").unwrap();
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;
    struct Counter;
    impl cw_sdk::Application for Counter {
        fn kind(&self) -> &str {
            "counter"
        }
        fn event(
            &self,
            state: &mut serde_json::Value,
            _: &cw_sdk::AppContext,
            event: &cw_sdk::AppEvent,
        ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
            *state = serde_json::json!(state.as_u64().unwrap_or(0) + 1);
            if event.kind == "fail" {
                return Err(cw_protocol::SimError::invalid("rejected"));
            }
            Ok(vec![])
        }
        fn page(
            &self,
            state: &serde_json::Value,
            _: &cw_sdk::AppContext,
        ) -> cw_protocol::Result<cw_protocol::Page> {
            Ok(cw_protocol::Page::new(format!("Count {state}")))
        }
    }
    #[test]
    fn custom_application_state_and_failed_event_atomicity() {
        let mut registry = cw_sdk::Registry::new();
        registry.register_application(Counter).unwrap();
        let mut apps = RegisteredApplications::default();
        let ctx = cw_sdk::AppContext {
            actor: "a".into(),
            machine: "m".into(),
            tick: 0,
            seed: 1,
            instance: "counter1".into(),
        };
        apps.launch(&registry, "counter", "counter1", serde_json::json!(0), &ctx)
            .unwrap();
        apps.event(
            &registry,
            "counter1",
            &ctx,
            &cw_sdk::AppEvent {
                kind: "increment".into(),
                target: None,
                data: serde_json::Value::Null,
            },
        )
        .unwrap();
        assert_eq!(
            apps.page(&registry, "counter1", &ctx).unwrap().title,
            "Count 1"
        );
        assert!(apps
            .event(
                &registry,
                "counter1",
                &ctx,
                &cw_sdk::AppEvent {
                    kind: "fail".into(),
                    target: None,
                    data: serde_json::Value::Null
                }
            )
            .is_err());
        assert_eq!(apps.instances["counter1"].state, serde_json::json!(1));
    }
}

#[cfg(test)]
mod file_manager_tests {
    use super::*;
    fn files(entries: &[&str]) -> DesktopState {
        let mut d = DesktopState {
            home: "/home/alice".into(),
            ..DesktopState::default()
        };
        let (id, _) = d.launch("files", "/work").unwrap();
        d.directory_loaded(id, 0, entries.iter().map(|e| (*e).to_owned()).collect())
            .unwrap();
        d
    }
    fn tab(d: &DesktopState) -> &FileTab {
        d.focused_tab().unwrap()
    }
    /// Sorting reorders the screen, so it has to reorder the click targets with it.
    /// Selection stays pinned to the file, not to the row it happened to be on.
    #[test]
    fn a_file_manager_opens_in_the_platforms_view_and_new_tabs_keep_it() {
        let mut d = DesktopState {
            file_view: FileView::Grid,
            ..Default::default()
        };
        d.launch("files", "/work").unwrap();
        assert_eq!(d.focused_tab().unwrap().view, FileView::Grid);
        d.click("files-newtab").unwrap();
        let AppState::Files { tabs, .. } = &d.windows[&0].state else {
            panic!("a file manager");
        };
        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().all(|t| t.view == FileView::Grid));
    }
    #[test]
    fn display_order_drives_click_targets_and_selection_follows_the_file() {
        let mut d = files(&["b.txt", "a/", "c.txt"]);
        // The raw listing is asciibetical; the default view sorts by name.
        assert_eq!(tab(&d).entries, vec!["a/", "b.txt", "c.txt"]);
        d.click("open:1").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "b.txt");
        // Folders first, then the same file is on a different row.
        d.click("files-sort:kind").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "b.txt");
        assert_eq!(tab(&d).row_of(1), Some(1));
        d.click("files-sort:kind").unwrap();
        assert!(tab(&d).descending);
        assert_eq!(tab(&d).row_of(1), Some(1));
        // Reversed by name: row 0 is the last entry of the listing.
        d.click("files-sort:name").unwrap();
        d.click("files-sort:name").unwrap();
        d.click("open:0").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "c.txt");
        // A row past the end of the displayed list is refused, not clamped.
        assert!(d.click("open:3").is_err());
    }
    #[test]
    fn search_filters_the_rows_and_renumbers_them() {
        let mut d = files(&["alpha.txt", "beta.txt", "gamma.txt"]);
        d.click("files-search").unwrap();
        d.text("MA").unwrap();
        // Case-insensitive substring, and the surviving row is row 0.
        assert_eq!(tab(&d).display(), vec![2]);
        d.click("open:0").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "gamma.txt");
        // Escape cancels the filter outright; a hidden filter would be a lie.
        d.key("Escape").unwrap();
        assert!(tab(&d).query.is_empty());
        assert_eq!(tab(&d).display().len(), 3);
        // The field is bounded, and so is the rename buffer.
        d.click("files-search").unwrap();
        d.text(&"x".repeat(FIELD_LIMIT)).unwrap();
        assert!(d.text("y").is_err());
        assert_eq!(tab(&d).query.chars().count(), FIELD_LIMIT);
    }
    #[test]
    fn navigating_leaves_every_field_and_the_recents_scope() {
        let mut d = files(&["a.txt"]);
        d.recents = vec!["/work/a.txt".into()];
        d.click("files-recents").unwrap();
        assert_eq!(tab(&d).scope, FileScope::Recents);
        // A listing for the folder the tab left does not overwrite Recents.
        let id = d.focused.unwrap();
        d.directory_loaded(id, 0, vec!["stale.txt".into()]).unwrap();
        assert_eq!(tab(&d).entries, vec!["/work/a.txt"]);
        // Opening a recent reaches the absolute path, not a child of the folder.
        d.click("open:0").unwrap();
        let effects = d.activate("open:0").unwrap();
        assert!(matches!(
            effects.first(),
            Some(AppEffect::ReadFile { path, .. }) if path == "/work/a.txt"
        ));
        // Mutations are refused in Recents: it is a list, not a folder.
        let mut d = files(&["a.txt"]);
        d.recents = vec!["/work/a.txt".into()];
        d.click("files-recents").unwrap();
        d.click("open:0").unwrap();
        for command in ["files-rename", "files-delete", "files-new-file"] {
            assert!(d.click(command).is_err(), "{command} acted on Recents");
        }
        // And a filter typed in one folder does not follow the tab to the next.
        let mut d = files(&["a.txt"]);
        d.click("files-search").unwrap();
        d.text("zz").unwrap();
        d.click("files-up").unwrap();
        assert!(tab(&d).query.is_empty() && !tab(&d).searching);
    }
    #[test]
    fn recents_are_bounded_deduplicated_and_only_ever_opened_documents() {
        let mut d = files(&["a.txt", "sub/"]);
        // Opening a folder is navigation, not a document: it never lands in Recents.
        d.click("open:1").unwrap();
        d.activate("open:1").unwrap();
        assert!(d.recents.is_empty());
        for i in 0..RECENT_LIMIT + 5 {
            d.remember(&format!("/work/f{i}.txt"));
        }
        assert_eq!(d.recents.len(), RECENT_LIMIT);
        assert_eq!(d.recents[0], format!("/work/f{}.txt", RECENT_LIMIT + 4));
        // Reopening moves a path to the front rather than repeating it.
        d.remember("/work/f0.txt");
        assert_eq!(d.recents[0], "/work/f0.txt");
        assert_eq!(d.recents.iter().filter(|p| *p == "/work/f0.txt").count(), 1);
        assert_eq!(d.recents.len(), RECENT_LIMIT);
    }
    #[test]
    fn clipboard_names_a_free_destination_and_a_cut_is_consumed_by_its_paste() {
        let mut d = files(&["notes.txt", "notes (copy).txt"]);
        d.click("open:1").unwrap();
        d.click("files-copy").unwrap();
        // The first free `(copy)` name, derived from the listing already on screen.
        let effects = d.click("files-paste").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::CopyPath { to, .. } if to == "/work/notes (copy) 2.txt"
        ));
        // A copy survives its paste, so the same thing can be pasted twice.
        assert!(d.clipboard.is_some());
        d.click("files-cut").unwrap();
        // A cut onto a name already here is refused: a move must not rename itself.
        assert!(d.click("files-paste").is_err());
        d.click("files-up").unwrap();
        let effects = d.click("files-paste").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::MovePath { from, to, .. }
                if from == "/work/notes.txt" && to == "/notes.txt"
        ));
        assert!(d.clipboard.is_none(), "a cut outlived its paste");
        assert_eq!(
            Clipboard::new(vec!["/p".into(); CLIPBOARD_LIMIT + 9], true)
                .paths
                .len(),
            CLIPBOARD_LIMIT
        );
    }
    #[test]
    fn delete_is_a_move_into_the_trash_and_refuses_to_nest_it() {
        let mut d = files(&["notes.txt"]);
        assert_eq!(d.trash_folder(), "/home/alice/.local/share/Trash/files");
        d.click("open:0").unwrap();
        let effects = d.click("files-delete").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::TrashPath { path, trash, .. }
                if path == "/work/notes.txt" && trash == &d.trash_folder()
        ));
        // Emptying the trash from inside it is refused rather than nesting it.
        let mut d = files(&["old.txt"]);
        let (id, _) = d.launch("files", &d.trash_folder()).unwrap();
        d.directory_loaded(id, 0, vec!["old.txt".into()]).unwrap();
        d.click("open:0").unwrap();
        assert!(d.click("files-delete").is_err());
    }
    /// The same trash from either spelling, the Delete key included, and a Restore that
    /// exists only where there is a record to restore from.
    #[test]
    fn move_to_trash_and_restore_are_one_trash_and_refuse_everywhere_else() {
        let mut d = files(&["notes.txt"]);
        d.click("open:0").unwrap();
        let effects = d.click("files-move-to-trash").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::TrashPath { path, trash, .. }
                if path == "/work/notes.txt" && trash == &d.trash_folder()
        ));
        // The Delete key is the same command, and only outside a text field.
        assert!(matches!(
            &d.key("Delete").unwrap()[0],
            AppEffect::TrashPath { path, .. } if path == "/work/notes.txt"
        ));
        d.click("files-rename").unwrap();
        assert!(d.key("Delete").is_err(), "Delete deleted a file mid-rename");
        d.key("Escape").unwrap();
        // Restore and Empty have no record to read outside the trash, and say so
        // rather than guessing where a file used to live.
        assert!(d.click("files-restore").is_err());
        assert!(d.click("files-restore:0").is_err());
        assert!(d.click("files-empty-trash").is_err());

        let trash = d.trash_folder();
        let (id, _) = d.launch("files", &trash).unwrap();
        d.directory_listed(
            id,
            0,
            vec![FileRow {
                entry: "old.txt".into(),
                original: Some("/work/old.txt".into()),
                ..FileRow::named("old.txt")
            }],
        )
        .unwrap();
        // Nothing is selected yet, so there is nothing to put back.
        assert!(d.click("files-restore").is_err());
        d.click("open:0").unwrap();
        let effects = d.click("files-restore").unwrap();
        assert!(
            matches!(&effects[0], AppEffect::RestorePath { path, .. }
                if *path == format!("{trash}/old.txt")),
            "{effects:?}"
        );
        // The Trash is re-listed afterwards: what is on screen is the machine's answer.
        assert!(matches!(&effects[1], AppEffect::ListDirectory { path, .. } if *path == trash));
        // A row on screen can be named directly, the way a context menu names one.
        assert!(matches!(
            &d.click("files-restore:0").unwrap()[0],
            AppEffect::RestorePath { path, .. } if *path == format!("{trash}/old.txt")
        ));
        assert!(matches!(
            &d.click("files-empty-trash").unwrap()[0],
            AppEffect::EmptyTrash { .. }
        ));
        // The row knows where it came from, which is what the Trash view draws.
        assert_eq!(
            d.focused_tab().unwrap().original_of(0).as_deref(),
            Some("/work/old.txt")
        );
        // An empty trash has nothing to empty.
        d.directory_listed(id, 0, vec![]).unwrap();
        assert!(d.click("files-empty-trash").is_err());
    }
    /// A listing carries the machine's own `stat`, and the Size and Date columns sort
    /// on it. What the machine said nothing about sorts last instead of posing as zero.
    #[test]
    fn a_listing_carries_real_metadata_and_the_new_columns_sort_on_it() {
        let mut d = DesktopState {
            home: "/home/alice".into(),
            ..DesktopState::default()
        };
        let (id, _) = d.launch("files", "/work").unwrap();
        let row = |entry: &str, kind, size, modified| FileRow {
            entry: entry.into(),
            kind,
            size,
            mode: Some(0o644),
            modified,
            original: None,
        };
        d.directory_listed(
            id,
            0,
            vec![
                row("big.bin", EntryKind::File, Some(4096), Some(3_000_000)),
                row("small.txt", EntryKind::File, Some(12), Some(9_000_000)),
                row("link", EntryKind::Symlink, Some(9), Some(1_000_000)),
                row("sub/", EntryKind::Directory, None, Some(5_000_000)),
            ],
        )
        .unwrap();
        let shown = |d: &DesktopState| -> Vec<String> {
            let tab = d.focused_tab().unwrap();
            tab.display()
                .into_iter()
                .map(|i| tab.entries[i].clone())
                .collect()
        };
        // The metadata survives the listing, kind and all.
        let tab = d.focused_tab().unwrap();
        assert_eq!(tab.entries, ["big.bin", "link", "small.txt", "sub/"]);
        assert_eq!(tab.row(1).unwrap().kind, EntryKind::Symlink);
        assert_eq!(tab.row(2).unwrap().size, Some(12));
        assert_eq!(tab.row(2).unwrap().mode, Some(0o644));
        assert_eq!(tab.row(3).unwrap().modified, Some(5_000_000));
        // Smallest first, and the folder — which has no byte count — last.
        d.click("files-sort:size").unwrap();
        assert_eq!(shown(&d), ["link", "small.txt", "big.bin", "sub/"]);
        // Oldest first.
        d.click("files-sort:modified").unwrap();
        assert_eq!(shown(&d), ["link", "big.bin", "sub/", "small.txt"]);
        // The same key again reverses it, exactly as Name and Kind do.
        d.click("files-sort:modified").unwrap();
        assert!(d.focused_tab().unwrap().descending);
        assert_eq!(shown(&d), ["small.txt", "sub/", "big.bin", "link"]);
        // "date" is the other spelling of the same column.
        assert_eq!(SortKey::parse("date"), Some(SortKey::Modified));
        assert_eq!(SortKey::parse("bogus"), None);
        assert!(d.click("files-sort:bogus").is_err());
        // A listing given as bare names says nothing it was not told.
        d.directory_loaded(id, 0, vec!["plain.txt".into()]).unwrap();
        let only = d.focused_tab().unwrap().row(0).unwrap();
        assert_eq!(
            (only.size, only.modified, only.kind),
            (None, None, EntryKind::File)
        );
    }
    #[test]
    fn a_rename_only_commits_a_name_that_is_really_a_name() {
        let mut d = files(&["notes.txt", "launch.txt"]);
        d.click("open:1").unwrap();
        d.click("files-rename").unwrap();
        assert_eq!(tab(&d).rename.as_ref().unwrap().name, "notes.txt");
        for bad in ["", "..", "../escape", "a/b", r"a\b"] {
            let t = d.focused_tab_mut().unwrap();
            t.rename.as_mut().unwrap().name = bad.into();
            assert!(d.key("Enter").is_err(), "{bad} was accepted as a name");
            // A refused commit clears the edit rather than leaving it half-applied.
            assert!(tab(&d).rename.is_none());
            d.click("files-rename").unwrap();
        }
        d.focused_tab_mut().unwrap().rename.as_mut().unwrap().name = "launch.txt".into();
        assert!(d.key("Enter").is_err(), "renamed onto an existing file");
        d.click("files-rename").unwrap();
        d.key("Backspace").unwrap();
        let effects = d.key("Enter").unwrap();
        assert!(matches!(
            &effects[0],
            AppEffect::MovePath { from, to, .. }
                if from == "/work/notes.txt" && to == "/work/notes.tx"
        ));
    }
    #[test]
    fn the_terminal_caret_and_scroll_are_state_the_snapshot_keeps() {
        let mut d = DesktopState::default();
        let (id, _) = d.launch("terminal", "").unwrap();
        d.text("echo hi").unwrap();
        d.click_at("terminal-line", 4 * 8, 0).unwrap();
        d.text("X").unwrap();
        d.key("Backspace").unwrap();
        d.key("Home").unwrap();
        d.text("!").unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { input, cursor, .. } => {
                assert_eq!(input, "!echo hi");
                assert_eq!(*cursor, 1);
            }
            _ => panic!("not a terminal"),
        }
        // Scroll is bounded, and any output pins the view back to the tail.
        d.click(&format!("terminal-scroll:{}", SCROLL_LIMIT + 1000))
            .unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { scroll, .. } => assert_eq!(*scroll, SCROLL_LIMIT),
            _ => panic!("not a terminal"),
        }
        d.terminal_output(id, TerminalEntry::new("$", "ls", "a", "", 0))
            .unwrap();
        match &d.windows[&id].state {
            AppState::Terminal { scroll, .. } => assert_eq!(*scroll, 0),
            _ => panic!("not a terminal"),
        }
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
    /// A star is keyed by the absolute path, folders keep their separator, the same
    /// star again takes it off, and the Starred list follows at once.
    #[test]
    fn stars_toggle_by_path_and_the_starred_list_follows() {
        let mut d = files(&["notes.txt", "src/"]);
        d.click("files-star:1").unwrap();
        d.click("files-star:0").unwrap();
        // Newest first: the list reads in the order things were starred.
        assert_eq!(d.starred, ["/work/notes.txt", "/work/src/"]);
        assert!(d.is_starred("/work/src"));
        d.click("files-starred").unwrap();
        assert_eq!(tab(&d).scope, FileScope::Starred);
        assert_eq!(tab(&d).entries, d.starred);
        // Rows in a list are absolute, so opening a starred folder goes there.
        d.click("open:1").unwrap();
        assert_eq!(tab(&d).selected_path().unwrap(), "/work/src");
        // Unstarring from the list takes the row off, and the selection stays put.
        d.click("files-star:0").unwrap();
        assert_eq!(tab(&d).entries, ["/work/src/"]);
        assert_eq!(tab(&d).selection().unwrap(), "/work/src/");
        // Nothing selected, nothing to star; and the list is not a folder to edit.
        d.click("files-star").unwrap();
        assert!(d.starred.is_empty());
        assert!(d.click("files-star").is_err());
        assert!(d.click("files-new-folder").is_err());
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
    /// Explorer's Home and Gallery are shaped from a real listing when it lands.
    #[test]
    fn home_and_gallery_are_views_over_real_listings() {
        let mut d = files(&["x.txt"]);
        let id = d.focused.unwrap();
        d.starred = vec!["/work/x.txt".into()];
        d.recents = vec!["/work/x.txt".into(), "/home/alice/a.txt".into()];
        let effects = d.click("files-quick-access").unwrap();
        assert!(matches!(
            &effects[..],
            [AppEffect::ListDirectory { path, .. }] if path == "/home/alice"
        ));
        // Only the pinned folders the listing holds, in pinned order, then the
        // favourites, then the recents, each path once.
        d.directory_loaded(
            id,
            0,
            vec![
                "Music/".into(),
                "Desktop/".into(),
                "notes/".into(),
                "a.txt".into(),
            ],
        )
        .unwrap();
        assert_eq!(
            tab(&d).entries,
            [
                "/home/alice/Desktop/",
                "/home/alice/Music/",
                "/work/x.txt",
                "/home/alice/a.txt"
            ]
        );
        assert!(d.click("files-paste").is_err());
        d.click("files-gallery").unwrap();
        d.directory_loaded(
            id,
            0,
            vec![
                "a.PNG".into(),
                "b.txt".into(),
                "c.jpg".into(),
                "d.png/".into(),
            ],
        )
        .unwrap();
        assert_eq!(tab(&d).path, "/home/alice/Pictures");
        assert_eq!(tab(&d).entries, ["a.PNG", "c.jpg"]);
        d.click("open:1").unwrap();
        assert_eq!(
            tab(&d).selected_path().unwrap(),
            "/home/alice/Pictures/c.jpg"
        );
        // Reload re-reads the view, not the folder under it.
        let effects = d.click("files-reload").unwrap();
        assert_eq!(tab(&d).scope, FileScope::Gallery);
        assert_eq!(effects.len(), 1);
    }
    #[test]
    fn dot_files_are_hidden_until_the_tab_shows_them() {
        let mut d = files(&[".cache/", "a.txt", ".profile"]);
        assert_eq!(tab(&d).display().len(), 1);
        assert_eq!(tab(&d).listed(), 1);
        d.click("open:0").unwrap();
        assert_eq!(tab(&d).selection().unwrap(), "a.txt");
        d.key("Ctrl+h").unwrap();
        assert_eq!(tab(&d).display().len(), 3);
        assert_eq!(tab(&d).listed(), 3);
        d.click("files-hidden").unwrap();
        assert_eq!(tab(&d).display().len(), 1);
        // The trash place is a folder like any other, with its own title.
        d.click("files-trash").unwrap();
        assert_eq!(tab(&d).path, "/home/alice/.local/share/Trash/files");
        let t = desktop_scene::DesktopTheme::Windows;
        assert_eq!(
            tab(&d).title(t, "/home/alice", &d.trash_folder()),
            "Recycle Bin"
        );
        assert!(tab(&d).is_place(&d.trash_folder()));
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;
    #[test]
    fn shift_cycles_off_once_lock_and_a_one_shot_releases_after_one_character() {
        let mut k = KeyboardState::default();
        assert_eq!(k.apply('a'), "a");
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Once);
        assert_eq!(k.apply('a'), "A");
        assert_eq!(k.shift, Shift::Off, "a one-shot must release");
        assert_eq!(k.apply('b'), "b");
        k.cycle_shift();
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Lock);
        assert_eq!((k.apply('c'), k.apply('d')), ("C".into(), "D".into()));
        assert_eq!(k.shift, Shift::Lock, "a lock must not release");
        k.cycle_shift();
        assert_eq!(k.shift, Shift::Off);
    }
    #[test]
    fn leaving_the_letter_plane_drops_a_pending_shift() {
        let mut k = KeyboardState::default();
        k.cycle_shift();
        k.set_plane(Plane::Numbers);
        assert_eq!(k.shift, Shift::Off, "a shift must not survive into digits");
        assert_eq!(k.plane, Plane::Numbers);
        k.set_plane(Plane::Letters);
        k.cycle_shift();
        k.set_plane(Plane::Letters);
        assert_eq!(k.shift, Shift::Once, "staying put must not drop it");
    }
    #[test]
    fn a_character_with_no_upper_case_is_unchanged_by_shift() {
        let mut k = KeyboardState {
            shift: Shift::Lock,
            plane: Plane::Letters,
        };
        assert_eq!(k.apply('1'), "1");
        assert_eq!(k.apply('.'), ".");
        // Non-ASCII still upper-cases, and may widen: ß becomes SS.
        assert_eq!(k.apply('é'), "É");
    }
}
#[cfg(test)]
mod focus_tests {
    use super::*;
    #[test]
    fn wrong_application_input_and_invalid_editor_cursor_rejected() {
        let mut desktop = DesktopState::default();
        let (id, _) = desktop.launch("editor", "/a").unwrap();
        assert!(desktop.click("terminal-input").is_err());
        desktop.click("editor-text").unwrap();
        if let AppState::Editor { text, cursor, .. } =
            &mut desktop.windows.get_mut(&id).unwrap().state
        {
            *text = "😀".into();
            *cursor = 1;
        }
        assert!(desktop.key("Backspace").is_err());
    }
}

#[cfg(test)]
mod window_geometry_tests {
    use super::*;
    use cw_scene::Rect;
    #[test]
    fn dragging_capture_survives_snapshot_and_focus_is_mru() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("editor", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let before = d.effective_frame(a, area);
        d.pointer_down(a, "drag", before.x + 100, before.y + 12, area)
            .unwrap();
        assert_eq!(d.ordered_windows(), vec![b, a]);
        d.pointer_move(before.x + 150, before.y + 42, area).unwrap();
        let mut restored: DesktopState =
            serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        restored
            .pointer_up(before.x + 150, before.y + 42, area)
            .unwrap();
        assert_eq!(
            restored.effective_frame(a, area),
            Rect::new(before.x + 50, before.y + 30, before.width, before.height)
        );
        restored.minimize(a).unwrap();
        assert_eq!(restored.focused, Some(b));
        restored.focus(a).unwrap();
        assert!(!restored.windows[&a].minimized);
    }
    #[test]
    fn maximize_restore_is_per_window_and_snap_tearoff_restores_size() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("files", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let original = d.effective_frame(a, area);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), area);
        assert!(!d.windows[&b].maximized);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), original);
        d.snap(a, WindowSnap::Left, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, 600);
        d.pointer_down(a, "drag", 200, 45, area).unwrap();
        d.pointer_up(450, 200, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, original.width);
        assert_eq!(d.windows[&a].snapped, None);
    }
    #[test]
    fn all_resize_edges_preserve_opposite_edge_and_reject_bad_handles() {
        let area = Rect::new(0, 0, 1400, 1000);
        for edge in ["n", "ne", "e", "se", "s", "sw", "w", "nw"] {
            let mut d = DesktopState::default();
            let (id, _) = d.launch("terminal", "").unwrap();
            d.windows.get_mut(&id).unwrap().frame = Some(Rect::new(200, 200, 600, 400));
            d.pointer_down(id, &format!("resize:{edge}"), 300, 300, area)
                .unwrap();
            d.pointer_up(320, 330, area).unwrap();
            let r = d.effective_frame(id, area);
            assert_eq!(r.x, if edge.contains('w') { 220 } else { 200 });
            assert_eq!(r.y, if edge.contains('n') { 230 } else { 200 });
            assert_eq!(
                r.width,
                if edge.contains('w') {
                    580
                } else if edge.contains('e') {
                    620
                } else {
                    600
                }
            );
            assert_eq!(
                r.height,
                if edge.contains('n') {
                    370
                } else if edge.contains('s') {
                    430
                } else {
                    400
                }
            );
            assert!(d.pointer_down(id, "resize:bad", 0, 0, area).is_err());
        }
    }
}
