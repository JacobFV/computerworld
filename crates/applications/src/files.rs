use super::*;

/// List or icon grid. Purely how the same rows are laid out; the order and the click
/// targets are identical in both, so a view change can never move a file under a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileView {
    #[default]
    List,
    Grid,
}
impl FileView {
    pub fn is_list(&self) -> bool {
        *self == Self::List
    }
}
/// The columns a listing really has. A listing carries the machine's own metadata —
/// kind, size and modification time, straight off `stat` — so every one of these keys
/// sorts on something an observer can read back off the filesystem. A row whose
/// metadata never arrived sorts last rather than pretending to a size it does not have.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Kind,
    Size,
    Modified,
}
impl SortKey {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "name" => Some(Self::Name),
            "kind" => Some(Self::Kind),
            "size" => Some(Self::Size),
            // Finder calls the column "Date Modified" and Explorer "Date modified";
            // both spellings reach the same key.
            "modified" | "date" => Some(Self::Modified),
            _ => None,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Kind => "kind",
            Self::Size => "size",
            Self::Modified => "modified",
        }
    }
}
/// What a listed entry actually is, as `lstat` reports it rather than as its name
/// suggests. A symbolic link is its own kind: following it would hide the link, and
/// guessing from the extension would invent a fact the filesystem never stated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    #[default]
    File,
    Directory,
    Symlink,
}
impl EntryKind {
    pub fn is_dir(self) -> bool {
        self == Self::Directory
    }
}
/// One row of a listing, with the metadata the machine really reported for it. The
/// `entry` is spelled exactly as `FileTab::entries` spells it — a folder ends in `/` —
/// so a row and its entry can never drift apart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow {
    pub entry: String,
    #[serde(default)]
    pub kind: EntryKind,
    /// Bytes, as `stat` reports them. `None` for a folder (no file manager claims a
    /// byte count for one) and for a row whose metadata never arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Permission bits, `None` when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u16>,
    /// World-clock microseconds of the last write, `None` when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
    /// Where a trashed item came from, read from its `.trashinfo` record. Only the
    /// Trash has these, and it is what lets the Trash say where a row used to live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}
