//! Key chords: modifiers plus one key. Parsed once, then rendered the way
//! Hyprland wants them, for binds and for `send_shortcut`.

use std::fmt;

/// Modifier bits, numbered as in Hyprland's `modmask`.
const MODS: [(u32, &str, &[&str]); 8] = [
    (64, "SUPER", &["SUPER", "WIN", "LOGO", "MOD4"]),
    (4, "CTRL", &["CTRL", "CONTROL"]),
    (8, "ALT", &["ALT", "MOD1"]),
    (1, "SHIFT", &["SHIFT"]),
    (2, "CAPS", &["CAPS"]),
    (16, "MOD2", &["MOD2"]),
    (32, "MOD3", &["MOD3"]),
    (128, "MOD5", &["MOD5", "ALTGR"]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    mods: u32,
    key: String,
}

impl Chord {
    /// Parses `SUPER + SHIFT + J` (Hyprland 0.56), `SUPER SHIFT, J` (the older
    /// Hyprland form) and `ctrl+shift+v`. Modifier names are case-insensitive.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (mod_names, key): (Vec<&str>, &str) = match text.rsplit_once(',') {
            Some((mods, key)) => (
                mods.split(|c: char| c.is_whitespace() || c == '+' || c == '_')
                    .collect(),
                key,
            ),
            None => {
                let mut parts: Vec<&str> = text.split('+').collect();
                let key = parts.pop().unwrap_or_default();
                (parts, key)
            }
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("'{text}' has no key"));
        }
        let mut mods = 0;
        for name in mod_names.iter().map(|m| m.trim()).filter(|m| !m.is_empty()) {
            let (bit, _, _) = MODS
                .iter()
                .find(|(_, _, aliases)| aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
                .ok_or_else(|| format!("unknown modifier '{name}' in '{text}'"))?;
            mods |= bit;
        }
        Ok(Self {
            mods,
            key: key.to_string(),
        })
    }

    /// From a row of `hyprctl -j binds`.
    pub fn from_modmask(modmask: u32, key: &str) -> Self {
        Self {
            mods: modmask,
            key: key.to_string(),
        }
    }

    fn mod_names(&self) -> impl Iterator<Item = &'static str> {
        MODS.iter()
            .filter(|(bit, _, _)| self.mods & bit != 0)
            .map(|(_, name, _)| *name)
    }

    /// Single letters compare and print in upper case, so `super+j` and
    /// `SUPER + J` are the same chord.
    fn key_name(&self) -> String {
        if self.key.len() == 1 {
            self.key.to_ascii_uppercase()
        } else {
            self.key.clone()
        }
    }

    /// The form `hl.bind` and `hl.unbind` take: `SUPER + SHIFT + J`.
    pub fn hyprland(&self) -> String {
        let mut parts: Vec<String> = self.mod_names().map(str::to_string).collect();
        parts.push(self.key_name());
        parts.join(" + ")
    }

    /// Compact form for ids: `SUPER+SHIFT+J`.
    pub fn id(&self) -> String {
        self.hyprland().replace(" + ", "+")
    }

    /// Whether both chords are the same physical combination.
    pub fn same_keys(&self, other: &Chord) -> bool {
        self.mods == other.mods && self.key.eq_ignore_ascii_case(&other.key)
    }

    /// The modifiers as Hyprland's `send_shortcut` takes them: `CTRL SHIFT`.
    pub fn mods_hyprland(&self) -> String {
        self.mod_names().collect::<Vec<_>>().join(" ")
    }

    /// The key as Hyprland's `send_shortcut` takes it.
    pub fn key_hyprland(&self) -> String {
        self.key_name()
    }

    /// The chord as `wtype` arguments: `-M ctrl -M shift -k v -m shift -m
    /// ctrl`. `None` for a modifier wtype has no name for (MOD2, MOD3).
    pub fn wtype_args(&self) -> Option<Vec<String>> {
        let mods = self
            .mod_names()
            .map(|name| match name {
                "SUPER" => Some("logo"),
                "CTRL" => Some("ctrl"),
                "ALT" => Some("alt"),
                "SHIFT" => Some("shift"),
                "CAPS" => Some("capslock"),
                "MOD5" => Some("altgr"),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        // A letter's keysym is the lower-case one; shift is a modifier above.
        let key = if self.key.len() == 1 {
            self.key.to_ascii_lowercase()
        } else {
            self.key.clone()
        };
        let mut args = Vec::new();
        for m in &mods {
            args.extend(["-M".to_string(), m.to_string()]);
        }
        args.extend(["-k".to_string(), key]);
        for m in mods.iter().rev() {
            args.extend(["-m".to_string(), m.to_string()]);
        }
        Some(args)
    }
}

/// A key press that produces one character: `(modifiers, key)` in the form
/// `send_shortcut` takes. Assumes a US layout for punctuation.
pub type KeyPress = (&'static str, String);

/// The key press that types `c`, if a US layout has one.
pub fn key_for_char(c: char) -> Option<KeyPress> {
    const PLAIN: [(char, &str); 14] = [
        (' ', "space"),
        ('\n', "Return"),
        ('\t', "Tab"),
        ('`', "grave"),
        ('-', "minus"),
        ('=', "equal"),
        ('[', "bracketleft"),
        (']', "bracketright"),
        ('\\', "backslash"),
        (';', "semicolon"),
        ('\'', "apostrophe"),
        (',', "comma"),
        ('.', "period"),
        ('/', "slash"),
    ];
    // Shifted symbols, with the key they sit on.
    const SHIFTED: [(char, &str); 21] = [
        ('~', "grave"),
        ('!', "1"),
        ('@', "2"),
        ('#', "3"),
        ('$', "4"),
        ('%', "5"),
        ('^', "6"),
        ('&', "7"),
        ('*', "8"),
        ('(', "9"),
        (')', "0"),
        ('_', "minus"),
        ('+', "equal"),
        ('{', "bracketleft"),
        ('}', "bracketright"),
        ('|', "backslash"),
        (':', "semicolon"),
        ('"', "apostrophe"),
        ('<', "comma"),
        ('>', "period"),
        ('?', "slash"),
    ];
    if c.is_ascii_lowercase() || c.is_ascii_digit() {
        return Some(("", c.to_string()));
    }
    if c.is_ascii_uppercase() {
        return Some(("SHIFT", c.to_ascii_lowercase().to_string()));
    }
    if let Some((_, key)) = PLAIN.iter().find(|(ch, _)| *ch == c) {
        return Some(("", key.to_string()));
    }
    SHIFTED
        .iter()
        .find(|(ch, _)| *ch == c)
        .map(|(_, key)| ("SHIFT", key.to_string()))
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hyprland())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_spellings_of_a_chord_agree() {
        let forms = [
            "SUPER + SHIFT + J",
            "SUPER SHIFT, J",
            "SUPER_SHIFT, J",
            "super+shift+j",
            "shift + win + j",
        ];
        for form in forms {
            let chord = Chord::parse(form).unwrap();
            assert_eq!(chord.hyprland(), "SUPER + SHIFT + J", "{form}");
            assert_eq!(chord.id(), "SUPER+SHIFT+J", "{form}");
        }
    }

    #[test]
    fn key_without_modifiers() {
        assert_eq!(Chord::parse("F12").unwrap().hyprland(), "F12");
        assert_eq!(Chord::parse(", Print").unwrap().hyprland(), "Print");
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert_eq!(
            Chord::parse("HYPER + J").unwrap_err(),
            "unknown modifier 'HYPER' in 'HYPER + J'"
        );
        assert_eq!(
            Chord::parse("SUPER + ").unwrap_err(),
            "'SUPER + ' has no key"
        );
        assert_eq!(Chord::parse("").unwrap_err(), "'' has no key");
    }

    #[test]
    fn matches_rows_of_hyprctl_binds() {
        let chord = Chord::parse("SUPER + J").unwrap();
        assert!(chord.same_keys(&Chord::from_modmask(64, "J")));
        assert!(chord.same_keys(&Chord::from_modmask(64, "j")));
        assert!(!chord.same_keys(&Chord::from_modmask(65, "J")));
        assert!(!chord.same_keys(&Chord::from_modmask(64, "K")));
        assert_eq!(
            Chord::from_modmask(77, "F20").hyprland(),
            "SUPER + CTRL + ALT + SHIFT + F20"
        );
    }

    #[test]
    fn characters_as_key_presses() {
        let press = |c| key_for_char(c).map(|(m, k)| format!("{m}|{k}"));
        assert_eq!(press('a').as_deref(), Some("|a"));
        assert_eq!(press('Q').as_deref(), Some("SHIFT|q"));
        assert_eq!(press('7').as_deref(), Some("|7"));
        assert_eq!(press(' ').as_deref(), Some("|space"));
        assert_eq!(press('\n').as_deref(), Some("|Return"));
        assert_eq!(press('-').as_deref(), Some("|minus"));
        assert_eq!(press('_').as_deref(), Some("SHIFT|minus"));
        assert_eq!(press('!').as_deref(), Some("SHIFT|1"));
        assert_eq!(press('"').as_deref(), Some("SHIFT|apostrophe"));
        assert_eq!(press('ü'), None);
        assert_eq!(press('✓'), None);
    }

    #[test]
    fn parts_for_send_shortcut() {
        let chord = Chord::parse("ctrl+shift+v").unwrap();
        assert_eq!(chord.mods_hyprland(), "CTRL SHIFT");
        assert_eq!(chord.key_hyprland(), "V");
        let bare = Chord::parse("Return").unwrap();
        assert_eq!(bare.mods_hyprland(), "");
        assert_eq!(bare.key_hyprland(), "Return");
    }

    #[test]
    fn parts_for_wtype() {
        let args = |text| {
            Chord::parse(text)
                .unwrap()
                .wtype_args()
                .map(|a| a.join(" "))
        };
        assert_eq!(
            args("ctrl+shift+v").as_deref(),
            Some("-M ctrl -M shift -k v -m shift -m ctrl")
        );
        assert_eq!(
            args("SUPER + ALT + J").as_deref(),
            Some("-M logo -M alt -k j -m alt -m logo")
        );
        assert_eq!(args("Return").as_deref(), Some("-k Return"));
        // wtype's names: capslock, altgr.
        assert_eq!(
            args("CAPS + MOD5 + a").as_deref(),
            Some("-M capslock -M altgr -k a -m altgr -m capslock")
        );
        assert_eq!(args("MOD3 + x"), None);
    }
}
