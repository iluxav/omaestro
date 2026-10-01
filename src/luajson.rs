//! Lua values as JSON and back, for `om.json` and `om.store`.

use mlua::{Lua, Result, Table, Value};
use serde_json::{Map, Number, Value as Json};

/// A Lua value as JSON. A table with keys 1..n is an array; any other
/// table is an object with string keys; an empty table is `{}`. Functions
/// and userdata cannot be represented and become an error.
pub fn to_json(value: &Value) -> Result<Json> {
    Ok(match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(*b),
        Value::Integer(i) => Json::from(*i),
        Value::Number(n) => Number::from_f64(*n)
            .map(Json::Number)
            .ok_or_else(|| mlua::Error::runtime(format!("{n} cannot be represented in JSON")))?,
        Value::String(s) => Json::String(s.to_str()?.to_string()),
        Value::Table(table) => table_to_json(table)?,
        other => {
            return Err(mlua::Error::runtime(format!(
                "a {} cannot be represented in JSON",
                other.type_name()
            )));
        }
    })
}

fn table_to_json(table: &Table) -> Result<Json> {
    let len = table.raw_len();
    let mut count = 0;
    for pair in table.pairs::<Value, Value>() {
        pair?;
        count += 1;
    }
    if len > 0 && count == len {
        let mut items = Vec::with_capacity(len);
        for index in 1..=len {
            items.push(to_json(&table.raw_get::<Value>(index)?)?);
        }
        return Ok(Json::Array(items));
    }
    let mut object = Map::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let key = match key {
            Value::String(s) => s.to_str()?.to_string(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            other => {
                return Err(mlua::Error::runtime(format!(
                    "a table key of type {} cannot be a JSON key",
                    other.type_name()
                )));
            }
        };
        object.insert(key, to_json(&value)?);
    }
    Ok(Json::Object(object))
}

/// JSON as a Lua value. `null` becomes nil, so a null inside an array
/// leaves a hole.
pub fn from_json(lua: &Lua, json: &Json) -> Result<Value> {
    Ok(match json {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => Value::String(lua.create_string(s)?),
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (index, item) in items.iter().enumerate() {
                table.raw_set(index + 1, from_json(lua, item)?)?;
            }
            Value::Table(table)
        }
        Json::Object(object) => {
            let table = lua.create_table()?;
            for (key, value) in object {
                table.raw_set(key.as_str(), from_json(lua, value)?)?;
            }
            Value::Table(table)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(code: &str) -> String {
        let lua = Lua::new();
        let value: Value = lua.load(code).eval().unwrap();
        let json = to_json(&value).unwrap();
        let back = from_json(&lua, &json).unwrap();
        assert_eq!(
            to_json(&back).unwrap(),
            json,
            "second trip differs for {code}"
        );
        json.to_string()
    }

    #[test]
    fn scalars_and_tables() {
        assert_eq!(roundtrip("return nil"), "null");
        assert_eq!(roundtrip("return true"), "true");
        assert_eq!(roundtrip("return 42"), "42");
        assert_eq!(roundtrip("return 2.5"), "2.5");
        assert_eq!(roundtrip("return 'text'"), "\"text\"");
        assert_eq!(roundtrip("return {1, 'two', {3}}"), "[1,\"two\",[3]]");
        assert_eq!(roundtrip("return {}"), "{}");
        assert_eq!(
            roundtrip("return {name = 'x', n = 1}"),
            "{\"n\":1,\"name\":\"x\"}"
        );
        assert_eq!(roundtrip("return {[10] = 'sparse'}"), "{\"10\":\"sparse\"}");
    }

    #[test]
    fn what_json_cannot_hold() {
        let lua = Lua::new();
        let value: Value = lua.load("return print").eval().unwrap();
        assert!(
            to_json(&value)
                .unwrap_err()
                .to_string()
                .contains("function")
        );
        let value: Value = lua.load("return {[true] = 1}").eval().unwrap();
        assert!(to_json(&value).unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn json_text_in() {
        let lua = Lua::new();
        let json: Json =
            serde_json::from_str(r#"{"a": [1, 2.5, null, "s"], "b": {"c": false}}"#).unwrap();
        lua.globals()
            .set("v", from_json(&lua, &json).unwrap())
            .unwrap();
        let got: String = lua
            .load("return v.a[1] .. ',' .. v.a[2] .. ',' .. tostring(v.a[3]) .. ',' .. v.a[4] .. ',' .. tostring(v.b.c)")
            .eval()
            .unwrap();
        assert_eq!(got, "1,2.5,nil,s,false");
    }
}
