use {
    crate::{
        action::Action,
        app_context::AppContext,
        components::component_traits::{Component, HandleFocus},
        event::Event,
        modal::Mode,
    },
    ratatui::{
        layout::{Alignment, Constraint, Direction, Layout, Rect},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::Paragraph,
    },
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedSender,
};

/// `StatusBar` is a Helix-style status bar showing:
/// - Mode indicator (NOR / VIS / INS / SPC / PKR) on the left
/// - Chat name + status info in the middle
/// - Unread count + notifications on the right
pub struct StatusBar {
    /// The application configuration.
    app_context: Arc<AppContext>,
    /// The name of the `StatusBar`.
    name: String,
    /// An unbounded sender that send action for processing.
    command_tx: Option<UnboundedSender<Action>>,
    /// Indicates whether the `StatusBar` is focused or not.
    focused: bool,
    /// The area of the terminal where the all the content will be rendered.
    terminal_area: Rect,
    /// The last key pressed.
    last_key: Event,
    /// Optional short status message (e.g. "Message yanked"); cleared on next key press.
    status_message: Option<String>,
}

impl StatusBar {
    /// Create a new instance of the `StatusBar` struct.
    pub fn new(app_context: Arc<AppContext>) -> Self {
        StatusBar {
            app_context,
            command_tx: None,
            name: "".to_string(),
            terminal_area: Rect::default(),
            last_key: Event::Unknown,
            focused: false,
            status_message: None,
        }
    }

    /// Set the name of the `StatusBar`.
    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_string();
        self
    }

    /// Get the style for the mode indicator based on the current mode.
    fn mode_style(mode: Mode) -> Style {
        match mode {
            Mode::Normal => Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            Mode::Visual => Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            Mode::Insert => Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Mode::Space => Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Mode::Picker => Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }
}

impl HandleFocus for StatusBar {
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl Component for StatusBar {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> std::io::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::UpdateArea(area) => {
                self.terminal_area = area;
            }
            Action::Key(key, modifiers) => {
                self.last_key = Event::Key(key, modifiers.into());
                self.status_message = None;
            }
            Action::StatusMessage(msg) => {
                self.status_message = Some(msg);
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) -> std::io::Result<()> {
        let mode = self.app_context.current_mode();

        // Split area into top (main status) and bottom (hint line)
        let area_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Fill(1)])
            .split(area);
        let main_area = area_split[0];
        let hint_area = area_split[1];

        // Get chat info
        let selected_chat = self
            .app_context
            .tg_context()
            .name_of_open_chat_id()
            .unwrap_or_default();

        // Build the left section: mode indicator
        let mode_str = format!(" {} ", mode);
        let mode_style = Self::mode_style(mode);

        // Build the middle section: chat name
        let mut middle_spans: Vec<Span<'_>> = vec![Span::raw("  ")];
        if !selected_chat.is_empty() {
            middle_spans.push(Span::styled(
                selected_chat,
                self.app_context.style_status_bar_open_chat_name(),
            ));
        }

        // Build the right section: unread count
        let tg_context = self.app_context.tg_context();
        let unread = tg_context.unread_messages().len();
        let right_text = if unread > 0 {
            format!("{} Unread msgs ", unread)
        } else {
            String::new()
        };

        // Lay out top line: mode tag (fixed) | middle (fill) | right (min)
        let right_width = right_text.len() as u16;
        let mode_width = mode_str.len() as u16;

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(mode_width),
                Constraint::Fill(1),
                Constraint::Length(right_width),
            ])
            .split(main_area);

        // Render mode indicator
        let mode_para = Paragraph::new(Span::styled(mode_str, mode_style));
        frame.render_widget(mode_para, chunks[0]);

        // Render middle
        let middle_line = Line::from(middle_spans);
        let middle_para = Paragraph::new(middle_line)
            .style(self.app_context.style_status_bar())
            .alignment(Alignment::Left);
        frame.render_widget(middle_para, chunks[1]);

        // Render right
        if !right_text.is_empty() {
            let right_para = Paragraph::new(Span::styled(
                right_text,
                self.app_context.style_status_bar_size_info_text(),
            ))
            .style(self.app_context.style_status_bar())
            .alignment(Alignment::Right);
            frame.render_widget(right_para, chunks[2]);
        }

        // Render hint line (row 2)
        let hint_content = if let Some(ref msg) = self.status_message {
            Line::from(vec![
                Span::styled(
                    " STATUS ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    msg.as_str(),
                    self.app_context.style_status_bar_message_quit_key(),
                ),
            ])
        } else {
            Line::from("")
        };

        let hint_para = Paragraph::new(hint_content).style(self.app_context.style_status_bar());
        frame.render_widget(hint_para, hint_area);

        Ok(())
    }
}
