//! Key events → terminal input bytes (xterm conventions).

use egui::{Key, Modifiers};

/// Encode a key press. `None` means the key produces no bytes (text comes
/// through `egui::Event::Text` instead).
pub fn encode(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = mods.ctrl || mods.command;
    let shift = mods.shift;
    let alt = mods.alt;

    // Modifier parameter for CSI sequences (xterm): 1 + shift(1) + alt(2) + ctrl(4).
    let param = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let csi = |suffix: &str| -> Vec<u8> {
        if param == 1 {
            format!("\x1b[{suffix}").into_bytes()
        } else {
            format!("\x1b[1;{param}{suffix}").into_bytes()
        }
    };
    let ss3 = |ch: char| -> Vec<u8> {
        if param == 1 {
            format!("\x1bO{ch}").into_bytes()
        } else {
            format!("\x1b[1;{param}{ch}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if param == 1 {
            format!("\x1b[{code}~").into_bytes()
        } else {
            format!("\x1b[{code};{param}~").into_bytes()
        }
    };

    let bytes = match key {
        Key::Enter => {
            if alt {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        Key::Tab => {
            if shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        Key::Backspace => {
            if alt {
                b"\x1b\x7f".to_vec()
            } else if ctrl {
                b"\x08".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => {
            if app_cursor && param == 1 {
                ss3('A')
            } else {
                csi("A")
            }
        }
        Key::ArrowDown => {
            if app_cursor && param == 1 {
                ss3('B')
            } else {
                csi("B")
            }
        }
        Key::ArrowRight => {
            if app_cursor && param == 1 {
                ss3('C')
            } else {
                csi("C")
            }
        }
        Key::ArrowLeft => {
            if app_cursor && param == 1 {
                ss3('D')
            } else {
                csi("D")
            }
        }
        Key::Home => {
            if app_cursor && param == 1 {
                ss3('H')
            } else {
                csi("H")
            }
        }
        Key::End => {
            if app_cursor && param == 1 {
                ss3('F')
            } else {
                csi("F")
            }
        }
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => ss3('P'),
        Key::F2 => ss3('Q'),
        Key::F3 => ss3('R'),
        Key::F4 => ss3('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        Key::Space if ctrl => vec![0],
        _ if ctrl && !alt => {
            // Ctrl+letter → control code; Ctrl+[ ] \ etc.
            let c = key_to_ascii(key)?;
            match c {
                'a'..='z' => vec![c as u8 - b'a' + 1],
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                '/' => vec![0x1f],
                _ => return None,
            }
        }
        _ if alt => {
            // Alt+key → ESC prefix + the key's text (the Text event is suppressed by the widget).
            let c = key_to_ascii(key)?;
            let c = if shift { c.to_ascii_uppercase() } else { c };
            vec![0x1b, c as u8]
        }
        _ => return None,
    };
    Some(bytes)
}

fn key_to_ascii(key: Key) -> Option<char> {
    use Key::*;
    Some(match key {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        Num0 => '0',
        Num1 => '1',
        Num2 => '2',
        Num3 => '3',
        Num4 => '4',
        Num5 => '5',
        Num6 => '6',
        Num7 => '7',
        Num8 => '8',
        Num9 => '9',
        OpenBracket => '[',
        CloseBracket => ']',
        Backslash => '\\',
        Slash => '/',
        Minus => '-',
        Equals => '=',
        Semicolon => ';',
        Quote => '\'',
        Comma => ',',
        Period => '.',
        Backtick => '`',
        Space => ' ',
        _ => return None,
    })
}

/// Paste text, with bracketed-paste markers when the application asked for them.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let normalised = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let mut v = b"\x1b[200~".to_vec();
        v.extend_from_slice(normalised.as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        normalised.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_follow_application_cursor_mode() {
        assert_eq!(
            encode(Key::ArrowUp, Modifiers::NONE, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode(Key::ArrowUp, Modifiers::NONE, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode(Key::ArrowUp, Modifiers::SHIFT, true),
            Some(b"\x1b[1;2A".to_vec())
        );
        assert_eq!(
            encode(Key::ArrowLeft, Modifiers::CTRL, false),
            Some(b"\x1b[1;5D".to_vec())
        );
    }

    #[test]
    fn control_and_alt_combinations() {
        assert_eq!(encode(Key::C, Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(encode(Key::Z, Modifiers::CTRL, false), Some(vec![26]));
        assert_eq!(
            encode(Key::OpenBracket, Modifiers::CTRL, false),
            Some(vec![0x1b])
        );
        assert_eq!(
            encode(Key::X, Modifiers::ALT, false),
            Some(vec![0x1b, b'x'])
        );
        assert_eq!(encode(Key::A, Modifiers::NONE, false), None);
        assert_eq!(
            encode(Key::Tab, Modifiers::SHIFT, false),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            encode(Key::F5, Modifiers::NONE, false),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            encode(Key::Delete, Modifiers::SHIFT, false),
            Some(b"\x1b[3;2~".to_vec())
        );
    }

    #[test]
    fn paste_is_bracketed_on_request_and_newlines_become_cr() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(paste_bytes("x", true), b"\x1b[200~x\x1b[201~".to_vec());
    }
}
