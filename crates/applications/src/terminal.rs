use super::*;

/// The shell prompt this machine would print, in its own dialect. Derived from machine
/// facts so an observer reading the screen sees what the machine says, not what the
/// harness guessed: there is no synthesised prompt left to label as an efference copy.
pub fn shell_prompt(user: &str, host: &str, cwd: &str, dialect: &str) -> String {
    if dialect == "powershell" {
        // The VFS is posix underneath; powershell shows the drive path it would show.
        let path = cwd.trim_start_matches('/').replace('/', "\\");
        return if path.as_bytes().get(1) == Some(&b':') {
            format!("PS {path}>")
        } else {
            format!("PS C:\\{path}>")
        };
    }
    format!("{user}@{host}:{cwd}$")
}
/// The prompt each platform's default shell prints, with the home folder written `~`:
/// bash's `\u@\h:\w\$` (`alice@host:~/src$`) for `posix`, zsh's `%n@%m %1~ %#`
/// (`alice@host src %`) for `zsh`, the default shell of a Mac, and PowerShell's
/// `PS C:\path>`, which has no such abbreviation.
pub fn shell_prompt_at(user: &str, host: &str, cwd: &str, home: &str, dialect: &str) -> String {
    let home = home.trim_end_matches('/');
    let under = !home.is_empty() && (cwd == home || cwd.starts_with(&format!("{home}/")));
    match dialect {
        "zsh" => {
            // `%1~`: the last component, or `~` at home and `/` at the root.
            let dir = if under && cwd.len() == home.len() {
                "~"
            } else {
                cwd.trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("/")
            };
            format!("{user}@{host} {dir} %")
        }
        "powershell" => shell_prompt(user, host, cwd, dialect),
        _ if under => format!("{user}@{host}:~{}$", &cwd[home.len()..]),
        _ => shell_prompt(user, host, cwd, dialect),
    }
}
/// One finished command in a terminal session: the prompt as it stood when the command
/// ran, the command itself, both streams and the exit status. `exit_code` is the point —
/// success is a number here, not a regex over `stderr`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEntry {
    pub prompt: String,
    pub command: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: i32,
}
impl TerminalEntry {
    pub fn new(
        prompt: &str,
        command: impl Into<String>,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> Self {
        Self {
            prompt: prompt_or_sigil(prompt).into(),
            command: command.into(),
            stdout: clamp_stream(stdout),
            stderr: clamp_stream(stderr),
            exit_code,
        }
    }
    /// The echoed command line, exactly as the screen shows it.
    pub fn echo(&self) -> String {
        format!("{} {}", self.prompt, self.command)
    }
    pub fn failed(&self) -> bool {
        self.exit_code != 0
    }
    /// Unambiguous end-of-command marker; `[exit 0]` and `[exit 1]` differ by a character
    /// an observer can match, where error prose differs by wording.
    pub fn status(&self) -> String {
        format!("[exit {}]", self.exit_code)
    }
}
/// Shared fallback so the screen, the semantic page and stored entries never disagree.
pub fn prompt_or_sigil(prompt: &str) -> &str {
    if prompt.is_empty() {
        "$"
    } else {
        prompt
    }
}
/// Keep the head of a stream and say how much was dropped: a listing is read from the
/// top, and a silent cut would look like a shorter result.
fn clamp_stream(text: &str) -> String {
    if text.len() <= STREAM_LIMIT {
        return text.to_owned();
    }
    let mut cut = STREAM_LIMIT;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n[… {} bytes truncated]", &text[..cut], text.len() - cut)
}
