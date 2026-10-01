//! `om.on_typed`: the last few characters typed, matched against the
//! registered texts. The keys come from the evdev backend; this part is
//! pure and runs in tests.

#![cfg_attr(not(feature = "typed"), allow(dead_code))]

/// One key the user typed, already resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Backspace,
    /// Anything that is not text (an arrow, a modifier chord, Enter): the
    /// buffer starts over.
    Reset,
}

/// A text a rule waits for.
#[derive(Debug, Clone, PartialEq)]
pub struct Watched {
    pub id: String,
    pub text: String,
}

/// The longest text a rule may watch for.
pub const MAX_TEXT: usize = 32;

/// The recent keystrokes. Never logged, never longer than the longest text
/// watched for, and empty when nothing is watched.
#[derive(Default)]
pub struct Typed {
    buffer: String,
}

impl Typed {
    /// Feeds one key. If the buffer now ends with a watched text, that
    /// match is returned and the buffer is cleared.
    pub fn feed(&mut self, key: Key, watched: &[Watched]) -> Option<Watched> {
        if watched.is_empty() {
            self.buffer.clear();
            return None;
        }
        match key {
            Key::Char(c) => self.buffer.push(c),
            Key::Backspace => {
                self.buffer.pop();
                return None;
            }
            Key::Reset => {
                self.buffer.clear();
                return None;
            }
        }
        let max = watched
            .iter()
            .map(|w| w.text.len())
            .max()
            .unwrap_or(0)
            .min(MAX_TEXT);
        while self.buffer.len() > max {
            let first = self.buffer.chars().next().map_or(1, char::len_utf8);
            self.buffer.drain(..first);
        }
        // The longest match wins when several texts end the same way.
        let hit = watched
            .iter()
            .filter(|w| self.buffer.ends_with(&w.text))
            .max_by_key(|w| w.text.len())
            .cloned();
        if hit.is_some() {
            self.buffer.clear();
        }
        hit
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// What a rule may watch for: 1 to `MAX_TEXT` printable ASCII characters,
/// because the key mapping knows a US layout.
pub fn check_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("the text is empty".to_string());
    }
    if text.len() > MAX_TEXT {
        return Err(format!("the text is longer than {MAX_TEXT} characters"));
    }
    if let Some(c) = text.chars().find(|c| !c.is_ascii_graphic() && *c != ' ') {
        return Err(format!(
            "'{c}' cannot be typed on a US layout, only printable ASCII"
        ));
    }
    Ok(())
}

/// Linux input key codes of a US keyboard as characters. `shift` covers
/// Shift and Caps Lock (for letters).
pub fn key_to_char(code: u16, shift: bool) -> Option<Key> {
    const ROW_DIGITS: &str = "1234567890-=";
    const ROW_DIGITS_SHIFT: &str = "!@#$%^&*()_+";
    const ROW_Q: &str = "qwertyuiop[]";
    const ROW_Q_SHIFT: &str = "QWERTYUIOP{}";
    const ROW_A: &str = "asdfghjkl;'`";
    const ROW_A_SHIFT: &str = "ASDFGHJKL:\"~";
    const ROW_Z: &str = "zxcvbnm,./";
    const ROW_Z_SHIFT: &str = "ZXCVBNM<>?";
    let pick = |plain: &str, shifted: &str, index: u16| {
        let row = if shift { shifted } else { plain };
        row.chars().nth(index as usize).map(Key::Char)
    };
    match code {
        2..=13 => pick(ROW_DIGITS, ROW_DIGITS_SHIFT, code - 2),
        14 => Some(Key::Backspace),
        16..=27 => pick(ROW_Q, ROW_Q_SHIFT, code - 16),
        30..=41 => pick(ROW_A, ROW_A_SHIFT, code - 30),
        43 => Some(Key::Char(if shift { '|' } else { '\\' })),
        44..=53 => pick(ROW_Z, ROW_Z_SHIFT, code - 44),
        57 => Some(Key::Char(' ')),
        // Modifiers on their own are not keys the buffer cares about.
        29 | 42 | 54 | 56 | 58 | 97 | 100 | 125 | 126 => None,
        _ => Some(Key::Reset),
    }
}

