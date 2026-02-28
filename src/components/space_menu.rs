use {
    crate::{
        action::Action,
        app_context::AppContext,
        components::component_traits::{Component, HandleFocus},
        modal::Mode,
    },
    crossterm::event::KeyCode,
    ratatui::{
        layout::Rect,
        style::{Color, Modifier, Style},
        symbols::border::PLAIN,
        text::{Line, Span},
        widgets::{Block, Borders, Clear, List, ListItem},
        Frame,
    },
    std::sync::Arc,
    tokio::sync::mpsc::UnboundedSender,
};

/// A command available in the Space menu.
struct SpaceCommand {
    key: char,
    description: &'static str,
    action: Action,
}

/// `SpaceMenu` displays a floating overlay with available commands
/// when the user enters Space mode.
pub struct SpaceMenu {
    app_context: Arc<AppContext>,
    name: String,
    command_tx: Option<UnboundedSender<Action>>,
    focused: bool,
    commands: Vec<SpaceCommand>,
}

impl SpaceMenu {
    pub fn new(app_context: Arc<AppContext>) -> Self {
        // Define the available space commands
        let commands = vec![
            SpaceCommand {
                key: 'f',
                description: "all chats",
                action: Action::OpenPickerAllChats,
            },
            SpaceCommand {
                key: 'b',
                description: "active chats",
                action: Action::OpenPickerActiveChats,
            },
            SpaceCommand {
                key: '/',
                description: "search messages",
                action: Action::ChatListSearch, // Reuses existing search overlay for now
            },
            SpaceCommand {
                key: 't',
                description: "theme selector",
                action: Action::SwitchTheme,
            },
            SpaceCommand {
                key: '?',
                description: "command guide",
                action: Action::ShowCommandGuide,
            },
        ];

        SpaceMenu {
            app_context,
            name: String::new(),
            command_tx: None,
            focused: false,
            commands,
        }
    }

    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        self.name = name.as_ref().to_string();
        self
    }

    /// Close the menu by returning to Normal mode
    fn close(&self) {
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(Action::SetMode(Mode::Normal));
        }
    }

    /// Handle a key press in Space mode
    fn handle_key(&self, c: char) {
        // Find matching command
        if let Some(cmd) = self.commands.iter().find(|cmd| cmd.key == c) {
            if let Some(tx) = &self.command_tx {
                let _ = tx.send(cmd.action.clone());
            }
        } else {
            // Unknown key, close the menu
            self.close();
        }
    }
}

impl HandleFocus for SpaceMenu {
    fn focus(&mut self) {
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl Component for SpaceMenu {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> std::io::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) {
        if self.app_context.current_mode() != Mode::Space {
            return;
        }

        if let Action::Key(key_code, _modifiers) = action {
            match key_code {
                KeyCode::Esc => self.close(),
                KeyCode::Char(c) => self.handle_key(c),
                _ => self.close(), // Any other key aborts Space mode
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) -> std::io::Result<()> {
        if self.app_context.current_mode() != Mode::Space {
            return Ok(());
        }

        // Calculate size based on commands
        let height = (self.commands.len() as u16) + 4; // Top/bottom border + padding
        let width = 35; // Fixed width

        // Position: Bottom-right, slightly padded from the edges
        let x = area.x + area.width.saturating_sub(width + 2);
        let y = area.y + area.height.saturating_sub(height + 3); // Extra 1 for status bar

        // Clamp to ensure it doesn't go off-screen
        let rect_x = x.max(area.x);
        let rect_y = y.max(area.y);
        let rect_width = width.min(area.width);
        let rect_height = height.min(area.height);

        let menu_area = Rect::new(rect_x, rect_y, rect_width, rect_height);

        // Clear the background
        frame.render_widget(Clear, menu_area);

        // Draw block
        let block = Block::default()
            .border_set(PLAIN)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title("Space")
            .style(self.app_context.style_chat());

        let inner_area = block.inner(menu_area);
        frame.render_widget(block, menu_area);

        // Draw commands
        let items: Vec<ListItem> = self
            .commands
            .iter()
            .map(|cmd| {
                let line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        cmd.key.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::raw(cmd.description),
                ]);
                ListItem::new(line)
            })
            .collect();

        // Add an empty line at the top for padding
        let mut padded_items = vec![ListItem::new(Line::from(""))];
        padded_items.extend(items);
        padded_items.push(ListItem::new(Line::from("")));

        let list = List::new(padded_items).style(self.app_context.style_chat());

        frame.render_widget(list, inner_area);

        Ok(())
    }
}
