use std::io::{self, Write};

/// Build an OSC 52 escape sequence that asks the controlling terminal to
/// place `content` on the system clipboard. Most modern terminals (kitty,
/// alacritty, wezterm, iTerm2, foot, modern xterm) honor this directly;
/// tmux forwards it when `set-clipboard on` is configured. Older terminals
/// silently ignore the sequence.
pub fn format_osc52(content: &str) -> String {
    let encoded = base64_encode(content.as_bytes());
    format!("\x1b]52;c;{encoded}\x1b\\")
}

/// Put `content` on the system clipboard. On this machine that's the
/// platform's own tool (`pbcopy`, `wl-copy`, `xclip`, `xsel`, `clip`): it
/// works inside tmux or zellij, which drop OSC 52 by default. OSC 52 is
/// sent as well, for a session over SSH, whose clipboard is the terminal's
/// end. Errors only surface when neither way could be tried.
pub fn copy(content: &str) -> io::Result<()> {
    let native = native_copy(content);
    let mut stdout = io::stdout();
    let osc = stdout
        .write_all(format_osc52(content).as_bytes())
        .and_then(|()| stdout.flush());
    if native { Ok(()) } else { osc }
}

/// The clipboard tools to try here, in order. None over SSH: there the
/// clipboard that matters is the one on the other end.
fn native_tools() -> Vec<(&'static str, &'static [&'static str])> {
    let env = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if env("SSH_CONNECTION") || env("SSH_TTY") {
        return Vec::new();
    }
    let mut tools: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if cfg!(target_os = "macos") {
        tools.push(("pbcopy", &[]));
    } else if cfg!(windows) {
        tools.push(("clip", &[]));
    } else {
        if env("WAYLAND_DISPLAY") {
            tools.push(("wl-copy", &[]));
        }
        if env("DISPLAY") {
            tools.push(("xclip", &["-selection", "clipboard"]));
            tools.push(("xsel", &["--clipboard", "--input"]));
        }
    }
    tools
}

/// Pipe `content` into the first clipboard tool that takes it.
fn native_copy(content: &str) -> bool {
    use std::process::{Command, Stdio};
    for (tool, args) in native_tools() {
        let Ok(mut child) = Command::new(tool)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let wrote = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(content.as_bytes()).is_ok());
        if child.wait().is_ok_and(|s| s.success()) && wrote {
            return true;
        }
    }
    false
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_wraps_base64_payload_in_escape_sequence() {
        // "hi" → "aGk=" in base64. The full sequence is the OSC introducer
        // (ESC ]), the 52;c; selector for the system clipboard, the payload,
        // and the ST terminator (ESC \).
        assert_eq!(format_osc52("hi"), "\x1b]52;c;aGk=\x1b\\");
    }

    #[test]
    fn osc52_handles_empty_input() {
        assert_eq!(format_osc52(""), "\x1b]52;c;\x1b\\");
    }

    #[test]
    fn base64_known_vectors() {
        // RFC 4648 test vectors plus a UTF-8 case to confirm we encode bytes,
        // not chars.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode("café".as_bytes()), "Y2Fmw6k=");
    }
}