/// Key codes that are modifiers, with whether they are a Shift.
pub fn modifier(code: u16) -> Option<Modifier> {
    match code {
        42 | 54 => Some(Modifier::Shift),
        58 => Some(Modifier::CapsLock),
        29 | 56 | 97 | 100 | 125 | 126 => Some(Modifier::Other),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Modifier {
    Shift,
    CapsLock,
    /// Ctrl, Alt, Super: while held, keys are shortcuts, not text.
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watched(texts: &[&str]) -> Vec<Watched> {
        texts
            .iter()
            .map(|t| Watched {
                id: format!("typed:{t}"),
                text: t.to_string(),
            })
            .collect()
    }

    fn type_text(typed: &mut Typed, text: &str, watched: &[Watched]) -> Vec<String> {
        text.chars()
            .filter_map(|c| typed.feed(Key::Char(c), watched).map(|w| w.text))
            .collect()
    }

    #[test]
    fn matches_the_end_of_what_was_typed() {
        let watched = watched(&[":sig", ":date"]);
        let mut typed = Typed::default();
        assert_eq!(type_text(&mut typed, "hello :sig", &watched), [":sig"]);
        // The buffer was cleared by the match: typing the tail again is not a match.
        assert_eq!(type_text(&mut typed, "ig", &watched), Vec::<String>::new());
        assert_eq!(type_text(&mut typed, " and :date", &watched), [":date"]);
    }

    #[test]
    fn backspace_edits_and_other_keys_reset() {
        let watched = watched(&[":sig"]);
        let mut typed = Typed::default();
        type_text(&mut typed, ":sif", &watched);
        assert_eq!(typed.feed(Key::Backspace, &watched), None);
        assert_eq!(
            typed.feed(Key::Char('g'), &watched).map(|w| w.text),
            Some(":sig".into())
        );

        type_text(&mut typed, ":si", &watched);
        assert_eq!(typed.feed(Key::Reset, &watched), None);
        assert_eq!(typed.feed(Key::Char('g'), &watched), None);
    }

    #[test]
    fn the_longest_text_wins_and_the_buffer_stays_short() {
        let watched = watched(&["sig", "::sig"]);
        let mut typed = Typed::default();
        assert_eq!(type_text(&mut typed, "x::sig", &watched), ["::sig"]);
        let mut typed = Typed::default();
        let long = "a".repeat(200);
        type_text(&mut typed, &long, &watched);
        assert!(typed.buffer.len() <= 5);
        assert_eq!(typed.feed(Key::Char('x'), &[]), None);
        assert!(typed.buffer.is_empty(), "nothing watched, nothing kept");
    }

    #[test]
    fn texts_are_checked() {
        assert_eq!(check_text(":sig"), Ok(()));
        assert_eq!(check_text(""), Err("the text is empty".into()));
        assert!(check_text(&"x".repeat(33)).unwrap_err().contains("longer"));
        assert!(check_text("ünïcode").unwrap_err().contains("US layout"));
    }

    #[test]
    fn us_keys() {
        assert_eq!(key_to_char(30, false), Some(Key::Char('a')));
        assert_eq!(key_to_char(30, true), Some(Key::Char('A')));
        assert_eq!(key_to_char(2, true), Some(Key::Char('!')));
        assert_eq!(key_to_char(39, true), Some(Key::Char(':')));
        assert_eq!(key_to_char(57, false), Some(Key::Char(' ')));
        assert_eq!(key_to_char(14, false), Some(Key::Backspace));
        assert_eq!(key_to_char(28, false), Some(Key::Reset), "Enter resets");
        assert_eq!(key_to_char(42, false), None, "Shift alone is nothing");
        assert_eq!(modifier(42), Some(Modifier::Shift));
        assert_eq!(modifier(125), Some(Modifier::Other));
        assert_eq!(modifier(30), None);
    }
}
