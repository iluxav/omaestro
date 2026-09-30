//! Turning an `mlua::Error` into the one line a user needs:
//! `rules.d/foo.lua:12: <message>`.

use mlua::Error;

/// Chunk name of `om eval` code.
pub const EVAL_CHUNK: &str = "eval";
/// Chunk name of `lua/prelude.lua`.
pub const PRELUDE_CHUNK: &str = "prelude";

/// The message with the file and line of the rule that failed.
///
/// Errors raised by Lua itself (`error(...)`, indexing nil, syntax) already
/// carry their position. Errors returned by our Rust functions do not, so the
/// position is taken from the traceback mlua captured at the failing call.
pub fn describe(err: &Error) -> String {
    let mut traceback = None;
    let mut root = err;
    loop {
        match root {
            Error::CallbackError {
                traceback: tb,
                cause,
            } => {
                // Innermost wins: it is the call that actually failed.
                traceback = Some(tb.as_str());
                root = cause;
            }
            Error::WithContext { cause, .. } => root = cause,
            _ => break,
        }
    }
    let message = match root {
        Error::RuntimeError(message) | Error::SyntaxError { message, .. } => message.clone(),
        other => other.to_string(),
    };
    // Lua appends its own traceback to errors that reach the top of a chunk.
    let (message, lua_traceback) = match message.split_once("\nstack traceback:") {
        Some((message, traceback)) => (message, Some(traceback)),
        None => (message.as_str(), None),
    };
    let message = message.trim_end();
    if has_position(message) {
        return message.to_string();
    }
    match traceback.or(lua_traceback).and_then(first_rule_frame) {
        Some(position) => format!("{position}: {message}"),
        None => message.to_string(),
    }
}

/// True when the message starts with `<chunk>:<line>: `.
pub fn has_position(message: &str) -> bool {
    split_position(message).is_some()
}

/// Splits `<chunk>:<line>: rest` into (`<chunk>:<line>`, `rest`). Only our
/// own chunk names count, so a message like `port 80: refused` is not
/// mistaken for a position.
fn split_position(line: &str) -> Option<(&str, &str)> {
    let mut from = 0;
    while let Some(colon) = line[from..].find(':') {
        let chunk_end = from + colon;
        let after = &line[chunk_end + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with(": ") && is_chunk_name(&line[..chunk_end]) {
            let position_end = chunk_end + 1 + digits;
            return Some((&line[..position_end], &line[position_end + 2..]));
        }
        from = chunk_end + 1;
    }
    None
}

/// Rule files are loaded as `@<name>.lua`; `om eval` and the prelude are the
/// two chunks without a file.
fn is_chunk_name(name: &str) -> bool {
    name.ends_with(".lua") || name == EVAL_CHUNK || name == PRELUDE_CHUNK
}

/// The first traceback frame that is Lua code the user wrote: not a C
/// function, not mlua's own async glue.
fn first_rule_frame(traceback: &str) -> Option<&str> {
    traceback
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('[') && !line.starts_with("__mlua"))
        .find_map(|line| split_position(line).map(|(position, _)| position))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const TRACEBACK: &str = "stack traceback:\n\t[C]: in function 'om.notify'\n\t__mlua_async_poll:16: in function <__mlua_async_poll:1>\n\trules.d/foo.lua:12: in function <rules.d/foo.lua:10>";

    #[test]
    fn lua_errors_keep_their_own_position() {
        let err = Error::RuntimeError("rules.d/foo.lua:3: attempt to index a nil value".into());
        assert_eq!(
            describe(&err),
            "rules.d/foo.lua:3: attempt to index a nil value"
        );
    }

    #[test]
    fn rust_errors_get_the_position_of_the_calling_rule() {
        let err = Error::CallbackError {
            traceback: TRACEBACK.into(),
            cause: Arc::new(Error::RuntimeError("notify-send is not installed".into())),
        };
        assert_eq!(
            describe(&err),
            "rules.d/foo.lua:12: notify-send is not installed"
        );
    }

    #[test]
    fn innermost_traceback_wins() {
        let inner = Error::CallbackError {
            traceback: TRACEBACK.into(),
            cause: Arc::new(Error::RuntimeError("boom".into())),
        };
        let outer = Error::CallbackError {
            traceback: "stack traceback:\n\tinit.lua:1: in main chunk".into(),
            cause: Arc::new(inner),
        };
        assert_eq!(describe(&outer), "rules.d/foo.lua:12: boom");
    }

    #[test]
    fn lua_traceback_is_cut_off_and_used_for_the_position() {
        let with_position = Error::RuntimeError(
            "init.lua:2: boom\nstack traceback:\n\t[C]: in function 'error'".into(),
        );
        assert_eq!(describe(&with_position), "init.lua:2: boom");
        let without = Error::RuntimeError(
            "bare\nstack traceback:\n\t[C]: in function 'error'\n\trules.d/odd.lua:7: in main chunk".into(),
        );
        assert_eq!(describe(&without), "rules.d/odd.lua:7: bare");
    }

    #[test]
    fn no_position_anywhere_leaves_the_message_alone() {
        assert_eq!(describe(&Error::RuntimeError("boom".into())), "boom");
    }

    #[test]
    fn position_detection() {
        assert!(has_position("init.lua:1: x"));
        assert!(has_position("rules.d/a b.lua:120: x"));
        assert!(has_position("eval:1: x"));
        assert!(!has_position("no position here"));
        assert!(has_position("prelude:7: x"));
        assert!(!has_position("time 12:30: not a position"));
        assert!(!has_position("port 80: refused"));
        assert!(!has_position(":3: x"));
    }
}
