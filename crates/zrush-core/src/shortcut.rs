//! The editor's key for this worktree's conversation, written into the
//! editor's own files: a task that runs zrush, and a key that runs the task.
//!
//! The task names zrush by its absolute path. The desktop app carries the
//! command line beside it, but nothing puts that on the `PATH`, and a task
//! that says `zrush` would find nothing there. The path is the app's own
//! copy, so the key keeps working whichever editor is picked later: picking
//! another one only asks for that editor's files to be written too.
//!
//! The files are JSON with comments, and they are the user's: what is added
//! goes in as text, beside what is there, and a task of ours already there
//! is replaced in place. Everything else, comments included, is left alone.

use std::path::{Path, PathBuf};

use crate::error::{Result, ZrushError};

/// The editors a key can be set up in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Editor {
    Code,
    Cursor,
    Zed,
}

/// Where a key is, or could be, set up.
#[derive(Clone, Debug)]
pub struct Shortcut {
    editor: Editor,
    /// The folder holding the editor's tasks and keys.
    dir: PathBuf,
}

/// How far the key is set up in an editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No task of ours, or no key on it.
    Missing,
    /// Our task is there, but runs another zrush than this one: written by
    /// hand, or by an app that has moved since.
    Stale,
    Installed,
}

impl Shortcut {
    /// The key in the editor `config.toml` calls `name`, in that editor's
    /// own folder; `None` for an editor this cannot set a key in.
    pub fn for_editor(name: &str) -> Option<Self> {
        let editor = editor(name)?;
        let dir = match editor {
            Editor::Code => dirs::config_dir()?.join("Code").join("User"),
            Editor::Cursor => dirs::config_dir()?.join("Cursor").join("User"),
            Editor::Zed if cfg!(windows) => dirs::config_dir()?.join("Zed"),
            Editor::Zed => dirs::home_dir()?.join(".config").join("zed"),
        };
        Some(Self { editor, dir })
    }

    /// The same, with the editor's files in `dir`.
    pub fn in_dir(name: &str, dir: &Path) -> Option<Self> {
        Some(Self {
            editor: editor(name)?,
            dir: dir.to_path_buf(),
        })
    }

