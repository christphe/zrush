//! What a key does: the normal list, the filter, and any modal that is up.

use super::{
    Action, App, BranchKind, CUSTOM, EDITOR_COMMAND, EXISTING_BRANCH, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, Modal, Mode, Pending,
};

impl App {
    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        // Windows reports every key twice, going down and coming back up.
        // Only the first is a key press; a held key repeats and still counts.
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        // A modal owns the keyboard while it is up. No key means two things.
        if self.modal.is_some() {
            return self.modal_key(key);
        }
        match self.mode {
            Mode::Filter => self.filter_key(key),
            Mode::Purge => self.purge_key(key),
            Mode::Normal => self.normal_key(key),
        }
    }

    pub(super) fn normal_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('c' | 'q'), true) | (KeyCode::Esc, _) => Action::Quit,
            (KeyCode::Up | KeyCode::Char('k'), false) => {
                self.move_by(-1);
                Action::Redraw
            }
            (KeyCode::Down | KeyCode::Char('j'), false) => {
                self.move_by(1);
                Action::Redraw
            }
            (KeyCode::Left | KeyCode::Char('h'), false) => self.fold(true),
            (KeyCode::Right | KeyCode::Char('l'), false) => self.fold(false),
            (KeyCode::Char('l'), true) => Action::Reload,
            (KeyCode::Enter, _) => self.enter(),
            // On a session row this resumes THAT conversation. It used to
            // pass None whatever the cursor was on, so a session row
            // started a fresh one — the core could resume all along and
            // nothing ever asked it to.
            (KeyCode::Char('o'), true) => {
                self.worktree_at_cursor()
                    .map_or(Action::None, |worktree| Action::RunAgent {
                        worktree,
                        session: self.session_at_cursor(),
                    })
            }
            (KeyCode::Char('w'), true) => {
                self.pending = Pending {
                    session: self.session_at_cursor(),
                    worktree: None,
                    at: None,
                    question: None,
                };
                self.ask_branch_kind();
                Action::Redraw
            }
            (KeyCode::Char(','), false) => {
                self.open_settings();
                Action::Redraw
            }
            (KeyCode::Char('d'), true) => self.ask_delete(),
            (KeyCode::Char('p'), true) => {
                // Marking rows you cannot see would be a trap: unfold first.
                self.collapsed.clear();
                self.show_all = true;
                self.mode = Mode::Purge;
                self.marked.clear();
                Action::ReloadUncapped
            }
            (KeyCode::Char('/'), false) => {
                self.mode = Mode::Filter;
                Action::Redraw
            }
            (KeyCode::Char(':'), false) => {
                self.modal = Some(Modal::Command {
                    value: String::new(),
                });
                Action::Redraw
            }
            (KeyCode::Char('?'), false) => {
                self.modal = Some(Modal::Help);
                Action::Redraw
            }
            (KeyCode::Char('U'), false) => self.ask_update(),
            _ => Action::None,
        }
    }

    pub(super) fn filter_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
                self.refilter();
                Action::Redraw
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                Action::Redraw
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.refilter();
                Action::Redraw
            }
            KeyCode::Up => {
                self.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down => {
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.refilter();
                Action::Redraw
            }
            _ => Action::None,
        }
    }

    pub(super) fn modal_key(&mut self, key: KeyEvent) -> Action {
        let Some(modal) = self.modal.as_mut() else {
            return Action::None;
        };
        // An error box has one answer: it goes away.
        if matches!(modal, Modal::Error { .. }) {
            self.modal = None;
            return Action::Redraw;
        }
        match key.code {
            KeyCode::Esc => {
                self.settings_open = false;
                self.pending = Pending::default();
                self.modal = None;
                Action::Redraw
            }
            KeyCode::Up => {
                modal.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down => {
                modal.move_by(1);
                Action::Redraw
            }
            KeyCode::Backspace => {
                match modal {
                    Modal::Input { value, .. } | Modal::Command { value } => {
                        value.pop();
                    }
                    _ => {}
                }
                self.refilter_suggestions();
                Action::Redraw
            }
            KeyCode::Char(c) => {
                match modal {
                    Modal::Input { value, .. } | Modal::Command { value } => {
                        value.push(c);
                        self.refilter_suggestions();
                        return Action::Redraw;
                    }
                    Modal::Help => {
                        self.modal = None;
                        return Action::Redraw;
                    }
                    _ => {}
                }
                Action::None
            }
            KeyCode::Enter => self.modal_enter(),
            _ => Action::None,
        }
    }

    pub(super) fn modal_enter(&mut self) -> Action {
        let Some(modal) = self.modal.take() else {
            return Action::None;
        };
        match modal {
            Modal::Help | Modal::Error { .. } => Action::Redraw,
            Modal::Command { value } => self.run_command(&value),
            Modal::AgentPicker { agents, at, .. } => agents
                .get(at)
                .map_or(Action::Redraw, |a| Action::SwitchAgent(a.clone())),
            Modal::Input { title, value, .. }
                if zrush_core::config::SETTINGS
                    .iter()
                    .any(|(k, _)| *k == title) =>
            {
                Action::SetSetting { key: title, value }
            }
            Modal::Input { title, value, .. } if title == EDITOR_COMMAND => {
                let words: Vec<String> = value.split_whitespace().map(str::to_string).collect();
                if words.is_empty() {
                    return Action::Redraw;
                }
                Action::SetEditorCommand(words)
            }
            Modal::Input {
                title,
                value,
                suggestions,
                at,
                ..
            } => {
                let existing = title == EXISTING_BRANCH;
                // Under "existing", the highlighted branch wins: the list
                // is the answer. Under "new", what was typed is the whole
                // point and there is no list.
                let name = if existing {
                    suggestions.get(at).cloned().unwrap_or(value)
                } else {
                    value
                };
                if name.trim().is_empty() {
                    return Action::Redraw;
                }
                // On a session row, ctrl-w hands that session to the new
                // worktree; that is the one thing the removed "+ new
                // worktree" row could never do.
                Action::CreateWorktree {
                    name,
                    kind: if existing {
                        BranchKind::Existing
                    } else {
                        BranchKind::New
                    },
                    at: self.pending.at.take(),
                    hand_over: self.pending.session.clone(),
                }
            }
            Modal::Settings { at, .. } => self.edit_setting(at),
            Modal::EditorPicker { editors, at } => match editors.get(at).map(String::as_str) {
                None => Action::Redraw,
                // Not an editor: the row that asks for a command line.
                Some(CUSTOM) => {
                    self.modal = Some(Modal::Input {
                        title: EDITOR_COMMAND.into(),
                        prompt: "cmd>".into(),
                        value: self.editor.clone(),
                        suggestions: Vec::new(),
                        at: 0,
                    });
                    Action::Redraw
                }
                Some(e) => Action::SetEditor(e.to_string()),
            },
            Modal::Confirm {
                title, choices, at, ..
            } => self.confirmed(&title, &choices, at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn a_modal_owns_the_keyboard_while_it_is_up() {
        let mut a = app();
        a.on_key(key('?'));
        // j would move the cursor in normal mode; here it closes the help.
        let before = a.cursor;
        a.on_key(key('j'));
        assert_eq!(a.cursor, before);
    }

    #[test]
    fn escape_closes_a_modal_rather_than_quitting() {
        let mut a = app();
        a.on_key(key('?'));
        assert_eq!(a.on_key(code(KeyCode::Esc)), Action::Redraw);
        assert!(a.modal.is_none());
        // Now it quits.
        assert_eq!(a.on_key(code(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn an_error_box_goes_away_on_any_key() {
        let mut a = app();
        a.error("Remove worktree", "fatal: it contains untracked files");
        assert!(matches!(a.modal, Some(Modal::Error { .. })));
        assert_eq!(a.on_key(key('j')), Action::Redraw);
        assert!(a.modal.is_none());
        // And it did not move the cursor on the way out.
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn typing_in_filter_mode_narrows_the_list() {
        let mut a = app();
        a.on_key(key('/'));
        assert_eq!(a.mode, Mode::Filter);
        a.on_key(key('x'));
        assert!(a.visible.len() < a.rows.len());
    }

    #[test]
    fn escaping_the_filter_restores_every_row() {
        let mut a = app();
        a.on_key(key('/'));
        a.on_key(key('x'));
        a.on_key(code(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.visible.len(), a.rows.len());
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let mut a = app();
        a.on_key(key('/'));
        a.on_key(key('x'));
        let narrow = a.visible.len();
        a.on_key(code(KeyCode::Backspace));
        assert!(a.visible.len() > narrow);
    }

    #[test]
    fn ctrl_o_on_a_worktree_starts_a_fresh_session() {
        let mut a = app();
        assert_eq!(
            a.on_key(ctrl('o')),
            Action::RunAgent {
                worktree: "/repo".into(),
                session: None,
            }
        );
    }

    /// The core could resume from the start; the interface never asked, so
    /// ctrl-o on a session row opened a new conversation instead of the
    /// one under the cursor.
    #[test]
    fn ctrl_o_on_a_session_resumes_that_one_in_its_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(ctrl('o')),
            Action::RunAgent {
                worktree: "/repo".into(),
                session: Some(("claude".into(), "dead".into())),
            }
        );
    }
}
