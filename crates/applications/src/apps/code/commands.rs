//! The command table: one entry per command the palette, the menus, the keybindings and
//! the buttons all dispatch, so a menu item and its shortcut can never do different things.

pub struct Command {
    pub id: &'static str,
    /// Title in the Command Palette, with its category.
    pub title: &'static str,
    /// Label in a menu.
    pub label: &'static str,
    /// Keybinding in Windows/Linux form; the macOS form is derived from it.
    pub keys: &'static str,
}
macro_rules! commands {
    ($(($id:literal, $title:literal, $label:literal, $keys:literal)),+ $(,)?) => {
        pub const COMMANDS: &[Command] = &[$(Command { id: $id, title: $title, label: $label, keys: $keys }),+];
    };
}
commands! {
    ("workbench.action.showCommands", "Show All Commands", "Command Palette...", "Ctrl+Shift+P"),
    ("workbench.action.quickOpen", "Go to File...", "Go to File...", "Ctrl+P"),
    ("workbench.action.gotoLine", "Go to Line/Column...", "Go to Line/Column...", "Ctrl+G"),
    ("workbench.action.files.newUntitledFile", "File: New Untitled Text File", "New Text File", "Ctrl+N"),
    ("explorer.newFile", "File: New File...", "New File...", ""),
    ("explorer.newFolder", "File: New Folder...", "New Folder...", ""),
    ("workbench.action.files.openFolder", "File: Open Folder...", "Open Folder...", "Ctrl+K Ctrl+O"),
    ("workbench.action.closeFolder", "Workspaces: Close Workspace", "Close Folder", "Ctrl+K F"),
    ("workbench.action.files.save", "File: Save", "Save", "Ctrl+S"),
    ("workbench.action.files.saveAs", "File: Save As...", "Save As...", "Ctrl+Shift+S"),
    ("workbench.action.files.saveAll", "File: Save All Files", "Save All", "Ctrl+K S"),
    ("workbench.action.files.revert", "File: Revert File", "Revert File", ""),
    ("workbench.action.closeActiveEditor", "View: Close Editor", "Close Editor", "Ctrl+W"),
    ("workbench.action.closeAllEditors", "View: Close All Editors", "Close All Editors", ""),
    ("workbench.action.keepEditor", "View: Keep Editor", "Keep Editor", "Ctrl+K Enter"),
    ("workbench.action.nextEditor", "View: Open Next Editor", "Next Editor", "Ctrl+PageDown"),
    ("workbench.action.previousEditor", "View: Open Previous Editor", "Previous Editor", "Ctrl+PageUp"),
    ("undo", "Undo", "Undo", "Ctrl+Z"),
    ("redo", "Redo", "Redo", "Ctrl+Y"),
    ("editor.action.clipboardCutAction", "Cut", "Cut", "Ctrl+X"),
    ("editor.action.clipboardCopyAction", "Copy", "Copy", "Ctrl+C"),
    ("editor.action.clipboardPasteAction", "Paste", "Paste", "Ctrl+V"),
    ("actions.find", "Find", "Find", "Ctrl+F"),
    ("editor.action.startFindReplaceAction", "Replace", "Replace", "Ctrl+H"),
    ("workbench.action.findInFiles", "Search: Find in Files", "Find in Files", "Ctrl+Shift+F"),
    ("editor.action.commentLine", "Toggle Line Comment", "Toggle Line Comment", "Ctrl+/"),
    ("editor.action.selectAll", "Select All", "Select All", "Ctrl+A"),
    ("editor.action.copyLinesUpAction", "Copy Line Up", "Copy Line Up", "Shift+Alt+Up"),
    ("editor.action.copyLinesDownAction", "Copy Line Down", "Copy Line Down", "Shift+Alt+Down"),
    ("editor.action.moveLinesUpAction", "Move Line Up", "Move Line Up", "Alt+Up"),
    ("editor.action.moveLinesDownAction", "Move Line Down", "Move Line Down", "Alt+Down"),
    ("editor.action.deleteLines", "Delete Line", "Delete Line", "Ctrl+Shift+K"),
    ("editor.action.indentLines", "Indent Line", "Indent Line", "Ctrl+]"),
    ("editor.action.outdentLines", "Outdent Line", "Outdent Line", "Ctrl+["),
    ("editor.action.jumpToBracket", "Go to Bracket", "Go to Bracket", "Ctrl+Shift+\\"),
    ("workbench.view.explorer", "View: Show Explorer", "Explorer", "Ctrl+Shift+E"),
    ("workbench.view.search", "View: Show Search", "Search", "Ctrl+Shift+F"),
    ("workbench.view.scm", "View: Show Source Control", "Source Control", "Ctrl+Shift+G"),
    ("workbench.view.debug", "View: Show Run and Debug", "Run", "Ctrl+Shift+D"),
    ("workbench.action.toggleSidebarVisibility", "View: Toggle Primary Side Bar Visibility", "Primary Side Bar", "Ctrl+B"),
    ("workbench.action.togglePanel", "View: Toggle Panel Visibility", "Panel", "Ctrl+J"),
    ("workbench.actions.view.problems", "View: Toggle Problems", "Problems", "Ctrl+Shift+M"),
    ("workbench.action.output.toggleOutput", "View: Toggle Output", "Output", "Ctrl+Shift+U"),
    ("workbench.action.terminal.toggleTerminal", "View: Toggle Terminal", "Terminal", "Ctrl+`"),
    ("editor.action.toggleWordWrap", "View: Toggle Word Wrap", "Word Wrap", "Alt+Z"),
    ("workbench.action.terminal.new", "Terminal: Create New Terminal", "New Terminal", "Ctrl+Shift+`"),
    ("workbench.action.terminal.kill", "Terminal: Kill the Active Terminal Instance", "Kill Terminal", ""),
    ("workbench.action.terminal.clear", "Terminal: Clear", "Clear Terminal", ""),
    ("workbench.action.terminal.runActiveFile", "Terminal: Run Active File In Active Terminal", "Run Active File", ""),
    ("python.execInTerminal", "Python: Run Python File in Terminal", "Run Python File", ""),
    ("workbench.action.debug.start", "Run: Start Debugging", "Start Debugging", "F5"),
    ("workbench.action.debug.run", "Run: Run Without Debugging", "Run Without Debugging", "Ctrl+F5"),
    ("workbench.action.selectTheme", "Preferences: Color Theme", "Theme", "Ctrl+K Ctrl+T"),
    ("workbench.action.openSettings", "Preferences: Open Settings (UI)", "Settings", "Ctrl+,"),
    ("workbench.action.openSettingsJson", "Preferences: Open User Settings (JSON)", "Open User Settings (JSON)", ""),
    ("workbench.action.editor.changeLanguageMode", "Change Language Mode", "Change Language Mode", "Ctrl+K M"),
    ("editor.action.indentUsingSpaces", "Indent Using Spaces", "Indent Using Spaces", ""),
    ("workbench.action.editor.changeEOL", "Change End of Line Sequence", "Change End of Line Sequence", ""),
    ("workbench.files.action.refreshFilesExplorer", "File: Refresh Explorer", "Refresh Explorer", ""),
    ("workbench.files.action.collapseExplorerFolders", "File: Collapse Folders in Explorer", "Collapse Folders in Explorer", ""),
    ("renameFile", "File: Rename...", "Rename...", "F2"),
    ("deleteFile", "File: Delete", "Delete", "Delete"),
    ("git.init", "Git: Initialize Repository", "Initialize Repository", ""),
    ("git.refresh", "Git: Refresh", "Refresh", ""),
    ("git.stageAll", "Git: Stage All Changes", "Stage All Changes", ""),
    ("git.unstageAll", "Git: Unstage All Changes", "Unstage All Changes", ""),
    ("git.cleanAll", "Git: Discard All Changes", "Discard All Changes", ""),
    ("git.commit", "Git: Commit", "Commit", "Ctrl+Enter"),
    ("git.checkout", "Git: Checkout to...", "Checkout to...", ""),
    ("workbench.action.showAboutDialog", "Help: About", "About", ""),
}

