//! Lua values as text, for `om eval`.

use mlua::{Table, Value};

const MAX_DEPTH: usize = 4;

/// A top-level string prints raw (like `print`); inside a table it is quoted.
/// Tables print on one line, array part first, then keys in sorted order.
pub fn render(value: &Value) -> String {
    match value {
        Value::String(s) => s.to_string_lossy(),
        other => render_nested(other, 0),
    }
}

fn render_nested(value: &Value, depth: usize) -> String {
    match value {
        Value::String(s) => format!("{:?}", s.to_string_lossy()),
        Value::Table(table) if depth < MAX_DEPTH => render_table(table, depth),
        Value::Table(_) => "{...}".to_string(),
        other => other
            .to_string()
            .unwrap_or_else(|_| other.type_name().to_string()),
    }
}

fn render_table(table: &Table, depth: usize) -> String {
    let len = table.raw_len();
    let mut items = Vec::new();
    for index in 1..=len {
        let value = table.raw_get::<Value>(index).unwrap_or(Value::Nil);
        items.push(render_nested(&value, depth + 1));
    }

    let mut keyed = Vec::new();
    for (key, value) in table.pairs::<Value, Value>().flatten() {
        let key = match &key {
            Value::Integer(i) if usize::try_from(*i).is_ok_and(|i| (1..=len).contains(&i)) => {
                continue;
            }
            Value::String(s) if is_identifier(&s.to_string_lossy()) => s.to_string_lossy(),
            other => format!("[{}]", render_nested(other, depth + 1)),
        };
        keyed.push(format!("{key} = {}", render_nested(&value, depth + 1)));
    }
    keyed.sort();
    items.extend(keyed);
    format!("{{{}}}", items.join(", "))
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;

    fn rendered(code: &str) -> String {
        let lua = Lua::new();
        render(&lua.load(code).eval::<Value>().unwrap())
    }

    #[test]
    fn scalars() {
        assert_eq!(rendered("return 1 + 1"), "2");
        assert_eq!(rendered("return 1.5"), "1.5");
        assert_eq!(rendered("return 'plain text'"), "plain text");
        assert_eq!(rendered("return nil"), "nil");
        assert_eq!(rendered("return true"), "true");
    }

    #[test]
    fn tables_are_stable_and_readable() {
        assert_eq!(
            rendered(
                "return {10, 'x', class = 'firefox', floating = false, ['two words'] = 1, [10] = true}"
            ),
            r#"{10, "x", ["two words"] = 1, [10] = true, class = "firefox", floating = false}"#
        );
        assert_eq!(rendered("return {}"), "{}");
    }

    #[test]
    fn cycles_stop_at_the_depth_limit() {
        assert_eq!(
            rendered("local t = {} t.me = t return t"),
            "{me = {me = {me = {me = {...}}}}}"
        );
    }
}