    /// The editor's name, as its users know it.
    pub fn editor(&self) -> &'static str {
        match self.editor {
            Editor::Code => "VS Code",
            Editor::Cursor => "Cursor",
            Editor::Zed => "Zed",
        }
    }

    /// The key, as this system writes it.
    pub fn keys() -> &'static str {
        if cfg!(target_os = "macos") {
            "⌘⌥J"
        } else {
            "Ctrl+Alt+J"
        }
    }

    /// What the key does there, in a line.
    pub fn does(&self) -> &'static str {
        match self.editor {
            Editor::Code | Editor::Cursor => "opens this worktree's conversation in a Claude tab.",
            Editor::Zed => "resumes this worktree's conversation in a terminal.",
        }
    }

    fn tasks_file(&self) -> PathBuf {
        self.dir.join("tasks.json")
    }

    fn keys_file(&self) -> PathBuf {
        self.dir.join(match self.editor {
            Editor::Code | Editor::Cursor => "keybindings.json",
            Editor::Zed => "keymap.json",
        })
    }

    fn label(&self) -> &'static str {
        match self.editor {
            Editor::Code | Editor::Cursor => "zrush: claude tab",
            Editor::Zed => "zrush: session here",
        }
    }

    fn args(&self) -> Vec<&'static str> {
        match self.editor {
            Editor::Code => vec!["tab", "--editor", "code"],
            Editor::Cursor => vec!["tab", "--editor", "cursor"],
            Editor::Zed => vec!["session"],
        }
    }

    /// Our task, running `cli`.
    fn task(&self, cli: &Path) -> String {
        let label = json(self.label());
        let command = json(&cli.to_string_lossy());
        let args = self
            .args()
            .iter()
            .map(|a| json(a))
            .collect::<Vec<_>>()
            .join(", ");
        match self.editor {
            // A process, not a shell line: a path with a space in it, as
            // under Program Files, needs no quoting.
            Editor::Code | Editor::Cursor => format!(
                "{{\n  \"label\": {label},\n  \"type\": \"process\",\n  \"command\": {command},\n  \
                 \"args\": [{args}],\n  \"presentation\": {{ \"reveal\": \"silent\", \"panel\": \
                 \"dedicated\", \"close\": true }},\n  \"problemMatcher\": []\n}}"
            ),
            Editor::Zed => format!(
                "{{\n  \"label\": {label},\n  \"command\": {command},\n  \"args\": [{args}],\n  \
                 \"cwd\": \"$ZED_WORKTREE_ROOT\",\n  \"use_new_terminal\": true,\n  \
                 \"allow_concurrent_runs\": true,\n  \"reveal\": \"always\",\n  \"hide\": \"never\"\n}}"
            ),
        }
    }

    /// The key on our task.
    fn binding(&self) -> String {
        let label = json(self.label());
        let mac = cfg!(target_os = "macos");
        match self.editor {
            Editor::Code | Editor::Cursor => {
                let key = if mac { "cmd+alt+j" } else { "ctrl+alt+j" };
                format!(
                    "{{\n  \"key\": \"{key}\",\n  \"command\": \"workbench.action.tasks.runTask\",\n  \
                     \"args\": {label}\n}}"
                )
            }
            Editor::Zed => {
                let key = if mac { "cmd-alt-j" } else { "ctrl-alt-j" };
                format!(
                    "{{\n  \"context\": \"Workspace\",\n  \"bindings\": {{\n    \"{key}\": \
                     [\"task::Spawn\", {{ \"task_name\": {label} }}]\n  }}\n}}"
                )
            }
        }
    }

    /// How far the key is set up, for the zrush at `cli`.
    pub fn state(&self, cli: &Path) -> State {
        let tasks = read(&self.tasks_file()).unwrap_or_default();
        let keys = read(&self.keys_file()).unwrap_or_default();
        let toks = tokens(&tasks);
        let Some((start, end)) = object_with(&tasks, &toks, self.label()) else {
            return State::Missing;
        };
        if !has_string(&keys, self.label()) {
            return State::Missing;
        }
        let ours = parse(&self.task(cli));
        let theirs = parse(&tasks[start..=end]);
        let same = |k: &str| ours.as_ref().map(|o| &o[k]) == theirs.as_ref().map(|t| &t[k]);
        if same("command") && same("args") {
            State::Installed
        } else {
            State::Stale
        }
    }

    /// Write our task, running `cli`, and a key on it. A file that is
    /// changed is copied first, beside itself, as `<name>.zrush-bak`.
    pub fn install(&self, cli: &Path) -> Result<()> {
        let shown = cli.to_string_lossy();
        if shown.contains("/AppTranslocation/") || shown.starts_with("/Volumes/") {
            return Err(ZrushError::msg(
                "move zrush to Applications first: the key would stop working once the app moves",
            ));
        }
        std::fs::create_dir_all(&self.dir)?;
        let file = self.tasks_file();
        let text = read(&file).unwrap_or_default();
        let toks = tokens(&text);
        let task = self.task(cli);
        let new = if let Some((start, end)) = object_with(&text, &toks, self.label()) {
            let indent = line_indent(&text, start);
            format!(
                "{}{}{}",
                &text[..start],
                indented(&task, &indent),
                &text[end + 1..]
            )
        } else {
            let base = match self.editor {
                Editor::Code | Editor::Cursor => {
                    "{\n  \"version\": \"2.0.0\",\n  \"tasks\": []\n}\n"
                }
                Editor::Zed => "[]\n",
            };
            add_task(&text, base, &task, self.editor != Editor::Zed)
                .ok_or_else(|| unreadable(&file))?
        };
        write(&file, &text, &new)?;

        let file = self.keys_file();
        let text = read(&file).unwrap_or_default();
        if !has_string(&text, self.label()) {
            let base = if text.trim().is_empty() {
                "[]\n"
            } else {
                &text
            };
            let toks = tokens(base);
            let new = match toks.first() {
                Some((at, Tok::Open(b'['))) => append(base, &toks, 0, *at, &self.binding()),
                _ => None,
            }
            .ok_or_else(|| unreadable(&file))?;
            write(&file, &text, &new)?;
        }
        unquarantine(cli);
        Ok(())
    }
}

fn editor(name: &str) -> Option<Editor> {
    match name {
        "code" => Some(Editor::Code),
        "cursor" => Some(Editor::Cursor),
        "zed" => Some(Editor::Zed),
        _ => None,
    }
}

/// The zrush command line this app can name in a task: the copy shipped
/// beside it, or else the one on the `PATH`.
pub fn cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    cli_beside(&exe).or_else(on_path)
}

/// The command line beside the program at `exe`: `zrush-cli`, as the app
/// ships it, or `zrush` when that is not the program itself.
pub fn cli_beside(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let named = |n: &str| dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
    let shipped = named("zrush-cli");
    if shipped.is_file() {
        return Some(shipped);
    }
    let plain = named("zrush");
    (plain.is_file() && crate::paths::real(&plain) != crate::paths::real(exe)).then_some(plain)
}

