//! `om.json`: encode and decode. `om.store`: values that survive reloads
//! and restarts, in one JSON file.

use std::fs;
use std::path::{Path, PathBuf};

use mlua::{Error, Lua, Result, Table, Value};
use serde_json::{Map, Value as Json};

use super::Context;
use crate::luajson::{from_json, to_json};

const STORE_FILE: &str = "store.json";

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let json = lua.create_table()?;
    json.set(
        "encode",
        lua.create_function(|_, (value, options): (Value, Option<Table>)| {
            let json = to_json(&value)?;
            let pretty = options
                .map(|o| o.get::<Option<bool>>("pretty"))
                .transpose()?
                .flatten();
            if pretty.unwrap_or(false) {
                serde_json::to_string_pretty(&json)
            } else {
                serde_json::to_string(&json)
            }
            .map_err(|err| Error::runtime(format!("json.encode: {err}")))
        })?,
    )?;
    json.set(
        "decode",
        lua.create_function(|lua, text: String| {
            let json: Json = serde_json::from_str(&text)
                .map_err(|err| Error::runtime(format!("json.decode: {err}")))?;
            from_json(lua, &json)
        })?,
    )?;
    om.set("json", json)?;

    let store = lua.create_table()?;
    let path = cx.state_dir.join(STORE_FILE);
    let file = path.clone();
    store.set(
        "get",
        lua.create_function(move |lua, (key, default): (String, Value)| {
            let data = read(&file)?;
            match data.get(&key) {
                Some(value) => from_json(lua, value),
                None => Ok(default),
            }
        })?,
    )?;
    let file = path.clone();
    store.set(
        "set",
        lua.create_function(move |_, (key, value): (String, Value)| {
            let mut data = read(&file)?;
            if value == Value::Nil {
                data.remove(&key);
            } else {
                data.insert(key, to_json(&value)?);
            }
            write(&file, &data)
        })?,
    )?;
    let file = path.clone();
    store.set(
        "all",
        lua.create_function(move |lua, ()| from_json(lua, &Json::Object(read(&file)?)))?,
    )?;
    store.set("path", path.to_string_lossy().to_string())?;
    om.set("store", store)
}

fn read(path: &Path) -> Result<Map<String, Json>> {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Json>(&text) {
            Ok(Json::Object(map)) => Ok(map),
            Ok(_) => Err(Error::runtime(format!(
                "{} does not hold a JSON object",
                path.display()
            ))),
            Err(err) => Err(Error::runtime(format!(
                "{} is not valid JSON: {err}",
                path.display()
            ))),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(err) => Err(Error::runtime(format!("reading {}: {err}", path.display()))),
    }
}

/// Writes the whole store atomically: a crash mid-write cannot leave half a file.
fn write(path: &Path, data: &Map<String, Json>) -> Result<()> {
    let text = serde_json::to_string_pretty(&Json::Object(data.clone()))
        .map_err(|err| Error::runtime(format!("store: {err}")))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)
        .map_err(|err| Error::runtime(format!("creating {}: {err}", dir.display())))?;
    let temp: PathBuf = path.with_extension("json.tmp");
    fs::write(&temp, text)
        .map_err(|err| Error::runtime(format!("writing {}: {err}", temp.display())))?;
    fs::rename(&temp, path)
        .map_err(|err| Error::runtime(format!("replacing {}: {err}", path.display())))
}
