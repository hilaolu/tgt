use crate::{
    action::Action,
    app_context::AppContext,
    components::component_traits::{Component, HandleFocus},
    event::Event,
    modal::Mode,
    tg::message_entry::MessageEntry,
};
use arboard::Clipboard;
use crossterm::event::{KeyCode, MouseEventKind};
use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListDirection, ListItem, ListState},
};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// A standardized structure representing the current inline drafted message
#[derive(Clone, Default, Debug)]
pub struct InlineInput {
    /// If Some, we are editing an existing message. If None, it's a new message.
    pub message_id: Option<i64>,
    /// If Some, the drafted message is a reply to this message ID.
    pub reply_to_message_id: Option<i64>,
    /// The drafted text.
    pub text: String,
    /// The cursor position.
    pub cursor: usize,
}

/// Cursor position in the virtual buffer (all messages as one continuous Vim buffer).
/// Row 0 = first line of oldest visible message (visual top of buffer).
#[derive(Clone, Debug, Default)]
pub struct BufferCursor {
    /// Visual row: global line index across all messages (0 = top/oldest).
    pub row: usize,
    /// Column within the line.
    pub col: usize,
}

/// `ChatWindow` is a struct that represents a window for displaying a chat.
/// It is responsible for managing the layout and rendering of the chat window.
pub struct ChatWindow {
    /// The application context.
    app_context: Arc<AppContext>,
    /// The name of the `ChatWindow`.
    name: String,
    /// An unbounded sender that send action for processing.
    action_tx: Option<UnboundedSender<Action>>,
    /// A list of message items to be displayed in the `ChatWindow`.
    message_list: Vec<MessageEntry>,
    /// The state of the list.
    message_list_state: ListState,
    /// Indicates whether the `ChatWindow` is focused or not.
    focused: bool,
    /// When true, next draw will select the newest message (Alt+C restore order).
    request_jump_to_latest: bool,
    /// The inline input state for drafting new messages, modifying, or replying.
    inline_input: Option<InlineInput>,
    /// Vim buffer cursor position.
    buf_cursor: BufferCursor,
    /// Cached visible height of the message list area (updated each draw).
    cached_viewport_height: usize,
}
/// Implementation of the `ChatWindow` struct.
impl ChatWindow {
    /// Create a new instance of the `ChatWindow` struct.
    ///
    /// # Arguments
    /// * `app_context` - An Arc wrapped AppContext struct.
    ///
    /// # Returns
    /// * `Self` - The new instance of the `ChatWindow` struct.
    pub fn new(app_context: Arc<AppContext>) -> Self {
        let name = "".to_string();
        let action_tx = None;
        let message_list = vec![];
        let message_list_state = ListState::default();
        let focused = false;
        ChatWindow {
            app_context,
            name,
            action_tx,
            message_list,
            message_list_state,
            focused,
            request_jump_to_latest: false,
            inline_input: None,
            buf_cursor: BufferCursor::default(),
            cached_viewport_height: 0,
        }
    }
    /// Set the name of the `ChatWindow`.
    ///
    /// # Arguments
    /// * `name` - The name of the `ChatWindow`.
    ///
    /// # Returns
    /// * `Self` - The modified instance of the `ChatWindow`.
    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_string();
        self
    }

    /// Build message_list from data layer (read-only API). Uses a single-lock snapshot to avoid
    /// TOCTOU: another thread clearing the store between ordered_message_ids() and get_message().
    fn refresh_message_list_from_store(&mut self) {
        self.message_list = self.app_context.tg_context().ordered_messages_snapshot();
    }

    /// Select the next message item in the list (down = towards newer messages).
    fn next(&mut self) {
        let len = self.message_list.len();
        // Load more history when near top of loaded range (and not already loading)
        if len > 0 && !self.app_context.tg_context().is_history_loading() {
            let oldest = self.app_context.tg_context().oldest_message_id();
            let selected_id = self
                .message_list_state
                .selected()
                .and_then(|i| self.message_list.get(i).map(|m| m.id()));
            let near_top = match (oldest, selected_id) {
                (Some(old), Some(sel)) => sel == old,
                (_, Some(_)) => self.message_list_state.selected() == Some(0),
                _ => false,
            };
            if near_top {
                if let Some(event_tx) = self.app_context.tg_context().event_tx().as_ref() {
                    let _ = event_tx.send(Event::GetChatHistory);
                }
            }
        }

        // Handle empty list: unselect and return early
        if len == 0 {
            self.message_list_state.select(None);
            return;
        }

        // Bounds check: saturating_sub prevents going below 0 when already at oldest message (index 0).
        // If no selection, start at index 0 (oldest message).
        // Without these checks, scrolling past the ends could cause panics or invalid indices.
        let i = self
            .message_list_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.message_list_state.select(Some(i));
    }

    /// Select the previous message item in the list (up = towards older messages).
    fn previous(&mut self) {
        let len = self.message_list.len();
        // Load newer messages when near bottom (so user can scroll forward in time)
        if len > 0 && !self.app_context.tg_context().is_history_loading() {
            let newest = self.app_context.tg_context().newest_message_id();
            let selected_id = self
                .message_list_state
                .selected()
                .and_then(|i| self.message_list.get(i).map(|m| m.id()));
            let near_bottom = match (newest, selected_id) {
                (Some(new), Some(sel)) => sel == new,
                (_, Some(_)) => self.message_list_state.selected() == Some(len.saturating_sub(1)),
                _ => false,
            };
            if near_bottom {
                if let Some(event_tx) = self.app_context.tg_context().event_tx().as_ref() {
                    let _ = event_tx.send(Event::GetChatHistoryNewer);
                }
            }
        }

        // Handle empty list: unselect and return early
        if len == 0 {
            self.message_list_state.select(None);
            return;
        }

        // Bounds check: min(max_idx) prevents going above len-1 when already at newest message (index len-1).
        // If no selection, start at index 0 (oldest message).
        // Without these checks, scrolling past the ends could cause panics or invalid indices.
        let max_idx = len.saturating_sub(1);
        let i = self
            .message_list_state
            .selected()
            .map(|i| (i + 1).min(max_idx))
            .unwrap_or(0);
        self.message_list_state.select(Some(i));
    }

    /// Unselect the message item in the list.
    fn unselect(&mut self) {
        self.message_list_state.select(None);
    }

    /// Scroll half a page up (towards older messages).
    fn half_page_up(&mut self, visible_height: usize) {
        let half = visible_height / 2;
        for _ in 0..half {
            self.next();
        }
    }

    /// Scroll half a page down (towards newer messages).
    fn half_page_down(&mut self, visible_height: usize) {
        let half = visible_height / 2;
        for _ in 0..half {
            self.previous();
        }
    }

    /// Jump to the first (oldest) message.
    fn goto_top(&mut self) {
        if !self.message_list.is_empty() {
            self.message_list_state.select(Some(0));
            // Trigger history load for older messages
            if !self.app_context.tg_context().is_history_loading() {
                if let Some(event_tx) = self.app_context.tg_context().event_tx().as_ref() {
                    let _ = event_tx.send(Event::GetChatHistory);
                }
            }
        }
    }

    /// Jump to the last (newest) message.
    fn goto_bottom(&mut self) {
        if !self.message_list.is_empty() {
            let last = self.message_list.len() - 1;
            self.message_list_state.select(Some(last));
        }
    }

    /// Delete the selected message item in the list.
    ///
    /// # Arguments
    /// * `revoke` - A boolean flag indicating whether the message should be revoked or not.
    fn delete_selected(&mut self, revoke: bool) {
        if let Some(selected) = self.message_list_state.selected() {
            if let Some(event_tx) = self.app_context.tg_context().event_tx().as_ref() {
                let sender_id = self.message_list[selected].sender_id();
                if sender_id != self.app_context.tg_context().me() {
                    return;
                }
                let message_id = self.message_list[selected].id();
                event_tx
                    .send(Event::DeleteMessages(vec![message_id], revoke))
                    .unwrap();
                self.app_context.tg_context().delete_message(message_id);
            }
        }
    }

    /// Copy the selected message item in the list.
    fn copy_selected(&self) {
        let Some(selected) = self.message_list_state.selected() else {
            return;
        };
        let Some(entry) = self.message_list.get(selected) else {
            return;
        };
        let message = entry.message_content_to_string();
        match Clipboard::new() {
            Ok(mut clipboard) => {
                if clipboard.set_text(&message).is_ok() {
                    if let Some(tx) = self.action_tx.as_ref() {
                        let _ = tx.send(Action::StatusMessage("Message yanked".into()));
                    }
                } else {
                    tracing::warn!("Clipboard set_text failed");
                }
            }
            Err(e) => {
                tracing::warn!("Clipboard unavailable (copy message): {}", e);
            }
        }
    }

    /// Edit the selected message item in the list. Only our own messages can be edited.
    fn edit_selected(&mut self) {
        let Some(selected) = self.message_list_state.selected() else {
            return;
        };
        let Some(entry) = self.message_list.get(selected) else {
            return;
        };
        if entry.sender_id() != self.app_context.tg_context().me() {
            // Not our message: do nothing to avoid bugging the chat.
            return;
        }
        let message_id = entry.id();
        let message = entry.message_content_to_string();
        self.inline_input = Some(InlineInput {
            message_id: Some(message_id),
            reply_to_message_id: None,
            text: message.clone(),
            cursor: message.chars().count(),
        });
        if let Some(tx) = self.action_tx.as_ref() {
            let _ = tx.send(Action::SetMode(Mode::Insert));
        }
    }

    /// Reply to the selected message item in the list.
    /// Initiates a reply draft in Insert mode.
    fn reply_selected(&mut self) {
        if let Some(selected) = self.message_list_state.selected() {
            let message_id = self.message_list[selected].id();
            self.inline_input = Some(InlineInput {
                message_id: None,
                reply_to_message_id: Some(message_id),
                text: String::new(),
                cursor: 0,
            });
            if let Some(tx) = self.action_tx.as_ref() {
                let _ = tx.send(Action::SetMode(Mode::Insert));
            }
        }
    }

    /// View photo from the selected message.
    fn view_photo_selected(&self) {
        if let Some(selected) = self.message_list_state.selected() {
            let message_id = self.message_list[selected].id();
            if let Some(tx) = self.action_tx.as_ref() {
                let _ = tx.send(Action::ViewPhotoMessage(message_id));
            }
        }
    }

    /// Navigate to previous message and view its photo (up = towards newer messages).
    fn view_photo_previous(&mut self) {
        // Reuse the existing navigation logic with lazy loading
        self.next();

        // After navigation, send action to view the photo of the selected message
        if let Some(selected) = self.message_list_state.selected() {
            if let Some(message) = self.message_list.get(selected) {
                let message_id = message.id();
                if let Some(tx) = self.action_tx.as_ref() {
                    let _ = tx.send(Action::ViewPhotoMessage(message_id));
                }
            }
        }
    }

    /// Navigate to next message and view its photo (down = towards older messages).
    fn view_photo_next(&mut self) {
        // Reuse the existing navigation logic with lazy loading
        self.previous();

        // After navigation, send action to view the photo of the selected message
        if let Some(selected) = self.message_list_state.selected() {
            if let Some(message) = self.message_list.get(selected) {
                let message_id = message.id();
                if let Some(tx) = self.action_tx.as_ref() {
                    let _ = tx.send(Action::ViewPhotoMessage(message_id));
                }
            }
        }
    }

    /// Wraps each line with a border span on one side only (reply-target border-only highlight).
    /// Messages from others: `│` at the start of each line. Messages from me: `│` at the end.
    /// This keeps borders aligned and avoids broken vertical bars under each other.
    fn wrap_text_with_reply_border(
        content: Text,
        border_style: Style,
        alignment: Alignment,
        myself: bool,
    ) -> Text {
        let wrapped_lines: Vec<Line> = content
            .into_iter()
            .map(|line| {
                if myself {
                    let mut spans: Vec<Span> = line.into_iter().collect();
                    spans.push(Span::styled(" │", border_style));
                    Line::from(spans)
                } else {
                    let mut spans = vec![Span::styled("│ ", border_style)];
                    spans.extend(line);
                    Line::from(spans)
                }
            })
            .collect();
        Text::from(wrapped_lines).alignment(alignment)
    }
}

