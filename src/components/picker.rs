use {
    crate::{
        action::Action,
        app_context::AppContext,
        components::{
            chat_list_window::ChatListEntry,
            component_traits::{Component, HandleFocus},
        },
        event::Event,
    },
    crossterm::event::KeyCode,
    nucleo_matcher::{Config, Matcher, Utf32Str},
    ratatui::{
        layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
        style::{Color, Modifier, Style},
        symbols::border::PLAIN,
        text::{Line, Span, Text},
        widgets::{Block, Borders, Clear, List, ListDirection, ListState, Paragraph, Wrap},
        Frame,
    },
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedSender,
};

/// The kind of items the picker is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    /// Active / recently opened chats (space+b).
    ActiveChats,
    /// All chats from the full chat list (space+f).
    AllChats,
}

/// `Picker` is a Helix-style overlay component for fuzzy-searching and selecting
/// items (primarily chats). It renders as a centered overlay with:
///   - Left pane: fuzzy search input + filterable list
///   - Right pane: live preview of the selected item
pub struct Picker {
    app_context: Arc<AppContext>,
    name: String,
    command_tx: Option<UnboundedSender<Action>>,
    focused: bool,

    /// What kind of items are we picking?
    kind: PickerKind,
    /// Whether the picker is currently visible.
    visible: bool,
    /// The raw (unfiltered) list of items.
    all_items: Vec<ChatListEntry>,
    /// Indices into `all_items` after fuzzy filtering (ordered by match score).
    filtered_indices: Vec<usize>,
    /// Current selection index into `filtered_indices`.
    selected: usize,
    /// The fuzzy search input string.
    search_input: String,
    /// List state for ratatui stateful rendering.
    list_state: ListState,
}

