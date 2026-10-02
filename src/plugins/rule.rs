//! The rule file `om plugin add` writes into `rules.d/`: it loads the
//! plugin and passes it the options chosen for it, so a plugin runs at once
//! and its settings have a place that is not the plugin's code. `om plugin
//! configure` rewrites it; a file reshaped by hand is left to its owner.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value as Json};

use super::schema::{lua_key, lua_literal};
use super::variable;

pub(super) fn rule_path(config_dir: &Path, name: &str) -> PathBuf {
    config_dir.join("rules.d").join(format!("{name}.lua"))
}

/// The rule file for a plugin, with the options in `values` (in order).
pub(super) fn rule_text(name: &str, description: &str, values: &[(String, Json)]) -> String {
    let var = variable(name);
    let setup = if values.is_empty() {
        format!("{var}.setup({{}})\n")
    } else {
        let lines: Vec<String> = values
            .iter()
            .map(|(key, value)| format!("  {} = {},\n", lua_key(key), lua_literal(value)))
            .collect();
        format!("{var}.setup({{\n{}}})\n", lines.concat())
    };
    format!(
        "-- {name}: {description}\n\
         -- Options and what it does: ~/.config/omaestro/lib/{name}/README.md\n\
         local {var} = om.use(\"{name}\")\n\
         {setup}"
    )
}

/// Whether a rule file still has the shape `rule_text` writes: its header,
/// the `om.use` line, and a `setup` call with one `key = value,` per line.
/// Values may have been changed by hand; anything more is the owner's.
pub(super) fn is_generated_rule(name: &str, text: &str) -> bool {
    let var = variable(name);
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 4
        || !lines[0].starts_with(&format!("-- {name}: "))
        || lines[1]
            != format!("-- Options and what it does: ~/.config/omaestro/lib/{name}/README.md")
        || lines[2] != format!("local {var} = om.use(\"{name}\")")
    {
        return false;
    }
    if lines.len() == 4 {
        return lines[3] == format!("{var}.setup({{}})");
    }
    lines[3] == format!("{var}.setup({{")
        && lines[lines.len() - 1] == "})"
        && lines[4..lines.len() - 1].iter().all(|line| {
            line.starts_with("  ")
                && line.ends_with(',')
                && line.contains(" = ")
                && !line.trim_start().starts_with("--")
        })
}

/// The description in a generated rule's first line.
pub(super) fn description_of(name: &str, text: &str) -> Option<String> {
    text.lines()
        .next()?
        .strip_prefix(&format!("-- {name}: "))
        .map(str::to_string)
}

/// Writes rules.d/<name>.lua. An existing file is replaced only when
/// `replace` is set and it is still in the generated shape (or `force`).
pub(super) fn write_rule(
    config_dir: &Path,
    name: &str,
    description: &str,
    values: &[(String, Json)],
    replace: bool,
    force: bool,
) -> Result<()> {
    let path = rule_path(config_dir, name);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if !replace {
            println!("{} exists already; left alone", path.display());
            return Ok(());
        }
        if !force && !is_generated_rule(name, &text) {
            bail!(
                "{} has more in it than om writes; change it there, or add --force to replace it",
                path.display()
            );
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, rule_text(name, description, values))
        .with_context(|| format!("writing {}", path.display()))?;
    if values.is_empty() {
        println!("wrote {}: {name} runs with its defaults", path.display());
    } else {
        println!("wrote {}: {name} runs with your options", path.display());
    }
    Ok(())
}

/// The options a rule file passes to the plugin's `setup`, read by running
/// the file in an empty Lua state where `om.use` returns a recorder. Only
/// plain values come back; a file that does more than that, or passes code
/// (a function), says so.
pub(super) fn read_values(text: &str) -> Result<Map<String, Json>, String> {
    let lua = mlua::Lua::new();
    let run = || -> mlua::Result<Option<mlua::Table>> {
        let recorder = lua
            .load(
                r#"
                local captured
                local om = {}
                function om.use()
                  local module = {}
                  return setmetatable(module, { __index = function(_, _key)
                    return function(...)
                      local args = { ... }
                      for i = #args, 1, -1 do
                        if type(args[i]) == "table" and args[i] ~= module then
                          captured = args[i]
                          break
                        end
                      end
                      return module
                    end
                  end })
                end
                local env = {
                  om = om,
                  os = { getenv = os.getenv, date = os.date, time = os.time },
                  string = string, table = table, math = math,
                  pairs = pairs, ipairs = ipairs, tostring = tostring, tonumber = tonumber,
                  setmetatable = setmetatable, type = type, select = select,
                }
                return env, function() return captured end
                "#,
            )
            .set_name("=rule-reader")
            .eval::<(mlua::Table, mlua::Function)>()?;
        let (env, captured) = recorder;
        lua.load(text)
            .set_name("=rule")
            .set_environment(env)
            .exec()?;
        captured.call::<Option<mlua::Table>>(())
    };
    let table = run().map_err(|err| format!("could not read it: {err}"))?;
    let Some(table) = table else {
        return Ok(Map::new());
    };
    match crate::luajson::to_json(&mlua::Value::Table(table)) {
        Ok(Json::Object(map)) => Ok(map),
        Ok(_) => Ok(Map::new()),
        Err(_) => Err("it passes code (a function), which only an editor can change".to_string()),
    }
}

/// After a plugin is gone: its generated rule goes too; a rule the user
/// changed stays, with a warning.
pub(super) fn drop_rule(config_dir: &Path, name: &str) {
    let path = rule_path(config_dir, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if is_generated_rule(name, &text) {
        match std::fs::remove_file(&path) {
            Ok(()) => println!("removed {}, which only loaded it", path.display()),
            Err(err) => eprintln!("om: could not remove {}: {err}", path.display()),
        }
    } else if text.contains(&format!("om.use(\"{name}\")"))
        || text.contains(&format!("om.use('{name}')"))
    {
        println!(
            "{} still loads it: the rules will not load until you change that",
            path.display()
        );
    }
}
