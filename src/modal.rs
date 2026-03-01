use crossterm::event::{KeyCode, KeyModifiers};
use std::fmt;

/// The modal modes for the Helix-style editor UI.
///
/// The mode system is designed to be extensible — adding a new mode requires:
/// 1. Add variant here
/// 2. Add transition rules in `ModeStateMachine::handle_key`
/// 3. Add keymap section in `keymap.toml`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Navigate messages, execute single-key commands (y, d, R, e, etc.)
    #[default]
    Normal,
    /// Text selection within a message buffer
    Visual,
    /// Text input (composing/editing messages)
    Insert,
    /// Leader-key submenu (space pressed, waiting for next key)
    Space,
    /// Picker is open (fuzzy search + selection)
    Picker,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mode::Normal => write!(f, "NOR"),
            Mode::Visual => write!(f, "VIS"),
            Mode::Insert => write!(f, "INS"),
            Mode::Space => write!(f, "SPC"),
            Mode::Picker => write!(f, "PKR"),
        }
    }
}

/// Result of a key press in the modal state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeTransition {
    /// Mode changed to a new mode.
    ModeChanged(Mode),
    /// Stay in the current mode. The key should be handled by the current
    /// mode's keymap (normal command dispatch).
    Stay,
    /// The key was consumed by the state machine itself (e.g. space menu
    /// subcommand). Contains the action string to execute.
    ConsumedWithAction(String),
}

/// The modal state machine tracks the current mode and handles transitions.
///
/// It does NOT own keymaps — it only decides mode transitions. The actual
/// key→action mapping is done by the keymap system using the current mode.
#[derive(Debug, Clone)]
pub struct ModeStateMachine {
    mode: Mode,
}

impl Default for ModeStateMachine {
    fn default() -> Self {
        Self { mode: Mode::Normal }
    }
}

impl ModeStateMachine {
    /// Create a new state machine starting in Normal mode.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Force-set the mode (used when actions externally trigger mode changes,
    /// e.g. `Action::SetMode`).
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Handle a key event and determine the mode transition.
    ///
    /// This function only handles *mode-switching* keys that are universal
    /// across the state machine (e.g. `Esc` to return to Normal, `i` to enter
    /// Insert, `v` to enter Visual, `Space` to enter Space menu).
    ///
    /// Keys that are mode-specific commands (e.g. `y` in Normal mode to copy)
    /// are NOT handled here — they return `ModeTransition::Stay` and are
    /// dispatched by the keymap system.
    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> ModeTransition {
        match self.mode {
            Mode::Normal => self.handle_normal(key, modifiers),
            Mode::Visual => self.handle_visual(key, modifiers),
            Mode::Insert => self.handle_insert(key, modifiers),
            Mode::Space => self.handle_space(key, modifiers),
            Mode::Picker => self.handle_picker(key, modifiers),
        }
    }

    // ─── Normal mode transitions ───────────────────────────────────────