impl Picker {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        Picker {
            app_context,
            name: String::new(),
            command_tx: None,
            focused: false,
            kind: PickerKind::AllChats,
            visible: false,
            all_items: Vec::new(),
            filtered_indices: Vec::new(),
            selected: 0,
            search_input: String::new(),
            list_state: ListState::default(),
        }
    }

    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_string();
        self
    }

    /// Open the picker with the given kind, loading items from TgContext.
    fn open(&mut self, kind: PickerKind) {
        self.kind = kind;
        self.visible = true;
        self.search_input.clear();
        self.selected = 0;

        // Load items from TgContext
        self.all_items = match self.app_context.tg_context().get_chats_index() {
            Ok(Some(items)) => items,
            _ => Vec::new(),
        };

        // For ActiveChats mode, we could filter to only recently active.
        // For now, both modes show the full list (ActiveChats will be refined
        // when BufferManager is implemented in Phase 2).

        self.refilter();
    }

    /// Close the picker and return to Normal mode.
    fn close(&mut self) {
        self.visible = false;
        self.search_input.clear();
        self.all_items.clear();
        self.filtered_indices.clear();
        self.selected = 0;

        // Return to Normal mode
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(Action::SetMode(crate::modal::Mode::Normal));
        }
    }

    /// Confirm the current selection: open the selected chat.
    fn confirm_selection(&mut self) {
        if let Some(&item_idx) = self.filtered_indices.get(self.selected) {
            if let Some(chat) = self.all_items.get(item_idx) {
                let chat_id = chat.chat_id();
                let chat_user = chat.user().cloned();

                // Set the open chat context (same as ChatListWindow.confirm_selection)
                {
                    let tg = self.app_context.tg_context();
                    tg.set_open_chat_user(chat_user);
                    tg.set_open_chat_id_i64(chat_id);
                    tg.clear_open_chat_messages();
                    tg.set_jump_target_message_id_i64(0);
                }

                // Load chat history
                if let Some(tx) = &self.command_tx {
                    let _ = tx.send(Action::GetChatHistory);
                }

                // Mark messages as read
                {
                    let tg = self.app_context.tg_context();
                    if let Some(event_tx) = tg.event_tx().as_ref() {
                        let _ = event_tx.send(Event::ViewAllMessages);
                    };
                }
            }
        }

        self.close();
    }

    /// Refilter the item list based on the current search input.
    fn refilter(&mut self) {
        if self.search_input.is_empty() {
            // Show all items
            self.filtered_indices = (0..self.all_items.len()).collect();
        } else {
            let mut config = Config::DEFAULT;
            config.prefer_prefix = true;
            let mut matcher = Matcher::new(config);
            let search_chars: Vec<char> = self.search_input.chars().collect();

            // Collect matches with scores
            let mut scored: Vec<(usize, u16)> = self
                .all_items
                .iter()
                .enumerate()
                .filter_map(|(idx, chat)| {
                    let name_chars: Vec<char> = chat.chat_name().chars().collect();
                    matcher
                        .fuzzy_indices(
                            Utf32Str::Unicode(&name_chars),
                            Utf32Str::Unicode(&search_chars),
                            &mut Vec::new(),
                        )
                        .map(|score| (idx, score))
                })
                .collect();

            // Sort by score descending
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered_indices = scored.into_iter().map(|(idx, _)| idx).collect();
        }

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.filtered_indices.len() - 1);
        }
        self.list_state.select(if self.filtered_indices.is_empty() {
            None
        } else {
            Some(self.selected)
        });
    }

    /// Move selection down.
    fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected = (self.selected + 1).min(self.filtered_indices.len() - 1);
            self.list_state.select(Some(self.selected));
        }
    }

    /// Move selection up.
    fn select_prev(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected = self.selected.saturating_sub(1);
            self.list_state.select(Some(self.selected));
        }
    }

    /// Handle a character input: append to search and refilter.
    fn handle_char(&mut self, c: char) {
        self.search_input.push(c);
        self.refilter();
    }

    /// Handle backspace: remove last char and refilter.
    fn handle_backspace(&mut self) {
        self.search_input.pop();
        self.refilter();
    }

    /// Draw the left pane: search box + filtered list.
    fn draw_left_pane(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Fill(1)])
            .split(area);

        let search_area = layout[0];
        let list_area = layout[1];

        // Search input box
        let search_title = match self.kind {
            PickerKind::ActiveChats => "Active Chats",
            PickerKind::AllChats => "All Chats",
        };
        let search_block = Block::default()
            .border_set(PLAIN)
            .borders(Borders::ALL)
            .title(search_title)
            .border_style(Style::default().fg(Color::Cyan));
        let search_text = format!("{}_", self.search_input);
        let search_para = Paragraph::new(search_text).block(search_block);
        frame.render_widget(search_para, search_area);

        // Set cursor in search box
        if self.focused {
            frame.set_cursor_position(Position {
                x: search_area.x + self.search_input.len() as u16 + 1,
                y: search_area.y + 1,
            });
        }

        // Filtered list
        let items: Vec<Text<'_>> = self
            .filtered_indices
            .iter()
            .map(|&idx| {
                let chat = &self.all_items[idx];
                let name = chat.chat_name();
                let unread = chat.unread_count();
                let mut line = vec![Span::raw(name.to_string())];
                if unread > 0 {
                    line.push(Span::raw(" "));
                    line.push(Span::styled(
                        format!("({})", unread),
                        Style::default().fg(Color::Yellow),
                    ));
                }
                Text::from(Line::from(line))
            })
            .collect();

        let list_block = Block::default()
            .border_set(PLAIN)
            .borders(Borders::LEFT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray));

        let list = List::new(items)
            .block(list_block)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .direction(ListDirection::TopToBottom);

        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    /// Draw the right pane: preview of the selected chat's recent messages.
    fn draw_right_pane(&self, frame: &mut Frame<'_>, area: Rect) {
        let preview_block = Block::default()
            .border_set(PLAIN)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("Preview");

        if let Some(&item_idx) = self.filtered_indices.get(self.selected) {
            let chat = &self.all_items[item_idx];
            let chat_name = chat.chat_name();

            // Show chat name and last message preview
            let mut lines: Vec<Line<'_>> = vec![
                Line::from(Span::styled(
                    chat_name.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];

            // Show last message if available
            if let Some(last_msg) = chat.last_message() {
                let sender = last_msg.sender_id().to_string();
                let content = last_msg.message_content_to_string();
                let timestamp = crate::tg::message_entry::DateTimeEntry::convert_time(
                    last_msg.timestamp().timestamp,
                );

                lines.push(Line::from(vec![
                    Span::styled(sender, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::styled(timestamp, Style::default().fg(Color::DarkGray)),
                ]));
                lines.push(Line::from(Span::raw(content)));
            } else {
                lines.push(Line::from(Span::styled(
                    "[No messages]",
                    Style::default().fg(Color::DarkGray),
                )));
            }

            let preview = Paragraph::new(lines)
                .block(preview_block)
                .wrap(Wrap { trim: false });
            frame.render_widget(preview, area);
        } else {
            let empty = Paragraph::new("No chats found")
                .block(preview_block)
                .alignment(Alignment::Center);
            frame.render_widget(empty, area);
        }
    }
}

impl HandleFocus for Picker {
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl Component for Picker {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> std::io::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::OpenPickerActiveChats => {
                self.open(PickerKind::ActiveChats);
            }
            Action::OpenPickerAllChats => {
                self.open(PickerKind::AllChats);
            }
            Action::Key(key_code, _modifiers) if self.visible => match key_code {
                KeyCode::Esc => self.close(),
                KeyCode::Enter => self.confirm_selection(),
                KeyCode::Down | KeyCode::Tab => self.select_next(),
                KeyCode::Up | KeyCode::BackTab => self.select_prev(),
                KeyCode::Char(c) => self.handle_char(c),
                KeyCode::Backspace => self.handle_backspace(),
                _ => {}
            },
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> std::io::Result<()> {
        if !self.visible {
            return Ok(());
        }

        // Calculate overlay area: centered, taking 80% of width and 70% of height
        let overlay_width = (area.width as f32 * 0.80) as u16;
        let overlay_height = (area.height as f32 * 0.70) as u16;
        let overlay_x = area.x + (area.width.saturating_sub(overlay_width)) / 2;
        let overlay_y = area.y + (area.height.saturating_sub(overlay_height)) / 2;
        let overlay_area = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

        // Clear the area behind the overlay
        frame.render_widget(Clear, overlay_area);

        // Split into left (40%) and right (60%) panes
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(overlay_area);

        self.draw_left_pane(frame, panes[0]);
        self.draw_right_pane(frame, panes[1]);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::search_tests::{create_mock_chat, create_test_app_context};

    fn create_test_picker() -> Picker {
        let app_context = create_test_app_context();
        Picker::new(app_context)
    }

    fn setup_picker_items(picker: &mut Picker, names: &[&str]) {
        picker.all_items = names
            .iter()
            .enumerate()
            .map(|(i, name)| create_mock_chat(i as i64 + 1, name))
            .collect();
        picker.visible = true;
        picker.refilter();
    }

    #[test]
    fn test_picker_initial_state() {
        let picker = create_test_picker();
        assert!(!picker.visible);
        assert!(picker.search_input.is_empty());
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_picker_refilter_shows_all_when_empty_search() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob", "Charlie"]);

        assert_eq!(picker.filtered_indices.len(), 3);
        assert_eq!(picker.filtered_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_picker_fuzzy_filter() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob", "Charlie", "David"]);

        picker.handle_char('a');
        // Should match Alice, Charlie, David (all contain 'a')
        assert!(
            picker.filtered_indices.contains(&0),
            "Alice should match 'a'"
        );
        // Bob should not match
        assert!(
            !picker.filtered_indices.contains(&1),
            "Bob should not match 'a'"
        );
    }

    #[test]
    fn test_picker_fuzzy_filter_narrows() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob", "Charlie", "David"]);

        picker.handle_char('a');
        picker.handle_char('l');
        // "al" should primarily match Alice, possibly Charlie
        assert!(
            picker.filtered_indices.contains(&0),
            "Alice should match 'al'"
        );
    }

    #[test]
    fn test_picker_select_navigation() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob", "Charlie"]);

        assert_eq!(picker.selected, 0);

        picker.select_next();
        assert_eq!(picker.selected, 1);

        picker.select_next();
        assert_eq!(picker.selected, 2);

        // Should not go past the end
        picker.select_next();
        assert_eq!(picker.selected, 2);

        picker.select_prev();
        assert_eq!(picker.selected, 1);

        picker.select_prev();
        assert_eq!(picker.selected, 0);

        // Should not go below 0
        picker.select_prev();
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_picker_backspace_widens_filter() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob", "Charlie"]);

        picker.handle_char('a');
        picker.handle_char('l');
        let narrow_count = picker.filtered_indices.len();

        picker.handle_backspace();
        // After removing 'l', filter is just 'a' → should match more items
        assert!(picker.filtered_indices.len() >= narrow_count);

        picker.handle_backspace();
        // Empty search → all items shown
        assert_eq!(picker.filtered_indices.len(), 3);
    }

    #[test]
    fn test_picker_close_resets_state() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob"]);
        picker.search_input = "test".to_string();
        picker.selected = 1;

        picker.close();

        assert!(!picker.visible);
        assert!(picker.search_input.is_empty());
        assert!(picker.all_items.is_empty());
        assert!(picker.filtered_indices.is_empty());
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_picker_empty_results() {
        let mut picker = create_test_picker();
        setup_picker_items(&mut picker, &["Alice", "Bob"]);

        picker.search_input = "zzzzz".to_string();
        picker.refilter();

        assert!(picker.filtered_indices.is_empty());
        assert_eq!(picker.selected, 0);
    }
}