fn on_path() -> Option<PathBuf> {
    let path = crate::search_path::get().or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path)
        .map(|d| d.join(format!("zrush{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
}

/// A command line from a downloaded app carries the download's quarantine,
/// and macOS would stop the editor running it. The app it came in is
/// running, so it was let through already.
fn unquarantine(cli: &Path) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(cli)
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn read(file: &Path) -> Option<String> {
    std::fs::read_to_string(file).ok()
}

fn write(file: &Path, before: &str, after: &str) -> Result<()> {
    if before == after {
        return Ok(());
    }
    if file.exists() {
        let mut bak = file.as_os_str().to_owned();
        bak.push(".zrush-bak");
        std::fs::copy(file, PathBuf::from(bak))?;
    }
    std::fs::write(file, after)?;
    Ok(())
}

fn unreadable(file: &Path) -> ZrushError {
    ZrushError::msg(format!(
        "{} is not laid out as expected; nothing was written to it",
        file.display()
    ))
}

fn json(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Our task added to `text`: in its `tasks` list when the file is an
/// object (adding the list when there is none), at its end when it is one.
fn add_task(text: &str, base: &str, task: &str, object: bool) -> Option<String> {
    let text = if text.trim().is_empty() { base } else { text };
    let toks = tokens(text);
    let (root_at, root) = *toks.first()?;
    if !object {
        return (root == Tok::Open(b'[')).then(|| append(text, &toks, 0, root_at, task))?;
    }
    if root != Tok::Open(b'{') {
        return None;
    }
    let close = matching(&toks, 0)?;
    // A `tasks` key of the top object: a string followed by a colon, one
    // level down.
    let mut depth = 0;
    for i in 1..close {
        match toks[i].1 {
            Tok::Open(_) => depth += 1,
            Tok::Close(_) => depth -= 1,
            Tok::Str(s, e)
                if depth == 0
                    && &text[s + 1..e - 1] == "tasks"
                    && toks.get(i + 1).map(|t| t.1) == Some(Tok::Colon) =>
            {
                let (at, t) = *toks.get(i + 2)?;
                return (t == Tok::Open(b'[')).then(|| append(text, &toks, i + 2, at, task))?;
            }
            _ => {}
        }
    }
    let list = format!("\"tasks\": [\n{}\n]", indented(task, "  "));
    append(text, &toks, 0, root_at, &list)
}

/// `item` added at the end of the list or object opened by the token at
/// `open` (at byte `at`), on a line of its own.
fn append(text: &str, toks: &[(usize, Tok)], open: usize, at: usize, item: &str) -> Option<String> {
    let close = matching(toks, open)?;
    let (prev_at, prev) = toks[close - 1];
    let prev_end = match prev {
        Tok::Str(_, e) => e,
        _ => prev_at + 1,
    };
    let comma = if matches!(prev, Tok::Open(_) | Tok::Comma) {
        ""
    } else {
        ","
    };
    let lead = line_indent(text, at);
    let step = if lead.is_empty() { "  " } else { lead.as_str() };
    let indent = format!("{lead}{step}");
    let (close_at, _) = toks[close];
    // Right after the last item, so a comment after it stays where it was;
    // the close goes on a line of its own when it had none.
    let rest = &text[prev_end..];
    let newline = if text[prev_end..close_at].contains('\n') {
        String::new()
    } else {
        format!("\n{lead}")
    };
    Some(format!(
        "{}{comma}\n{indent}{}{newline}{}",
        &text[..prev_end],
        indented(item, &indent),
        rest.trim_start_matches([' ', '\t'])
    ))
}

/// Every line of `s` after the first, moved right by `indent`.
fn indented(s: &str, indent: &str) -> String {
    s.replace('\n', &format!("\n{indent}"))
}

/// The spaces the line holding byte `at` starts with.
fn line_indent(text: &str, at: usize) -> String {
    let start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    text[start..at]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Whether `text` holds the string `s`, outside its comments.
fn has_string(text: &str, s: &str) -> bool {
    tokens(text)
        .iter()
        .any(|(_, t)| matches!(t, Tok::Str(a, b) if &text[a + 1..b - 1] == s))
}

/// The object a string `label` is a value of, as the bytes of its braces.
fn object_with(text: &str, toks: &[(usize, Tok)], label: &str) -> Option<(usize, usize)> {
    let i = toks.iter().enumerate().position(|(i, (_, t))| {
        matches!(t, Tok::Str(a, b) if &text[a + 1..b - 1] == label)
            && i > 0
            && toks[i - 1].1 == Tok::Colon
    })?;
    let mut depth = 0;
    let open = (0..i).rev().find(|&j| match toks[j].1 {
        Tok::Close(_) => {
            depth += 1;
            false
        }
        Tok::Open(b) => {
            if depth == 0 {
                b == b'{'
            } else {
                depth -= 1;
                false
            }
        }
        _ => false,
    })?;
    let close = matching(toks, open)?;
    Some((toks[open].0, toks[close].0))
}

/// The token closing the one opened at `open`.
fn matching(toks: &[(usize, Tok)], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, (_, t)) in toks.iter().enumerate().skip(open) {
        match t {
            Tok::Open(_) => depth += 1,
            Tok::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A piece of JSON with comments, read as JSON: comments and the commas
/// that close a list dropped.
fn parse(text: &str) -> Option<serde_json::Value> {
    let toks = tokens(text);
    let mut out = String::new();
    for (i, (at, t)) in toks.iter().enumerate() {
        match t {
            Tok::Str(a, b) => out.push_str(&text[*a..*b]),
            Tok::Comma if matches!(toks.get(i + 1), Some((_, Tok::Close(_)))) => {}
            _ => out.push(char::from(text.as_bytes()[*at])),
        }
    }
    serde_json::from_str(&out).ok()
}

/// What a piece of JSON with comments is made of, comments left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Open(u8),
    Close(u8),
    Comma,
    Colon,
    /// A string, quotes included: its first byte and the one after it.
    Str(usize, usize),
    /// Any other byte of a number, `true`, `false` or `null`.
    Other,
}

fn tokens(text: &str) -> Vec<(usize, Tok)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                out.push((start, Tok::Str(start, i)));
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            c @ (b'{' | b'[') => out.push((i, Tok::Open(c))),
            c @ (b'}' | b']') => out.push((i, Tok::Close(c))),
            b',' => out.push((i, Tok::Comma)),
            b':' => out.push((i, Tok::Colon)),
            c if c.is_ascii_whitespace() => {}
            _ => out.push((i, Tok::Other)),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLI: &str = "/Applications/zrush.app/Contents/MacOS/zrush-cli";

    fn setup(name: &str) -> (tempfile::TempDir, Shortcut) {
        let td = tempfile::TempDir::new().unwrap();
        let s = Shortcut::in_dir(name, td.path()).unwrap();
        (td, s)
    }

    fn file(td: &tempfile::TempDir, name: &str) -> String {
        std::fs::read_to_string(td.path().join(name)).unwrap()
    }

    #[test]
    fn nothing_there_gets_a_task_and_a_key() {
        let (td, s) = setup("code");
        assert_eq!(s.state(Path::new(CLI)), State::Missing);
        s.install(Path::new(CLI)).unwrap();
        assert_eq!(s.state(Path::new(CLI)), State::Installed);
        let tasks = parse(&file(&td, "tasks.json")).unwrap();
        assert_eq!(tasks["tasks"][0]["command"], CLI);
        assert_eq!(tasks["tasks"][0]["args"][2], "code");
        let keys = parse(&file(&td, "keybindings.json")).unwrap();
        assert_eq!(keys[0]["args"], "zrush: claude tab");
    }

    #[test]
    fn a_task_written_by_hand_is_replaced_and_the_rest_kept() {
        let (td, s) = setup("cursor");
        std::fs::write(
            td.path().join("tasks.json"),
            r#"{
  // mine
  "version": "2.0.0",
  "tasks": [
    {
      // zrush resolves the pair itself
      "label": "zrush: claude tab",
      "type": "shell",
      "command": "zrush tab --editor cursor",
      "problemMatcher": []
    },
    {
      "label": "zrush: worktrees",
      "type": "shell",
      "command": "zrush", // the picker
    },
  ]
}
"#,
        )
        .unwrap();
        std::fs::write(
            td.path().join("keybindings.json"),
            r#"[
    { "key": "cmd+alt+j", "command": "workbench.action.tasks.runTask", "args": "zrush: claude tab" }
]"#,
        )
        .unwrap();
        assert_eq!(s.state(Path::new(CLI)), State::Stale);
        s.install(Path::new(CLI)).unwrap();
        assert_eq!(s.state(Path::new(CLI)), State::Installed);
        let text = file(&td, "tasks.json");
        assert!(
            text.contains("// mine") && text.contains("// the picker"),
            "{text}"
        );
        let tasks = parse(&text).unwrap();
        assert_eq!(tasks["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(tasks["tasks"][0]["args"][2], "cursor");
        assert_eq!(tasks["tasks"][1]["command"], "zrush");
        // The key was there already: left as it was, and no copy made.
        assert!(!td.path().join("keybindings.json.zrush-bak").exists());
        assert!(td.path().join("tasks.json.zrush-bak").exists());
    }

    #[test]
    fn other_tasks_and_keys_get_ours_beside_them() {
        let (td, s) = setup("code");
        std::fs::write(
            td.path().join("tasks.json"),
            "{\n  \"version\": \"2.0.0\",\n  \"tasks\": [\n    { \"label\": \"build\", \"command\": \"make\" } // last\n  ]\n}\n",
        )
        .unwrap();
        std::fs::write(
            td.path().join("keybindings.json"),
            "// Place your key bindings in this file\n[\n    { \"key\": \"f5\", \"command\": \"x\" }\n]\n",
        )
        .unwrap();
        s.install(Path::new(CLI)).unwrap();
        let tasks = parse(&file(&td, "tasks.json")).unwrap();
        assert_eq!(tasks["tasks"][0]["label"], "build");
        assert_eq!(tasks["tasks"][1]["label"], "zrush: claude tab");
        let keys = parse(&file(&td, "keybindings.json")).unwrap();
        assert_eq!(keys[0]["key"], "f5");
        assert_eq!(keys[1]["args"], "zrush: claude tab");
        // Twice is once.
        let before = file(&td, "tasks.json");
        s.install(Path::new(CLI)).unwrap();
        assert_eq!(file(&td, "tasks.json"), before);
    }

    #[test]
    fn a_tasks_file_without_a_list_gets_one() {
        let (td, s) = setup("code");
        std::fs::write(td.path().join("tasks.json"), "{ \"version\": \"2.0.0\" }").unwrap();
        s.install(Path::new(CLI)).unwrap();
        let tasks = parse(&file(&td, "tasks.json")).unwrap();
        assert_eq!(tasks["version"], "2.0.0");
        assert_eq!(tasks["tasks"][0]["command"], CLI);
    }

    #[test]
    fn an_empty_list_gets_no_stray_comma() {
        let (td, s) = setup("code");
        std::fs::write(td.path().join("keybindings.json"), "[]").unwrap();
        s.install(Path::new(CLI)).unwrap();
        let text = file(&td, "keybindings.json");
        assert!(
            serde_json::from_str::<serde_json::Value>(&text).is_ok(),
            "{text}"
        );
    }

    #[test]
    fn zed_gets_a_session_task_in_its_own_files() {
        let (td, s) = setup("zed");
        s.install(Path::new(CLI)).unwrap();
        let tasks = parse(&file(&td, "tasks.json")).unwrap();
        assert_eq!(tasks[0]["args"][0], "session");
        assert_eq!(tasks[0]["cwd"], "$ZED_WORKTREE_ROOT");
        let keys = parse(&file(&td, "keymap.json")).unwrap();
        assert_eq!(keys[0]["context"], "Workspace");
        assert_eq!(s.state(Path::new(CLI)), State::Installed);
    }

    #[test]
    fn a_path_with_spaces_and_backslashes_survives() {
        let (td, s) = setup("code");
        let cli = Path::new(r"C:\Program Files\zrush\zrush-cli.exe");
        s.install(cli).unwrap();
        let tasks = parse(&file(&td, "tasks.json")).unwrap();
        assert_eq!(
            tasks["tasks"][0]["command"],
            r"C:\Program Files\zrush\zrush-cli.exe"
        );
    }

    #[test]
    fn an_app_still_on_its_disk_image_is_refused() {
        let (td, s) = setup("code");
        assert!(
            s.install(Path::new(
                "/Volumes/zrush/zrush.app/Contents/MacOS/zrush-cli"
            ))
            .is_err()
        );
        assert!(!td.path().join("tasks.json").exists());
    }

    #[test]
    fn a_file_that_is_not_a_list_is_left_alone() {
        let (td, s) = setup("zed");
        std::fs::write(td.path().join("tasks.json"), "{ \"odd\": 1 }").unwrap();
        assert!(s.install(Path::new(CLI)).is_err());
        assert_eq!(file(&td, "tasks.json"), "{ \"odd\": 1 }");
    }

    #[test]
    fn the_shipped_command_line_is_found_beside_the_app() {
        let td = tempfile::TempDir::new().unwrap();
        let exe = td.path().join("zrush");
        std::fs::write(&exe, "").unwrap();
        // The app itself is no command line.
        assert_eq!(cli_beside(&exe), None);
        let shipped = td
            .path()
            .join(format!("zrush-cli{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&shipped, "").unwrap();
        assert_eq!(cli_beside(&exe), Some(shipped));
    }

    #[test]
    fn an_editor_without_tasks_has_no_key() {
        assert!(Shortcut::for_editor("vim").is_none());
    }
}