/// Menu bar menus, in order; `-` is a separator.
pub const MENUS: &[(&str, &str, &[&str])] = &[
    (
        "file",
        "File",
        &[
            "workbench.action.files.newUntitledFile",
            "explorer.newFile",
            "explorer.newFolder",
            "-",
            "workbench.action.files.openFolder",
            "workbench.action.closeFolder",
            "-",
            "workbench.action.files.save",
            "workbench.action.files.saveAs",
            "workbench.action.files.saveAll",
            "-",
            "workbench.action.openSettings",
            "workbench.action.selectTheme",
            "-",
            "workbench.action.files.revert",
            "workbench.action.closeActiveEditor",
        ],
    ),
    (
        "edit",
        "Edit",
        &[
            "undo",
            "redo",
            "-",
            "editor.action.clipboardCutAction",
            "editor.action.clipboardCopyAction",
            "editor.action.clipboardPasteAction",
            "-",
            "actions.find",
            "editor.action.startFindReplaceAction",
            "-",
            "workbench.action.findInFiles",
            "-",
            "editor.action.commentLine",
        ],
    ),
    (
        "selection",
        "Selection",
        &[
            "editor.action.selectAll",
            "-",
            "editor.action.copyLinesUpAction",
            "editor.action.copyLinesDownAction",
            "editor.action.moveLinesUpAction",
            "editor.action.moveLinesDownAction",
        ],
    ),
    (
        "view",
        "View",
        &[
            "workbench.action.showCommands",
            "-",
            "workbench.view.explorer",
            "workbench.view.search",
            "workbench.view.scm",
            "workbench.view.debug",
            "-",
            "workbench.actions.view.problems",
            "workbench.action.output.toggleOutput",
            "workbench.action.terminal.toggleTerminal",
            "-",
            "workbench.action.toggleSidebarVisibility",
            "workbench.action.togglePanel",
            "-",
            "editor.action.toggleWordWrap",
        ],
    ),
    (
        "go",
        "Go",
        &[
            "workbench.action.quickOpen",
            "workbench.action.gotoLine",
            "editor.action.jumpToBracket",
            "-",
            "workbench.action.nextEditor",
            "workbench.action.previousEditor",
        ],
    ),
    (
        "run",
        "Run",
        &["workbench.action.debug.start", "workbench.action.debug.run"],
    ),
    (
        "terminal",
        "Terminal",
        &[
            "workbench.action.terminal.new",
            "workbench.action.terminal.kill",
            "workbench.action.terminal.clear",
            "-",
            "workbench.action.terminal.runActiveFile",
        ],
    ),
    (
        "help",
        "Help",
        &[
            "workbench.action.showCommands",
            "workbench.action.showAboutDialog",
        ],
    ),
    (
        "manage",
        "Manage",
        &[
            "workbench.action.showCommands",
            "-",
            "workbench.action.openSettings",
            "workbench.action.openSettingsJson",
            "workbench.action.selectTheme",
        ],
    ),
];

pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A keybinding as the platform writes it: `Ctrl+Shift+P` on Windows and Linux,
/// `⇧⌘P` on a Mac.
pub fn display_keys(keys: &str, mac: bool) -> String {
    if !mac || keys.is_empty() {
        return keys.to_owned();
    }
    keys.split(' ')
        .map(|chord| {
            let parts: Vec<&str> = chord.split('+').collect();
            let (mods, key) = parts.split_at(parts.len() - 1);
            let mut out = String::new();
            // macOS order: ⌃ ⌥ ⇧ ⌘, with the app's Ctrl standing for ⌘.
            if mods.contains(&"Alt") {
                out.push('⌥');
            }
            if mods.contains(&"Shift") {
                out.push('⇧');
            }
            if mods.contains(&"Ctrl") {
                out.push('⌘');
            }
            let key = match key[0] {
                "Up" => "↑",
                "Down" => "↓",
                "Enter" => "↩",
                "PageDown" => "PageDown",
                k => k,
            };
            out.push_str(key);
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A key name normalised to one spelling: modifiers in `ctrl+alt+shift+` order (`Meta`
/// and `Cmd` are `ctrl`, as VS Code maps ⌘ where Windows uses Ctrl), the key lower-cased,
/// and a shifted symbol turned back into Shift plus its unshifted key.
pub fn normalize(key: &str) -> String {
    let (mods, last) = if key.len() > 1 && key.ends_with("++") {
        (&key[..key.len() - 2], "+")
    } else {
        match key.rsplit_once('+') {
            Some((m, k)) if !k.is_empty() => (m, k),
            _ => ("", key),
        }
    };
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "meta" | "cmd" | "command" | "super" | "os" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            _ => {}
        }
    }
    let mut k = last.to_owned();
    if k.chars().count() == 1 {
        let c = k.chars().next().unwrap_or(' ');
        if c.is_ascii_uppercase() && (ctrl || alt) {
            shift = true;
        }
        let unshifted = match c {
            '~' => Some('`'),
            '?' => Some('/'),
            '|' => Some('\\'),
            '_' => Some('-'),
            '{' => Some('['),
            '}' => Some(']'),
            ':' => Some(';'),
            '"' => Some('\''),
            '<' => Some(','),
            '>' => Some('.'),
            _ => None,
        };
        if let (Some(u), true) = (unshifted, ctrl || alt) {
            shift = true;
            k = u.to_string();
        } else {
            k = c.to_ascii_lowercase().to_string();
        }
    } else {
        k = k.to_ascii_lowercase();
        k = match k.as_str() {
            "up" => "arrowup".into(),
            "down" => "arrowdown".into(),
            "left" => "arrowleft".into(),
            "right" => "arrowright".into(),
            "esc" => "escape".into(),
            "return" => "enter".into(),
            "del" => "delete".into(),
            _ => k,
        };
    }
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl+");
    }
    if alt {
        out.push_str("alt+");
    }
    if shift {
        out.push_str("shift+");
    }
    out.push_str(&k);
    out
}

/// The normalised chord a command's first keybinding is pressed as.
pub fn binding(keys: &str) -> Option<String> {
    let first = keys.split(' ').next().filter(|k| !k.is_empty())?;
    // The table writes letters in capitals; the key itself is the unshifted one.
    let mut parts: Vec<String> = first.split('+').map(str::to_owned).collect();
    if let Some(last) = parts.last_mut() {
        match last.as_str() {
            "Up" | "Down" | "Left" | "Right" => *last = format!("Arrow{last}"),
            k if k.len() == 1 => *last = k.to_ascii_lowercase(),
            _ => {}
        }
    }
    Some(normalize(&parts.join("+")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_normalise_to_one_spelling() {
        assert_eq!(normalize("Ctrl+Shift+p"), "ctrl+shift+p");
        assert_eq!(normalize("Meta+P"), "ctrl+shift+p");
        assert_eq!(normalize("Shift+ArrowLeft"), "shift+arrowleft");
        assert_eq!(normalize("Ctrl+~"), "ctrl+shift+`");
        assert_eq!(normalize("Ctrl++"), "ctrl++");
        assert_eq!(normalize("F5"), "f5");
        assert_eq!(normalize("Alt+Shift+ArrowDown"), "alt+shift+arrowdown");
        assert_eq!(binding("Shift+Alt+Down").unwrap(), "alt+shift+arrowdown");
        assert_eq!(binding("Ctrl+PageDown").unwrap(), "ctrl+pagedown");
        assert_eq!(binding("Ctrl+K Ctrl+O").unwrap(), "ctrl+k");
    }
    #[test]
    fn every_menu_item_is_a_command_and_ids_are_unique() {
        for (_, _, items) in MENUS {
            for item in *items {
                assert!(*item == "-" || command(item).is_some(), "{item}");
            }
        }
        let mut ids: Vec<_> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        assert_eq!(display_keys("Ctrl+Shift+P", true), "⇧⌘P");
        assert_eq!(display_keys("Ctrl+K Ctrl+O", true), "⌘K ⌘O");
        assert_eq!(display_keys("Ctrl+Shift+P", false), "Ctrl+Shift+P");
    }
}