    fn handle_normal(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ModeTransition {
        match key {
            KeyCode::Char('i') => {
                self.mode = Mode::Insert;
                ModeTransition::ModeChanged(Mode::Insert)
            }
            KeyCode::Char('o') => {
                self.mode = Mode::Insert;
                ModeTransition::ConsumedWithAction("chat_window_open_draft".to_string())
            }
            KeyCode::Char('v') => {
                self.mode = Mode::Visual;
                ModeTransition::ModeChanged(Mode::Visual)
            }
            KeyCode::Char(' ') if _modifiers.is_empty() => {
                self.mode = Mode::Space;
                ModeTransition::ModeChanged(Mode::Space)
            }
            // All other keys are handled by the Normal mode keymap
            _ => ModeTransition::Stay,
        }
    }

    // ─── Visual mode transitions ───────────────────────────────────────

    fn handle_visual(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ModeTransition {
        match key {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                ModeTransition::ModeChanged(Mode::Normal)
            }
            // All other keys extend selection or execute visual commands
            KeyCode::Char(' ') if _modifiers.is_empty() => {
                self.mode = Mode::Space;
                ModeTransition::ModeChanged(Mode::Space)
            }
            _ => ModeTransition::Stay,
        }
    }

    // ─── Insert mode transitions ───────────────────────────────────────

    fn handle_insert(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ModeTransition {
        match key {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                ModeTransition::ModeChanged(Mode::Normal)
            }
            // All other keys are text input, handled by the prompt
            _ => ModeTransition::Stay,
        }
    }

    // ─── Space mode transitions (leader key submenu) ───────────────────

    fn handle_space(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ModeTransition {
        match key {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                ModeTransition::ModeChanged(Mode::Normal)
            }
            KeyCode::Char('b') => {
                self.mode = Mode::Picker;
                ModeTransition::ConsumedWithAction("open_picker_active_chats".to_string())
            }
            KeyCode::Char('f') => {
                self.mode = Mode::Picker;
                ModeTransition::ConsumedWithAction("open_picker_all_chats".to_string())
            }
            KeyCode::Char('/') => {
                // Server-side message search
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("show_search_overlay".to_string())
            }
            KeyCode::Char('t') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("show_theme_selector".to_string())
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("show_command_guide".to_string())
            }
            KeyCode::Char('w') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("forward_message".to_string())
            }
            KeyCode::Char('o') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("jump_to_forward_origin".to_string())
            }
            KeyCode::Char('q') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("try_quit".to_string())
            }
            KeyCode::Char('y') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("copy_visual_selection".to_string())
            }
            KeyCode::Char('p') => {
                self.mode = Mode::Normal;
                ModeTransition::ConsumedWithAction("paste_from_clipboard".to_string())
            }
            // Unknown space command — return to Normal
            _ => {
                self.mode = Mode::Normal;
                ModeTransition::ModeChanged(Mode::Normal)
            }
        }
    }

    // ─── Picker mode transitions ───────────────────────────────────────

    fn handle_picker(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> ModeTransition {
        match key {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                ModeTransition::ModeChanged(Mode::Normal)
            }
            // All other keys are handled by the picker component (fuzzy input, navigation)
            _ => ModeTransition::Stay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn empty_mods() -> KeyModifiers {
        KeyModifiers::empty()
    }

    #[test]
    fn test_default_mode_is_normal() {
        let sm = ModeStateMachine::new();
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_normal_to_insert_on_i() {
        let mut sm = ModeStateMachine::new();
        let result = sm.handle_key(KeyCode::Char('i'), empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Insert));
        assert_eq!(sm.mode(), Mode::Insert);
    }

    #[test]
    fn test_normal_to_insert_on_o() {
        let mut sm = ModeStateMachine::new();
        let result = sm.handle_key(KeyCode::Char('o'), empty_mods());
        assert_eq!(
            result,
            ModeTransition::ConsumedWithAction("chat_window_open_draft".to_string())
        );
        assert_eq!(sm.mode(), Mode::Insert);
    }

    #[test]
    fn test_normal_to_visual_on_v() {
        let mut sm = ModeStateMachine::new();
        let result = sm.handle_key(KeyCode::Char('v'), empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Visual));
        assert_eq!(sm.mode(), Mode::Visual);
    }

    #[test]
    fn test_normal_to_space_on_space() {
        let mut sm = ModeStateMachine::new();
        let result = sm.handle_key(KeyCode::Char(' '), empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Space));
        assert_eq!(sm.mode(), Mode::Space);
    }

    #[test]
    fn test_insert_to_normal_on_esc() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Insert);
        let result = sm.handle_key(KeyCode::Esc, empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Normal));
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_visual_to_normal_on_esc() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Visual);
        let result = sm.handle_key(KeyCode::Esc, empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Normal));
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_visual_space_to_space() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Visual);
        let result = sm.handle_key(KeyCode::Char(' '), empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Space));
        assert_eq!(sm.mode(), Mode::Space);
    }

    #[test]
    fn test_space_yank_and_paste() {
        let mut sm = ModeStateMachine::new();

        // Test Space y
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Char('y'), empty_mods());
        assert_eq!(
            result,
            ModeTransition::ConsumedWithAction("copy_visual_selection".to_string())
        );
        assert_eq!(sm.mode(), Mode::Normal);

        // Test Space p
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Char('p'), empty_mods());
        assert_eq!(
            result,
            ModeTransition::ConsumedWithAction("paste_from_clipboard".to_string())
        );
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_space_b_opens_active_chats_picker() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Char('b'), empty_mods());
        assert_eq!(
            result,
            ModeTransition::ConsumedWithAction("open_picker_active_chats".to_string())
        );
        assert_eq!(sm.mode(), Mode::Picker);
    }

    #[test]
    fn test_space_f_opens_all_chats_picker() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Char('f'), empty_mods());
        assert_eq!(
            result,
            ModeTransition::ConsumedWithAction("open_picker_all_chats".to_string())
        );
        assert_eq!(sm.mode(), Mode::Picker);
    }

    #[test]
    fn test_space_esc_returns_to_normal() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Esc, empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Normal));
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_space_unknown_key_returns_to_normal() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Space);
        let result = sm.handle_key(KeyCode::Char('z'), empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Normal));
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_picker_esc_returns_to_normal() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Picker);
        let result = sm.handle_key(KeyCode::Esc, empty_mods());
        assert_eq!(result, ModeTransition::ModeChanged(Mode::Normal));
        assert_eq!(sm.mode(), Mode::Normal);
    }

    #[test]
    fn test_normal_movement_keys_stay() {
        let mut sm = ModeStateMachine::new();
        // j, k, h, l should stay in Normal (handled by keymap)
        for key in &['j', 'k', 'h', 'l', 'y', 'd', 'e', 'q'] {
            let result = sm.handle_key(KeyCode::Char(*key), empty_mods());
            assert_eq!(result, ModeTransition::Stay);
            assert_eq!(sm.mode(), Mode::Normal);
        }
    }

    #[test]
    fn test_insert_mode_text_keys_stay() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Insert);
        // Regular chars stay in Insert (handled by prompt)
        let result = sm.handle_key(KeyCode::Char('a'), empty_mods());
        assert_eq!(result, ModeTransition::Stay);
        assert_eq!(sm.mode(), Mode::Insert);
    }

    #[test]
    fn test_space_command_actions() {
        let mut sm = ModeStateMachine::new();
        let commands = vec![
            ('/', "show_search_overlay"),
            ('t', "show_theme_selector"),
            ('?', "show_command_guide"),
            ('w', "forward_message"),
            ('o', "jump_to_forward_origin"),
            ('q', "try_quit"),
        ];
        for (key, expected_action) in commands {
            sm.set_mode(Mode::Space);
            let result = sm.handle_key(KeyCode::Char(key), empty_mods());
            assert_eq!(
                result,
                ModeTransition::ConsumedWithAction(expected_action.to_string()),
                "Space+{key} should produce action '{expected_action}'"
            );
        }
    }

    #[test]
    fn test_mode_display() {
        assert_eq!(format!("{}", Mode::Normal), "NOR");
        assert_eq!(format!("{}", Mode::Visual), "VIS");
        assert_eq!(format!("{}", Mode::Insert), "INS");
        assert_eq!(format!("{}", Mode::Space), "SPC");
        assert_eq!(format!("{}", Mode::Picker), "PKR");
    }

    #[test]
    fn test_set_mode_overrides_current() {
        let mut sm = ModeStateMachine::new();
        sm.set_mode(Mode::Visual);
        assert_eq!(sm.mode(), Mode::Visual);
        sm.set_mode(Mode::Insert);
        assert_eq!(sm.mode(), Mode::Insert);
        sm.set_mode(Mode::Normal);
        assert_eq!(sm.mode(), Mode::Normal);
    }
}
