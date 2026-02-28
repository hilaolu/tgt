# TGT — Helix-Style TUI Architecture

> Telegram TUI client refactored with a Helix-inspired modal editing paradigm.

---

## Modal System

The application is driven by a **finite state machine** (`modal.rs`) with five modes:

| Mode | Purpose | Entry | Exit |
|------|---------|-------|------|
| **Normal** | Navigation, selection, actions | `Esc` from any mode | — |
| **Insert** | Message drafting, editing, replying | `i` / `a` / `o` | `Esc` / `Enter` (submit) |
| **Visual** | Text/message selection | `v` | `Esc` / `y` (yank) |
| **Space** | Leader-key command palette | `Space` | Any command key / `Esc` |
| **Picker** | Fuzzy chat selector overlay | `Space b` / `Space f` | `Enter` (select) / `Esc` |

`ModeStateMachine` handles transitions only — key→action mapping is delegated to `keymap_custom.rs`.

---

## Component Architecture

```
┌─────────────────────────────────────────────────┐
│                   CoreWindow                     │
│  ┌─────────────┬───────────────────────────────┐ │
│  │ ChatList    │         ChatWindow            │ │
│  │ (toggleable)│  ┌─────────────────────────┐  │ │
│  │             │  │ Message List (Full)     │  │ │
│  │             │  │  • Vim buffer overlay   │  │ │
│  │             │  │  • Normal messages      │  │ │
│  │             │  │  • Inline edit blocks   │  │ │
│  │             │  │  • Draft input (bottom) │  │ │
│  │             │  └─────────────────────────┘  │ │
│  └─────────────┴───────────────────────────────┘ │
│  ┌─────────────────────────────────────────────┐ │
│  │            StatusBar (1-line)                │ │
│  └─────────────────────────────────────────────┘ │
│                                                   │
│  ╔═══════════╗  ╔═══════════╗  ╔════════════════╗│
│  ║  Picker   ║  ║ SpaceMenu ║  ║ CommandGuide   ║│
│  ║ (overlay) ║  ║ (overlay) ║  ║   (overlay)    ║│
│  ╚═══════════╝  ╚═══════════╝  ╚════════════════╝│
└─────────────────────────────────────────────────┘
```

### Core Components

| Component | File | Description |
|-----------|------|-------------|
| `CoreWindow` | `core_window.rs` | Root layout manager. Routes actions, manages focus, draws overlays. |
| `ChatWindow` | `chat_window.rs` | Message list + inline input. Handles editing, drafting, replying, and cursor rendering. |
| `ChatListWindow` | `chat_list_window.rs` | Sidebar chat list (toggleable). |
| `StatusBar` | `status_bar.rs` | 1-line Helix-style: mode indicator │ chat name │ unread count. |
| `Picker` | `picker.rs` | Fuzzy chat search overlay (80×70% centered, `nucleo-matcher`). |
| `SpaceMenu` | `space_menu.rs` | Command palette overlay for Space-mode actions. |
| `CommandGuide` | `command_guide.rs` | Keybinding reference overlay. |

---

## Virtual Buffer System (`BufferCursor`)

The ChatWindow acts as a continuous virtual Vim buffer. Instead of just selecting discrete messages, users navigate with a block cursor over the visual lines of the text.

```rust
pub struct BufferCursor {
    pub row: usize, // Visual row in the viewport (0 = top)
    pub col: usize, // Column within the line
}
```

### Navigation Behavior

- **h/j/k/l:** Move the terminal block cursor around the viewport.
- **Scrolling:** When moving `j` past the bottom or `k` past the top, the viewport triggers `next()` or `previous()` and loads older/newer history dynamically.
- **Selection Sync:** The visual `row` coordinate is mapped down via `ListState::offset` and line heights to automatically select the `MessageEntry` currently underneath the cursor.

---

## Inline Input System (`InlineInput`)

All message composition is unified into `ChatWindow` via the `InlineInput` struct:

```rust
pub struct InlineInput {
    pub message_id: Option<i64>,        // Some = editing, None = new message
    pub reply_to_message_id: Option<i64>, // Some = replying
    pub text: String,
    pub cursor: usize,
}
```

### Behavior Matrix

| Action | `message_id` | `reply_to_message_id` | Rendered As |
|--------|-------------|----------------------|-------------|
| Press `i` on own message | `Some(id)` | `None` | Edit overlay on message |
| Press `i` on others' message | `None` | `None` | New draft at bottom |
| Press `o` (Normal) | `None` | `None` | New draft at bottom |
| Reply action | `None` | `Some(reply_id)` | Draft with reply border |

### Submit Logic

- **Enter** with `message_id: Some(id)` → `Event::SendMessageEdited(id, text)`
- **Enter** with `message_id: None` → `Event::SendMessage(text, reply?)`
- **Esc** → Discard draft, return to Normal mode

---

## Event Flow

```
Key Press
  │
  ▼
ModeStateMachine.handle_key()
  │
  ├── ModeChanged(mode) → Action::SetMode(mode)
  │                         │
  │                         ▼
  │                    ChatWindow.update()
  │                      • Creates InlineInput (Insert)
  │                      • Clears InlineInput (Normal)
  │
  └── Stay → keymap_custom lookup
               │
               ▼
          Action dispatched → Component.update()
```

---

## Key Files

| File | Lines | Role |
|------|-------|------|
| `src/modal.rs` | 394 | Mode state machine (17 tests) |
| `src/action.rs` | ~200 | All Action enum variants |
| `src/app_context.rs` | ~330 | Shared state: mode, styles, TG context |
| `src/run.rs` | ~680 | Main event loop, action dispatch |
| `src/components/core_window.rs` | ~1290 | Root layout + action routing |
| `src/components/chat_window.rs` | ~1050 | Message list + inline input |
| `src/components/picker.rs` | 557 | Fuzzy chat picker (8 tests) |
| `src/components/status_bar.rs` | ~200 | Mode-aware status line |
| `src/components/space_menu.rs` | ~150 | Leader-key command palette |
| `src/configs/custom/keymap_custom.rs` | ~800 | Mode-based keymap system |

---

## Recent Changes (v2)

### Commit: `feat(ui): add Vim buffer cursor overlay for ChatWindow`
- Transformed `ChatWindow` into a continuous virtual Vim buffer.
- Added `BufferCursor` to track row/col within the viewport.
- Rendered terminal block cursor dynamically switching based on mode (`SteadyBlock` vs `SteadyBar`).
- Visual cursor position automatically dictates the underlying selected message by computing exact line heights.

### Commit: `refactor(ui): remove ChatWindow header`
- Removed the chat name + status header from `ChatWindow` to maximize the message buffer size.
- Rely entirely on the Helix-style `StatusBar` for context.

### Commit: `feat(ui): Unify message drafting with inline Input`
- **Deleted** `prompt_window.rs` (–920 lines)
- **Introduced** `InlineInput` struct in `chat_window.rs`
- Unified new message drafting, editing, and replying into `ChatWindow`
- `i` contextually edits own messages or opens a new draft
- `o` always opens a new message draft
- Reply drafts render with `│` border indicators
- Name header separated from content (matching `MessageEntry` layout)

### Commit: `feat: Helix-style Picker, Space Menu, and UI refinement`
- Added `Picker` component with `nucleo-matcher` fuzzy search
- Added `SpaceMenu` command palette overlay
- Removed `TitleBar`, added 1-line `StatusBar`
- Integrated `ModeStateMachine` into event loop

---

## Test Coverage

```
test result: ok. 171 passed; 0 failed; 0 ignored
```

| Module | Tests |
|--------|-------|
| `modal` | 17 (mode transitions) |
| `picker` | 8 (fuzzy filter, navigation, lifecycle) |
| `core_window` | 11 (actions, search, focus) |
| Others | 135 (existing, all passing) |