/// Implement the `HandleFocus` trait for the `ChatWindow` struct.
/// This trait allows the `ChatListWindow` to be focused or unfocused.
impl HandleFocus for ChatWindow {
    /// Set the `focused` flag for the `ChatWindow`.
    fn focus(&mut self) {
        self.focused = true;
    }
    /// Set the `focused` flag for the `ChatWindow`.
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

/// Implement the `Component` trait for the `ChatListWindow` struct.
impl Component for ChatWindow {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> std::io::Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_mouse_events(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> std::io::Result<Option<Action>> {
        if !self.focused {
            return Ok(None);
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => Ok(Some(Action::ChatWindowPrevious)),
            MouseEventKind::ScrollUp => Ok(Some(Action::ChatWindowNext)),
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::SetMode(mode) => {
                if mode != Mode::Insert {
                    // When leaving insert mode, exit inline editing state
                    self.inline_input = None;
                } else if self.inline_input.is_none() {
                    // Try to edit the currently selected message if it belongs to me
                    let mut editing = false;
                    if let Some(selected) = self.message_list_state.selected() {
                        if let Some(entry) = self.message_list.get(selected) {
                            if entry.sender_id() == self.app_context.tg_context().me() {
                                let message_id = entry.id();
                                let message = entry.message_content_to_string();
                                self.inline_input = Some(InlineInput {
                                    message_id: Some(message_id),
                                    reply_to_message_id: None,
                                    text: message.clone(),
                                    cursor: message.chars().count(),
                                });
                                editing = true;
                            }
                        }
                    }
                    if !editing {
                        // Create a draft for a new message
                        self.inline_input = Some(InlineInput {
                            message_id: None,
                            reply_to_message_id: None,
                            text: String::new(),
                            cursor: 0,
                        });
                    }
                }
            }
            // --- Legacy actions (kept for backward compat) ---
            Action::ChatWindowNext => self.next(),
            Action::ChatWindowPrevious => self.previous(),
            Action::ChatWindowUnselect => self.unselect(),
            Action::ChatWindowDeleteForEveryone => self.delete_selected(true),
            Action::ChatWindowDeleteForMe => self.delete_selected(false),
            Action::ChatWindowCopy => self.copy_selected(),
            Action::ChatWindowEdit => self.edit_selected(),
            Action::ChatWindowOpenDraft => {
                self.inline_input = Some(InlineInput {
                    message_id: None,
                    reply_to_message_id: None,
                    text: String::new(),
                    cursor: 0,
                });
                if let Some(tx) = self.action_tx.as_ref() {
                    let _ = tx.send(Action::SetMode(Mode::Insert));
                }
            }
            Action::ShowChatWindowReply => self.reply_selected(),

            // --- Modal buffer cursor actions ---
            Action::BufferCursorUp => self.next(),
            Action::BufferCursorDown => self.previous(),
            Action::BufferScrollHalfPageUp => self.half_page_up(20), // approximate; draw() will refine
            Action::BufferScrollHalfPageDown => self.half_page_down(20),
            Action::BufferGotoTop => self.goto_top(),
            Action::BufferGotoBottom => self.goto_bottom(),

            Action::ShowPhotoViewer => {
                // User pressed keybinding to view photo from selected message
                self.view_photo_selected();
            }
            Action::PhotoViewerPrevious => {
                // Navigate to previous message and view its photo
                self.view_photo_previous();
            }
            Action::PhotoViewerNext => {
                // Navigate to next message and view its photo
                self.view_photo_next();
            }
            Action::ViewPhotoMessage(message_id) => {
                // Forward to CoreWindow to show the photo viewer
                if let Some(tx) = self.action_tx.as_ref() {
                    let _ = tx.send(Action::ViewPhotoMessage(message_id));
                }
            }
            Action::JumpCompleted(_message_id) => {
                // Selection by message_id is applied in draw() when jump_target_message_id is set
            }
            Action::ChatWindowSortWithString(_) => {
                // No-op: chat message search is server-side only (search overlay)
            }
            Action::ChatWindowRestoreSort => {
                self.request_jump_to_latest = true;
            }
            Action::ChatWindowSearch | Action::ChatListSearch => {
                // Handled by CoreWindow: opens search overlay or focuses ChatList
            }
            Action::Key(key_code, modifiers) => {
                // Intercept key events if we are editing a message inline in Insert mode
                if let Some(inline) = self.inline_input.as_mut() {
                    let text = &mut inline.text;
                    let cursor = &mut inline.cursor;
                    match key_code {
                        KeyCode::Esc => {
                            self.inline_input = None;
                            if let Some(tx) = self.action_tx.as_ref() {
                                let _ = tx.send(Action::SetMode(Mode::Normal));
                            }
                        }
                        KeyCode::Enter => {
                            let new_text = text.clone();
                            if !new_text.trim().is_empty() {
                                if let Some(event_tx) =
                                    self.app_context.tg_context().event_tx().as_ref()
                                {
                                    if let Some(id) = inline.message_id {
                                        let _ =
                                            event_tx.send(Event::SendMessageEdited(id, new_text));
                                    } else {
                                        let reply = inline.reply_to_message_id.map(|reply_to| {
                                            crate::tg::td_enums::TdMessageReplyToMessage {
                                                message_id: reply_to,
                                                chat_id: self
                                                    .app_context
                                                    .tg_context()
                                                    .open_chat_id()
                                                    .into(),
                                            }
                                        });
                                        let _ = event_tx.send(Event::SendMessage(new_text, reply));
                                    }
                                }
                            }
                            self.inline_input = None;
                            if let Some(tx) = self.action_tx.as_ref() {
                                let _ = tx.send(Action::SetMode(Mode::Normal));
                            }
                        }
                        KeyCode::Left => {
                            *cursor = cursor.saturating_sub(1);
                        }
                        KeyCode::Right => {
                            *cursor = (*cursor + 1).min(text.chars().count());
                        }
                        KeyCode::Backspace => {
                            if *cursor > 0 {
                                let chars: Vec<char> = text.chars().collect();
                                *cursor -= 1;
                                let mut new_text = String::new();
                                for (i, c) in chars.iter().enumerate() {
                                    if i != *cursor {
                                        new_text.push(*c);
                                    }
                                }
                                *text = new_text;
                            }
                        }
                        KeyCode::Delete => {
                            let chars: Vec<char> = text.chars().collect();
                            if *cursor < chars.len() {
                                let mut new_text = String::new();
                                for (i, c) in chars.iter().enumerate() {
                                    if i != *cursor {
                                        new_text.push(*c);
                                    }
                                }
                                *text = new_text;
                            }
                        }
                        KeyCode::Char(c) => {
                            // Insert character
                            let chars: Vec<char> = text.chars().collect();
                            let mut new_text = String::new();
                            for (i, ch) in chars.iter().enumerate() {
                                if i == *cursor {
                                    new_text.push(c);
                                }
                                new_text.push(*ch);
                            }
                            if *cursor == chars.len() {
                                new_text.push(c);
                            }
                            *text = new_text;
                            *cursor += 1;
                        }
                        _ => {}
                    }
                } else if self.focused {
                    // ── Normal-mode: Vim buffer cursor (viewport-relative) ──
                    let vp_h = self.cached_viewport_height;
                    match key_code {
                        // j / ↓ = move cursor down one line
                        KeyCode::Char('j') | KeyCode::Down => {
                            if vp_h > 0 && self.buf_cursor.row < vp_h.saturating_sub(1) {
                                self.buf_cursor.row += 1;
                            } else {
                                // At bottom edge: scroll list towards newer messages
                                self.previous();
                            }
                        }
                        // k / ↑ = move cursor up one line
                        KeyCode::Char('k') | KeyCode::Up => {
                            if self.buf_cursor.row > 0 {
                                self.buf_cursor.row -= 1;
                            } else {
                                // At top edge: scroll list towards older messages
                                self.next();
                            }
                        }
                        // h / ← = move cursor left
                        KeyCode::Char('h') | KeyCode::Left => {
                            self.buf_cursor.col = self.buf_cursor.col.saturating_sub(1);
                        }
                        // l / → = move cursor right
                        KeyCode::Char('l') | KeyCode::Right => {
                            self.buf_cursor.col += 1;
                        }
                        // G = jump to bottom (newest)
                        KeyCode::Char('G') => {
                            if vp_h > 0 {
                                self.buf_cursor.row = vp_h.saturating_sub(1);
                            }
                            self.goto_bottom();
                        }
                        // g = jump to top (oldest)
                        KeyCode::Char('g') => {
                            self.buf_cursor.row = 0;
                            self.goto_top();
                        }
                        // 0 = start of line
                        KeyCode::Char('0') => {
                            self.buf_cursor.col = 0;
                        }
                        // $ = end of line
                        KeyCode::Char('$') => {
                            self.buf_cursor.col = usize::MAX;
                        }
                        // Ctrl+U = half page up
                        KeyCode::Char('u') if modifiers.control => {
                            let half = vp_h / 2;
                            if self.buf_cursor.row >= half {
                                self.buf_cursor.row -= half;
                            } else {
                                let scroll_by = half - self.buf_cursor.row;
                                self.buf_cursor.row = 0;
                                for _ in 0..scroll_by {
                                    self.next();
                                }
                            }
                        }
                        // Ctrl+D = half page down
                        KeyCode::Char('d') if modifiers.control => {
                            let half = vp_h / 2;
                            let max_row = vp_h.saturating_sub(1);
                            if self.buf_cursor.row + half <= max_row {
                                self.buf_cursor.row += half;
                            } else {
                                let scroll_by = (self.buf_cursor.row + half).saturating_sub(max_row);
                                self.buf_cursor.row = max_row;
                                for _ in 0..scroll_by {
                                    self.previous();
                                }
                            }
                        }
                        KeyCode::Char('r') if modifiers.alt => {
                            if let Some(tx) = self.action_tx.as_ref() {
                                let _ = tx.send(Action::ChatListSearch);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) -> std::io::Result<()> {
        // Capture selection by ID before any clear, so we can restore viewport on redraw
        let selected_message_id_before = self
            .message_list_state
            .selected()
            .and_then(|idx| self.message_list.get(idx).map(|m| m.id()));

        // In modal UI: always show cursor in Normal mode (buffer-style).
        // Only clear selection if there's no chat open.
        let current_mode = self.app_context.current_mode();
        let show_cursor = matches!(current_mode, Mode::Normal | Mode::Visual);
        if !show_cursor && !self.focused {
            self.message_list_state.select(None);
        }

        // Always refresh message list from store
        let selected_message_id = selected_message_id_before;
        let prev_len = self.message_list.len();
        self.refresh_message_list_from_store();

        // After jump-to-message: select the target and clear the flag
        let jump_target = self
            .app_context
            .tg_context()
            .jump_target_message_id()
            .as_i64();
        if jump_target != 0 {
            self.app_context
                .tg_context()
                .set_jump_target_message_id_i64(0);
            if let Some(idx) = self.message_list.iter().position(|m| m.id() == jump_target) {
                self.message_list_state.select(Some(idx));
            }
        } else {
            // Alt+C restore order: jump to latest message
            if self.request_jump_to_latest {
                self.request_jump_to_latest = false;
                if !self.message_list.is_empty() {
                    if let Some(newest_id) = self.app_context.tg_context().newest_message_id() {
                        if let Some(new_idx) =
                            self.message_list.iter().position(|m| m.id() == newest_id)
                        {
                            self.message_list_state.select(Some(new_idx));
                        }
                    }
                }
            } else {
                // Restore selection by message ID when possible
                let selection_restored = selected_message_id.and_then(|id| {
                    self.message_list
                        .iter()
                        .position(|m| m.id() == id)
                        .map(|idx| {
                            self.message_list_state.select(Some(idx));
                            id
                        })
                });
                // When we have no valid selection, jump to latest
                let at_bottom = selection_restored
                    .zip(self.app_context.tg_context().newest_message_id())
                    .is_some_and(|(sel_id, newest_id)| sel_id == newest_id);
                let should_jump_to_latest = selection_restored.is_none()
                    || (at_bottom && prev_len < self.message_list.len());
                if should_jump_to_latest && !self.message_list.is_empty() {
                    if let Some(newest_id) = self.app_context.tg_context().newest_message_id() {
                        if let Some(new_idx) =
                            self.message_list.iter().position(|m| m.id() == newest_id)
                        {
                            self.message_list_state.select(Some(new_idx));
                        }
                    }
                }
            }
        }

        // ── Layout: full area for message list (header removed) ──
        let list_area = area;

        // ── Message list block (clean borders, no sidebar joins) ──
        let block = Block::new()
            .border_set(border::PLAIN)
            .borders(Borders::LEFT | Borders::RIGHT)
            .style(self.app_context.style_chat());
        let list_inner = block.inner(list_area);
        let wrap_width = list_inner.width.saturating_sub(2) as i32;

        let reply_message_id = self.app_context.tg_context().reply_message_id().as_i64();
        let mut is_unread_outbox = true;
        let mut is_unread_inbox = true;
        let mut items: Vec<ListItem<'_>> = self
            .message_list
            .iter()
            .map(|message_entry| {
                let (myself, name_style, content_style, alignment) =
                    if message_entry.sender_id() == self.app_context.tg_context().me() {
                        if message_entry.id()
                            == self.app_context.tg_context().last_read_outbox_message_id()
                        {
                            is_unread_outbox = false;
                        }
                        (
                            true,
                            self.app_context.style_chat_message_myself_name(),
                            self.app_context.style_chat_message_myself_content(),
                            Alignment::Right,
                        )
                    } else {
                        if message_entry.id()
                            == self.app_context.tg_context().last_read_inbox_message_id()
                        {
                            is_unread_inbox = false;
                        }
                        (
                            false,
                            self.app_context.style_chat_message_other_name(),
                            self.app_context.style_chat_message_other_content(),
                            Alignment::Left,
                        )
                    };
                let content = if let Some(ref inline) = self.inline_input {
                    if inline.message_id == Some(message_entry.id()) {
                        let edit_text = &inline.text;
                        let cursor = inline.cursor;
                        // Editable text rendering
                        let mut text_lines = Vec::new();
                        let target_name = self
                            .app_context
                            .tg_context()
                            .try_name_from_chats_or_users(self.app_context.tg_context().me())
                            .unwrap_or_default();

                        text_lines.push(Line::from(vec![
                            Span::styled(target_name, name_style),
                            Span::raw(" [editing]"),
                        ]));

                        let mut current_line_spans = Vec::new();
                        let mut current_len = 0;

                        // Simple char width approximation
                        use unicode_width::UnicodeWidthChar;
                        let cursor_style =
                            Style::default().add_modifier(ratatui::style::Modifier::REVERSED);

                        for (i, c) in edit_text.chars().enumerate() {
                            let style = if i == cursor {
                                cursor_style
                            } else {
                                content_style
                            };
                            let char_w = c.width().unwrap_or(1);

                            if c == '\n' {
                                text_lines
                                    .push(Line::from(std::mem::take(&mut current_line_spans)));
                                current_len = 0;
                            } else {
                                if current_len + char_w > wrap_width as usize && current_len > 0 {
                                    text_lines
                                        .push(Line::from(std::mem::take(&mut current_line_spans)));
                                    current_len = 0;
                                }
                                current_line_spans.push(Span::styled(c.to_string(), style));
                                current_len += char_w;
                            }
                        }
                        if cursor == edit_text.chars().count() {
                            if current_len + 1 > wrap_width as usize && current_len > 0 {
                                text_lines
                                    .push(Line::from(std::mem::take(&mut current_line_spans)));
                            }
                            current_line_spans.push(Span::styled(" ", cursor_style));
                        }
                        if !current_line_spans.is_empty() {
                            text_lines.push(Line::from(current_line_spans));
                        }
                        Text::from(text_lines).alignment(alignment)
                    } else {
                        message_entry
                            .get_text_styled(
                                myself,
                                &self.app_context,
                                is_unread_outbox,
                                name_style,
                                content_style,
                                wrap_width,
                            )
                            .alignment(alignment)
                    }
                } else {
                    message_entry
                        .get_text_styled(
                            myself,
                            &self.app_context,
                            is_unread_outbox,
                            name_style,
                            content_style,
                            wrap_width,
                        )
                        .alignment(alignment)
                };
                let is_reply_target =
                    reply_message_id != 0 && message_entry.id() == reply_message_id;
                if is_reply_target {
                    let border_style = self.app_context.style_item_reply_target();
                    let content_with_border =
                        Self::wrap_text_with_reply_border(content, border_style, alignment, myself);
                    ListItem::new(content_with_border)
                } else {
                    ListItem::new(content)
                }
            })
            .collect();
        if self.app_context.tg_context().is_history_loading() {
            items.push(ListItem::new(Line::from(Span::styled(
                "Loading…",
                self.app_context.style_timestamp(),
            ))));
        }

        // ── Render Inline Draft at Bottom ──
        // (Appended before reverse, so it becomes index 0, i.e. the absolute bottom)
        if let Some(ref inline) = self.inline_input {
            if inline.message_id.is_none() {
                let mut text_lines = Vec::new();
                let target_name = self
                    .app_context
                    .tg_context()
                    .try_name_from_chats_or_users(self.app_context.tg_context().me())
                    .unwrap_or_default();
                let name_style = self.app_context.style_chat_message_myself_name();
                let content_style = self.app_context.style_chat_message_myself_content();

                text_lines.push(Line::from(vec![
                    Span::styled(target_name, name_style),
                    Span::raw(" [draft]"),
                ]));

                let mut current_line_spans = Vec::new();
                let mut current_len = 0;

                use unicode_width::UnicodeWidthChar;
                let cursor_style =
                    Style::default().add_modifier(ratatui::style::Modifier::REVERSED);

                for (i, c) in inline.text.chars().enumerate() {
                    let style = if i == inline.cursor {
                        cursor_style
                    } else {
                        content_style
                    };
                    let char_w = c.width().unwrap_or(1);

                    if c == '\n' {
                        text_lines.push(Line::from(std::mem::take(&mut current_line_spans)));
                        current_len = 0;
                    } else {
                        if current_len + char_w > wrap_width as usize && current_len > 0 {
                            text_lines.push(Line::from(std::mem::take(&mut current_line_spans)));
                            current_len = 0;
                        }
                        current_line_spans.push(Span::styled(c.to_string(), style));
                        current_len += char_w;
                    }
                }
                if inline.cursor == inline.text.chars().count() {
                    if current_len + 1 > wrap_width as usize && current_len > 0 {
                        text_lines.push(Line::from(std::mem::take(&mut current_line_spans)));
                    }
                    current_line_spans.push(Span::styled(" ", cursor_style));
                }
                if !current_line_spans.is_empty() {
                    text_lines.push(Line::from(current_line_spans));
                }
                let content = Text::from(text_lines).alignment(Alignment::Right);

                let is_reply = inline.reply_to_message_id.is_some();
                let list_item = if is_reply {
                    let border_style = self.app_context.style_item_reply_target();
                    let content_with_border = Self::wrap_text_with_reply_border(
                        content,
                        border_style,
                        Alignment::Right,
                        true,
                    );
                    ListItem::new(content_with_border)
                } else {
                    ListItem::new(content)
                };
                items.push(list_item);
            }
        }

        // Render from bottom to top: reverse so newest is drawn at bottom
        items.reverse();
        let item_count = items.len();

        // Convert selection index for reversed list
        let orig_selected = self.message_list_state.selected();
        let list_selected = orig_selected.map(|i| item_count.saturating_sub(1).saturating_sub(i));
        self.message_list_state.select(list_selected);

        // Cursor highlight style — use distinct style for Normal vs Visual mode
        let highlight_style = match current_mode {
            Mode::Visual => {
                // Visual mode: more prominent selection
                self.app_context.style_item_selected()
            }
            _ => self.app_context.style_item_selected(),
        };
        let list = List::new(items)
            .block(block)
            .style(self.app_context.style_chat())
            .highlight_style(highlight_style)
            .repeat_highlight_symbol(true)
            .direction(ListDirection::BottomToTop);

        frame.render_stateful_widget(list, list_area, &mut self.message_list_state);

        // Restore selection to message_list index (oldest=0) for next/previous and other logic.
        let list_sel = self.message_list_state.selected();
        self.message_list_state
            .select(list_sel.map(|i| item_count.saturating_sub(1).saturating_sub(i)));

        // ── Cache viewport height for key handler ──
        self.cached_viewport_height = list_inner.height as usize;

        // ── Render Vim buffer cursor (Normal/Visual mode) ──
        if self.focused && matches!(current_mode, Mode::Normal | Mode::Visual) {
            // Clamp cursor to viewport bounds
            let max_row = list_inner.height.saturating_sub(1) as usize;
            let max_col = list_inner.width.saturating_sub(1) as usize;
            if self.buf_cursor.row > max_row {
                self.buf_cursor.row = max_row;
            }
            if self.buf_cursor.col > max_col {
                self.buf_cursor.col = max_col;
            }
            let cursor_x = list_inner.x + self.buf_cursor.col as u16;
            let cursor_y = list_inner.y + self.buf_cursor.row as u16;
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::{Action, Modifiers},
        components::search_tests::{create_mock_message, create_test_app_context},
    };
    use crossterm::event::{KeyCode, KeyModifiers};

    fn create_test_chat_window() -> ChatWindow {
        let app_context = create_test_app_context();
        ChatWindow::new(app_context)
    }

    fn setup_messages(window: &mut ChatWindow, messages: Vec<MessageEntry>) {
        let tg_context = window.app_context.tg_context();
        {
            let mut store = tg_context.open_chat_messages();
            store.clear();
            store.insert_messages(messages);
        }
        window.refresh_message_list_from_store();
    }

    #[test]
    fn test_chat_window_selection_by_id() {
        let mut window = create_test_chat_window();
        let messages = vec![
            create_mock_message(100, "First"),
            create_mock_message(200, "Second"),
            create_mock_message(300, "Third"),
        ];
        setup_messages(&mut window, messages);
        window.message_list_state.select(Some(1));
        let selected_id = window
            .message_list_state
            .selected()
            .and_then(|idx| window.message_list.get(idx).map(|m| m.id()));
        assert_eq!(selected_id, Some(200));
    }

    #[test]
    fn test_chat_window_navigation() {
        let mut window = create_test_chat_window();
        let messages = vec![
            create_mock_message(1, "One"),
            create_mock_message(2, "Two"),
            create_mock_message(3, "Three"),
        ];
        setup_messages(&mut window, messages);
        window.focus(); // Key events only handled when focused
        let modifiers = Modifiers::from(KeyModifiers::empty());
        window.update(Action::Key(KeyCode::Down, modifiers.clone()));
        window.update(Action::Key(KeyCode::Up, modifiers.clone()));
        let selected = window.message_list_state.selected();
        assert_eq!(selected, Some(0));
    }

    #[test]
    fn test_chat_window_sort_actions_no_op() {
        let mut window = create_test_chat_window();
        window.update(Action::ChatWindowSortWithString("test".to_string()));
        window.update(Action::ChatWindowRestoreSort);
        // Chat message search is server-side only; these are no-ops
        assert_eq!(window.message_list.len(), 0);
    }

    /// Edit on someone else's message must be a no-op (does not send EditMessage, does not bug the chat).
    /// create_mock_message uses sender user_id 1; test app context has me() = 0, so all mock messages are "others".
    #[test]
    fn test_edit_on_others_message_is_no_op() {
        let mut window = create_test_chat_window();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        window.register_action_handler(tx).unwrap();
        let messages = vec![
            create_mock_message(100, "Other's message"),
            create_mock_message(200, "Another other"),
        ];
        setup_messages(&mut window, messages);
        window.message_list_state.select(Some(0));
        let selected_before = window.message_list_state.selected();
        let selected_id_before =
            selected_before.and_then(|i| window.message_list.get(i).map(|m| m.id()));
        window.update(Action::ChatWindowEdit);
        // Selection unchanged; no EditMessage is sent (we can't easily assert no event without mock event_tx).
        assert_eq!(window.message_list_state.selected(), selected_before);
        assert_eq!(
            selected_before.and_then(|i| window.message_list.get(i).map(|m| m.id())),
            selected_id_before
        );
    }
}
