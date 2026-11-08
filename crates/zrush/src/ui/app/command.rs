//! The `:` command bar.

use super::{Action, App, Modal};

impl App {
    pub(super) fn run_command(&mut self, value: &str) -> Action {
        let mut words = value.split_whitespace();
        match (words.next(), words.next()) {
            (Some("q" | "quit"), _) => Action::Quit,
            (Some("help" | "h"), _) => {
                self.modal = Some(Modal::Help);
                Action::Redraw
            }
            (Some("agent" | "a"), Some(name)) => Action::SwitchAgent(name.to_string()),
            // `:agent` on its own offers the list rather than doing nothing.
            (Some("agent" | "a"), None) => {
                self.open_agent_picker(None);
                Action::Redraw
            }
            // `:editor code` names one; `:editor code --profile x` is a
            // whole command line, which is a different setting.
            (Some("editor" | "e"), Some(_)) => {
                let rest: Vec<String> = value
                    .split_whitespace()
                    .skip(1)
                    .map(str::to_string)
                    .collect();
                match rest.len() {
                    1 => Action::SetEditor(rest[0].clone()),
                    _ => Action::SetEditorCommand(rest),
                }
            }
            (Some("settings"), _) => {
                self.open_settings();
                Action::Redraw
            }
            (Some("set"), Some(key)) => {
                let value = value
                    .split_whitespace()
                    .skip(2)
                    .collect::<Vec<_>>()
                    .join(" ");
                Action::SetSetting {
                    key: key.to_string(),
                    value,
                }
            }
            (Some("editor" | "e"), None) => {
                self.open_editor_picker();
                Action::Redraw
            }
            (Some(other), _) => {
                self.flash(format!("unknown command: {other}"));
                Action::Redraw
            }
            (None, _) => Action::Redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn set_from_the_command_bar_takes_the_rest_of_the_line() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "set terminal open -a Ghostty".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetSetting {
                key: "terminal".into(),
                value: "open -a Ghostty".into(),
            }
        );
    }

    #[test]
    fn editor_with_one_word_names_an_editor() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "editor code".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditor("code".into())
        );
    }

    #[test]
    fn editor_with_arguments_is_a_whole_command_line() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "editor code --reuse-window".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditorCommand(vec!["code".into(), "--reuse-window".into()])
        );
    }

    #[test]
    fn editor_on_its_own_offers_the_list_with_the_current_one_under_the_cursor() {
        let mut a = app();
        a.editor = "code".into();
        a.on_key(key(':'));
        for c in "editor".chars() {
            a.on_key(key(c));
        }
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::EditorPicker { editors, at } => {
                assert_eq!(editors.last().map(String::as_str), Some(CUSTOM));
                assert_eq!(editors[*at], "code");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn the_command_bar_quits() {
        let mut a = app();
        a.on_key(key(':'));
        a.on_key(key('q'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Quit);
    }

    #[test]
    fn the_command_bar_switches_agent() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "agent codex".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SwitchAgent("codex".into())
        );
    }

    #[test]
    fn an_unknown_command_says_so_rather_than_doing_something() {
        let mut a = app();
        a.on_key(key(':'));
        a.on_key(key('z'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        assert!(a.flash.unwrap().contains("unknown command"));
    }
}
