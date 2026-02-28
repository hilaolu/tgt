use ratatui::widgets::ListState;
pub fn foo() {
    let state = ListState::default();
    let _ = state.offset();
}