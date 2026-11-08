//! The pickers and inputs: settings, editor, agent, a new worktree's branch.

use super::{Action, App, CUSTOM, EXISTING_BRANCH, Modal, NEW_BRANCH};

impl App {
    /// The agent question: at startup when nothing settled it, and any
    /// time through `:agent`.
    pub fn open_agent_picker(&mut self, reason: Option<String>) {
        let at = self
            .agent_names
            .iter()
            .position(|a| *a == self.active_agent)
            .unwrap_or(0);
        self.modal = Some(Modal::AgentPicker {
            agents: self.agent_names.clone(),
            at,
            reason,
        });
    }

    /// Every setting and what it is now. Rebuilt from the config each time
    /// rather than held: a change goes through the file, and this is what
    /// came back from it.
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.modal = Some(Modal::Settings {
            rows: zrush_core::config::SETTINGS
                .iter()
                .map(|(key, _)| ((*key).to_string(), self.cfg.show(key)))
                .collect(),
            at: self.settings_at,
        });
    }

    /// Enter on a settings row, by what that row takes.
    pub(super) fn edit_setting(&mut self, at: usize) -> Action {
        self.settings_at = at;
        let Some((key, kind)) = zrush_core::config::SETTINGS.get(at) else {
            return Action::Redraw;
        };
        match kind {
            zrush_core::config::Kind::Flag => Action::ToggleSetting((*key).to_string()),
            zrush_core::config::Kind::Editor => {
                self.open_editor_picker();
                Action::Redraw
            }
            zrush_core::config::Kind::Agent => {
                self.open_agent_picker(None);
                Action::Redraw
            }
            _ => {
                self.modal = Some(Modal::Input {
                    title: (*key).to_string(),
                    prompt: format!("{key}>"),
                    value: self.cfg.show(key),
                    suggestions: Vec::new(),
                    at: 0,
                });
                Action::Redraw
            }
        }
    }

    /// The branch input, in whichever of its two meanings. The suggestion
    /// list is the point of the second one, so it starts full rather than
    /// waiting for the first keystroke.
    pub(super) fn open_branch_input(&mut self, existing: bool) {
        let suggestions = if existing {
            self.snap.branches.clone()
        } else {
            Vec::new()
        };
        self.modal = Some(Modal::Input {
            title: if existing {
                EXISTING_BRANCH
            } else {
                NEW_BRANCH
            }
            .into(),
            prompt: "branch>".into(),
            value: String::new(),
            suggestions,
            at: 0,
        });
    }

    /// Narrow the branch list to what has been typed. Substring, not
    /// prefix: branches are named `fix/2358-thing` and nobody types the
    /// prefix they already know.
    pub(super) fn refilter_suggestions(&mut self) {
        let all = self.snap.branches.clone();
        let Some(Modal::Input {
            title,
            value,
            suggestions,
            at,
            ..
        }) = self.modal.as_mut()
        else {
            return;
        };
        if title != EXISTING_BRANCH {
            return;
        }
        let needle = value.to_lowercase();
        *suggestions = all
            .into_iter()
            .filter(|b| b.to_lowercase().contains(&needle))
            .collect();
        *at = 0;
    }

    /// The editors that get a `-n`, plus the way out: type the command.
    pub fn open_editor_picker(&mut self) {
        let mut editors: Vec<String> = zrush_core::config::KNOWN_EDITORS
            .iter()
            .map(|e| (*e).to_string())
            .collect();
        let at = editors.iter().position(|e| *e == self.editor).unwrap_or(0);
        editors.push(CUSTOM.to_string());
        self.modal = Some(Modal::EditorPicker { editors, at });
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn a_comma_opens_the_settings_screen_with_the_live_values() {
        let mut a = app();
        a.cfg.editor = "code".into();
        a.cfg.resumable_max = 9;
        a.on_key(key(','));
        match a.modal.as_ref().unwrap() {
            Modal::Settings { rows, at } => {
                assert_eq!(*at, 0);
                assert_eq!(rows.len(), zrush_core::config::SETTINGS.len());
                assert!(rows.contains(&("editor".into(), "code".into())));
                assert!(rows.contains(&("resumable_max".into(), "9".into())));
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_flag_is_toggled_in_place_rather_than_typed() {
        let mut a = app();
        a.on_key(key(','));
        // titles is the fifth row.
        for _ in 0..4 {
            a.on_key(code(KeyCode::Down));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::ToggleSetting("titles".into())
        );
    }

    #[test]
    fn a_number_is_typed_and_comes_back_under_its_own_name() {
        let mut a = app();
        a.cfg.resumable_max = 5;
        a.on_key(key(','));
        for _ in 0..6 {
            a.on_key(code(KeyCode::Down));
        }
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            // The current value is there to edit, not an empty field.
            Modal::Input { title, value, .. } => {
                assert_eq!(title, "resumable_max");
                assert_eq!(value, "5");
            }
            other => panic!("wrong modal: {other:?}"),
        }
        a.on_key(code(KeyCode::Backspace));
        a.on_key(key('8'));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetSetting {
                key: "resumable_max".into(),
                value: "8".into(),
            }
        );
    }

    #[test]
    fn the_editor_row_opens_the_picker_that_also_takes_a_command() {
        let mut a = app();
        a.on_key(key(','));
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::EditorPicker { editors, .. } => {
                assert_eq!(editors.last().map(String::as_str), Some(CUSTOM));
            }
            other => panic!("wrong modal: {other:?}"),
        }
        assert!(a.settings_open, "the change should come back to settings");
    }

    #[test]
    fn escape_leaves_the_settings_screen_for_good() {
        let mut a = app();
        a.on_key(key(','));
        a.on_key(code(KeyCode::Esc));
        assert!(!a.settings_open);
        assert!(a.modal.is_none());
    }

    #[test]
    fn the_custom_row_asks_for_a_command_instead_of_settling() {
        let mut a = app();
        a.open_editor_picker();
        // Up from the first row wraps onto the last, which is the custom one.
        a.on_key(code(KeyCode::Up));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::Input { title, .. } => assert_eq!(title, EDITOR_COMMAND),
            other => panic!("wrong modal: {other:?}"),
        }
        for c in "hx -n".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditorCommand(vec!["hx".into(), "-n".into()])
        );
    }

    #[test]
    fn an_empty_editor_command_changes_nothing() {
        let mut a = app();
        a.open_editor_picker();
        a.on_key(code(KeyCode::Up));
        a.on_key(code(KeyCode::Enter));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
    }

    #[test]
    fn ctrl_w_on_a_session_hands_it_to_the_new_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        ask_new_branch(&mut a);
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "feat".into(),
                kind: BranchKind::New,
                at: None,
                hand_over: Some(("claude".into(), "dead".into())),
            }
        );
    }

    #[test]
    fn an_empty_branch_name_creates_nothing() {
        let mut a = app();
        ask_new_branch(&mut a);
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
    }

    #[test]
    fn ctrl_w_asks_which_kind_of_branch_before_anything_is_typed() {
        let mut a = app();
        a.on_key(ctrl('w'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm {
                title, choices, at, ..
            } => {
                assert_eq!(title, "New worktree");
                assert_eq!(choices[*at].label, NEW_BRANCH);
                assert_eq!(choices[1].label, EXISTING_BRANCH);
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_new_branch_gets_no_suggestions_to_be_mistaken_for_a_choice() {
        let mut a = app();
        a.snap.branches = vec!["main".into(), "feat".into()];
        ask_new_branch(&mut a);
        match a.modal.as_ref().unwrap() {
            Modal::Input {
                title, suggestions, ..
            } => {
                assert_eq!(title, NEW_BRANCH);
                assert_eq!(*suggestions, Vec::<String>::new());
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn an_existing_branch_offers_the_list_and_narrows_it_as_you_type() {
        let mut a = app();
        a.snap.branches = vec!["main".into(), "fix/2358-reader".into(), "feat".into()];
        a.on_key(ctrl('w'));
        a.on_key(code(KeyCode::Down));
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::Input {
                title, suggestions, ..
            } => {
                assert_eq!(title, EXISTING_BRANCH);
                assert_eq!(suggestions.len(), 3);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        // Substring, not prefix: nobody types the part of the name they
        // already know.
        for c in "2358".chars() {
            a.on_key(key(c));
        }
        match a.modal.as_ref().unwrap() {
            Modal::Input { suggestions, .. } => {
                assert_eq!(suggestions, &vec!["fix/2358-reader".to_string()]);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "fix/2358-reader".into(),
                kind: BranchKind::Existing,
                at: None,
                hand_over: None,
            }
        );
    }

    #[test]
    fn cancelling_the_kind_question_opens_no_input() {
        let mut a = app();
        a.on_key(ctrl('w'));
        a.on_key(code(KeyCode::Up)); // onto Cancel
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        assert!(a.modal.is_none());
    }
}

#[cfg(test)]
mod agent_tests {
    use super::super::*;

    fn chr(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn the_command_bar_with_no_name_offers_the_list() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into(), "codex".into()];
        a.active_agent = "codex".into();
        a.on_key(chr(':'));
        a.on_key(chr('a'));
        a.on_key(key(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::AgentPicker { agents, at, .. } => {
                assert_eq!(agents.len(), 2);
                assert_eq!(*at, 1, "the cursor starts on the active agent");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn picking_from_the_list_switches() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into(), "codex".into()];
        a.active_agent = "claude".into();
        a.open_agent_picker(None);
        a.on_key(key(KeyCode::Down));
        assert_eq!(
            a.on_key(key(KeyCode::Enter)),
            Action::SwitchAgent("codex".into())
        );
    }

    #[test]
    fn the_picker_can_say_why_it_is_asking() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into()];
        a.open_agent_picker(Some("codex is configured but not installed here".into()));
        match a.modal.as_ref().unwrap() {
            Modal::AgentPicker { reason, .. } => {
                assert!(reason.as_ref().unwrap().contains("not installed"));
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }
}