impl FileRow {
    /// A row with nothing but its name: what a list the desktop keeps (Recents,
    /// Starred) can honestly say, and the fallback for any listing that arrived
    /// without metadata.
    pub fn named(entry: impl Into<String>) -> Self {
        let entry = entry.into();
        let kind = if entry.ends_with('/') {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        Self {
            entry,
            kind,
            ..Self::default()
        }
    }
    /// The name without the listing's folder marker.
    pub fn name(&self) -> &str {
        entry_name(&self.entry)
    }
}
/// What a tab is listing. `Recents`, `Starred` and `QuickAccess` hold absolute paths
/// taken from desktop state, which is why an entry there is not a child of `path`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileScope {
    #[default]
    Folder,
    Recents,
    /// `DesktopState::starred`: Files' Starred, Explorer's Favorites.
    Starred,
    /// Explorer's Home: the Quick access folders the home folder really holds, then
    /// the favourites, then the recent files.
    QuickAccess,
    /// Explorer's Gallery: the image files in the Pictures folder, which `path` names.
    Gallery,
}
impl FileScope {
    /// Entries are absolute paths rather than names inside `path`.
    pub fn absolute(self) -> bool {
        matches!(self, Self::Recents | Self::Starred | Self::QuickAccess)
    }
}
/// A rename in progress: the entry it started from, and the name being typed. Held
/// separately from the entry so a listing arriving mid-edit cannot retarget it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    pub from: String,
    pub name: String,
}
/// Paths cut or copied in a file manager. Lives on the desktop, not the tab, because a
/// clipboard is shared: copy in one window, paste in another.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clipboard {
    pub paths: Vec<String>,
    /// A cut moves on paste; a copy duplicates.
    pub cut: bool,
    /// Pixels copied from an image editor. A clipboard holds files or a picture, and
    /// copying one replaces the other, as on every desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<cw_raster::Canvas>,
}
impl Clipboard {
    /// Bounded at `CLIPBOARD_LIMIT`: a clipboard holds a handful of paths, not a tree.
    pub fn new(mut paths: Vec<String>, cut: bool) -> Self {
        paths.truncate(CLIPBOARD_LIMIT);
        Self {
            paths,
            cut,
            image: None,
        }
    }
    pub fn picture(image: cw_raster::Canvas) -> Self {
        Self {
            paths: vec![],
            cut: false,
            image: Some(image),
        }
    }
}
/// One folder view inside a file manager window, with its own listing,
/// selection and back/forward history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTab {
    pub path: String,
    #[serde(default)]
    pub entries: Vec<String>,
    /// The metadata the machine reported for each entry, in the same order. Kept
    /// beside `entries` rather than inside it so every existing reader of a listing
    /// still sees the `name` / `name/` spelling it was written against; `row()` is the
    /// one way in, and it falls back to a name-only row whenever the two have drifted,
    /// so a listing set without metadata degrades instead of lying.
    #[serde(default)]
    pub rows: Vec<FileRow>,
    /// Index into `entries`, not into the displayed rows: a file stays selected when
    /// the sort or the filter moves it.
    #[serde(default)]
    pub selected: Option<usize>,
    /// Visited folders, oldest first; `position` is the one on screen.
    #[serde(default)]
    pub history: Vec<String>,
    #[serde(default)]
    pub position: usize,
    #[serde(default)]
    pub view: FileView,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub descending: bool,
    /// Substring filter applied to the listing, capped at `FIELD_LIMIT`.
    #[serde(default)]
    pub query: String,
    /// True while the search field is collecting keystrokes.
    #[serde(default)]
    pub searching: bool,
    #[serde(default)]
    pub rename: Option<Rename>,
    #[serde(default)]
    pub scope: FileScope,
    /// Dot files are listed. Off by default, as in Files, Finder and Explorer;
    /// `files-hidden` (and Ctrl+H) flips it.
    #[serde(default)]
    pub show_hidden: bool,
}
impl FileTab {
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            entries: vec![],
            rows: vec![],
            selected: None,
            history: vec![path.clone()],
            position: 0,
            view: FileView::default(),
            sort: SortKey::default(),
            descending: false,
            query: String::new(),
            searching: false,
            rename: None,
            scope: FileScope::default(),
            show_hidden: false,
            path,
        }
    }
    pub fn can_go_back(&self) -> bool {
        self.position > 0
    }
    pub fn can_go_forward(&self) -> bool {
        self.position + 1 < self.history.len()
    }
    /// What the platform calls what this tab shows: a list's own name, the Trash, the
    /// computer, or else the folder's name. Used for tab labels and window titles.
    pub fn title(&self, theme: desktop_scene::DesktopTheme, home: &str, trash: &str) -> String {
        use desktop_scene::DesktopTheme::*;
        let at =
            |p: &str| !p.is_empty() && self.path.trim_end_matches('/') == p.trim_end_matches('/');
        match self.scope {
            FileScope::Recents if matches!(theme, Macos | Ios) => "Recents".into(),
            FileScope::Recents => "Recent".into(),
            FileScope::Starred if theme == Windows => "Favorites".into(),
            FileScope::Starred => "Starred".into(),
            FileScope::QuickAccess => "Home".into(),
            FileScope::Gallery => "Gallery".into(),
            FileScope::Folder if at(trash) => match theme {
                Windows => "Recycle Bin".into(),
                Android => "Bin".into(),
                _ => "Trash".into(),
            },
            FileScope::Folder if at(home) && theme == Ubuntu => "Home".into(),
            FileScope::Folder if self.path == "/" => match theme {
                Macos => "Macintosh HD".into(),
                Windows => "This PC".into(),
                Ubuntu => "Computer".into(),
                Ios => "On My iPhone".into(),
                Android => "Internal storage".into(),
            },
            FileScope::Folder => self.name().to_owned(),
        }
    }
    /// A place that is not an ordinary folder to climb through: a list, a view over a
    /// listing, or the Trash. Path bars show its name instead of a folder trail.
    pub fn is_place(&self, trash: &str) -> bool {
        self.scope != FileScope::Folder
            || (!trash.is_empty() && self.path.trim_end_matches('/') == trash.trim_end_matches('/'))
    }
    /// Folder name for the tab label; the root keeps its separator.
    pub fn name(&self) -> &str {
        self.path
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("/")
    }
    pub fn selection(&self) -> Option<&String> {
        self.entries.get(self.selected?)
    }
    /// Replace the listing with rows the machine described. `entries` keeps its old
    /// spelling so nothing that reads it has to change.
    pub fn set_rows(&mut self, rows: Vec<FileRow>) {
        self.entries = rows.iter().map(|r| r.entry.clone()).collect();
        self.rows = rows;
    }
    /// Replace the listing with bare names, for a list the desktop keeps rather than a
    /// folder the machine described.
    pub fn set_names(&mut self, names: Vec<String>) {
        self.rows = names.iter().map(FileRow::named).collect();
        self.entries = names;
    }
    /// The metadata for `entries[index]`. A row whose metadata is missing or has
    /// drifted out of step comes back as a name-only row: unknown is said as unknown,
    /// never borrowed from the neighbour.
    pub fn row(&self, index: usize) -> Option<FileRow> {
        let entry = self.entries.get(index)?;
        Some(match self.rows.get(index) {
            Some(row) if row.entry == *entry => row.clone(),
            _ => FileRow::named(entry),
        })
    }
    /// Where a trashed row came from, when this tab is showing the Trash and the
    /// record said.
    pub fn original_of(&self, index: usize) -> Option<String> {
        self.rows
            .get(index)
            .filter(|row| Some(&row.entry) == self.entries.get(index))
            .and_then(|row| row.original.clone())
    }
    /// Whether this tab is showing the machine's trash folder.
    pub fn in_trash(&self, trash: &str) -> bool {
        !trash.is_empty() && self.path.trim_end_matches('/') == trash.trim_end_matches('/')
    }
    /// Absolute path of `entry` inside this folder.
    pub fn child(&self, entry: &str) -> String {
        format!("{}/{}", self.path.trim_end_matches('/'), entry)
    }
    /// The rows on screen, in the order the screen shows them, as indices into
    /// `entries`. This is the one place the filter and the sort are applied, and it is
    /// what `open:<i>` indexes: `i` is a position on screen, never a position in the
    /// raw listing, so reordering the view can never open the file next to the one
    /// that was clicked. Both the painter and the semantic page walk this list.
    pub fn display(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        // A list of paths names what was really opened or starred, dot or not; a folder
        // listing hides dot files unless the tab shows them.
        let hide = !self.show_hidden && !self.scope.absolute();
        let mut rows: Vec<usize> = (0..self.entries.len())
            .filter(|i| !(hide && self.entries[*i].starts_with('.')))
            .filter(|i| query.is_empty() || self.entries[*i].to_lowercase().contains(&query))
            .collect();
        // Sort keys read off the metadata the listing carried, resolved once so the
        // comparator does no work that could differ between two calls. `None` is
        // unknown, and unknown sorts last in either direction rather than posing as
        // zero bytes or as the epoch.
        let key = |i: usize| -> (Option<u64>, Option<u64>) {
            match self.rows.get(i) {
                Some(row) if Some(&row.entry) == self.entries.get(i) => (row.size, row.modified),
                _ => (None, None),
            }
        };
        let last = |v: Option<u64>| (v.is_none(), v.unwrap_or(0));
        rows.sort_by(|a, b| {
            let (x, y) = (self.entries[*a].as_str(), self.entries[*b].as_str());
            let order = match self.sort {
                // Folders before files, then by name, so the Kind column really groups.
                SortKey::Kind => x.ends_with('/').cmp(&y.ends_with('/')).reverse(),
                SortKey::Size => last(key(*a).0).cmp(&last(key(*b).0)),
                SortKey::Modified => last(key(*a).1).cmp(&last(key(*b).1)),
                SortKey::Name => std::cmp::Ordering::Equal,
            }
            .then_with(|| entry_name(x).cmp(entry_name(y)));
            if self.descending {
                order.reverse()
            } else {
                order
            }
            // Index last: a total order keeps the projection deterministic even if a
            // listing ever repeats a name.
            .then(a.cmp(b))
        });
        rows
    }
    /// How many entries the view lists before any search: everything but the dot files
    /// it is hiding. A status bar's "N of M" counts against this, not the raw listing.
    pub fn listed(&self) -> usize {
        let hide = !self.show_hidden && !self.scope.absolute();
        self.entries
            .iter()
            .filter(|e| !(hide && e.starts_with('.')))
            .count()
    }
    /// Screen row of `entries[index]`, or `None` when the filter hides it.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        self.display().into_iter().position(|i| i == index)
    }
    /// Absolute path of the selected entry, in either scope.
    pub fn selected_path(&self) -> Option<String> {
        let entry = entry_name(self.selection()?).to_owned();
        Some(if self.scope.absolute() {
            entry
        } else {
            self.child(&entry)
        })
    }
    /// The field collecting keystrokes, if any. A file manager opens one at a time.
    /// Whether this tab currently has a text field open, so the router and the shell
    /// agree about where a keystroke goes without either guessing.
    pub fn editing_text(&self) -> bool {
        self.rename.is_some() || self.searching
    }
    pub(super) fn field_mut(&mut self) -> Option<&mut String> {
        match (&mut self.rename, self.searching) {
            (Some(rename), _) => Some(&mut rename.name),
            (None, true) => Some(&mut self.query),
            (None, false) => None,
        }
    }
    /// Leave every text field. Any navigation ends an edit rather than carrying a
    /// half-typed name to a folder it was never meant for.
    pub(super) fn stop_editing(&mut self) {
        self.rename = None;
        self.searching = false;
    }
}
/// An entry as a name: the listing marks folders with a trailing separator, which is
/// display, not part of the path.
pub fn entry_name(entry: &str) -> &str {
    entry.trim_end_matches('/')
}
/// First `stem<suffix>extension` the listing does not already hold. Deterministic, and
/// derived only from what the tab was told is there; the kernel still refuses a
/// destination that exists, so a stale listing cannot overwrite anything.
pub(super) fn free_name(entries: &[String], stem: &str, extension: &str, joiner: &str) -> String {
    let taken = |name: &str| entries.iter().any(|e| entry_name(e) == name);
    let first = format!("{stem}{extension}");
    if !taken(&first) {
        return first;
    }
    for n in 2..=99 {
        let candidate = format!("{stem}{joiner}{n}{extension}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("{stem}{joiner}100{extension}")
}
