use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::io::Write;

/// Sets the clipboard text using `arboard` with a fallback to OSC 52 terminal sequences.
pub fn set_clipboard_text(text: &str) -> bool {
    let mut success = false;

    // Attempt standard arboard first.
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text).is_ok() {
            success = true;
        }
    }

    // Always attempt OSC 52 as a robust fallback for terminals like Kitty/Wezterm over SSH/Wayland.
    let base64_encoded = BASE64_STANDARD.encode(text);
    let osc52_payload = format!("\x1b]52;c;{}\x07", base64_encoded);

    let mut stdout = std::io::stdout();
    if stdout.write_all(osc52_payload.as_bytes()).is_ok() && stdout.flush().is_ok() {
        success = true;
    }

    success
}

/// Gets the clipboard text using `arboard`.
pub fn get_clipboard_text() -> Result<String, Box<dyn std::error::Error>> {
    // Reading from clipboard is generally restricted for security reasons in terminals via OSC52.
    // We primarily rely on arboard for pasting. If the user uses Kitty, they will likely just use Cmd+V / Shift+Insert to paste into the terminal which triggers `CrosstermEvent::Paste`.
    let mut clipboard = arboard::Clipboard::new()?;
    let text = clipboard.get_text()?;
    Ok(text)
}
