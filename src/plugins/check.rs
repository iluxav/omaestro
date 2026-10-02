//! Whether a fetched plugin loads, before it goes into `lib/`: its
//! `init.lua` (and the modules of its own it requires) compiled and run in a
//! Lua state of its own, where `om` and every other global the plugin
//! reaches for are inert stubs. Nothing it does there touches the machine:
//! no `io`, no `os.execute`, no `load`, no binary chunks, and a budget of
//! instructions and memory stops a loop at its top level. What comes back
//! is whether it returns a module with a `setup` function, which the rule
//! `om plugin add` writes calls.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Result, bail};
use mlua::{HookTriggers, Lua, VmState};

/// Instructions between two looks at the budget, and how many looks.
const STEP: u32 = 10_000;
const LOOKS: u32 = 2_000;
const MEMORY: usize = 64 << 20;

/// What loading the plugin returned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// A table with a `setup` function: the rule `om` writes can call it.
    pub setup: bool,
}

const SANDBOX: &str = r#"
local name, read = ...

-- Stands in for om.* and any global the sandbox does not have: indexing,
-- calling, joining or comparing it gives another stub, never an error.
local stub_meta = {}
local function stub() return setmetatable({}, stub_meta) end
stub_meta.__index = function() return stub() end
stub_meta.__call = function() return stub() end
stub_meta.__concat = function() return stub() end
stub_meta.__tostring = function() return "(stub)" end
stub_meta.__len = function() return 0 end
stub_meta.__lt = function() return false end
stub_meta.__le = function() return false end
for _, op in ipairs({ "__unm", "__add", "__sub", "__mul", "__div", "__mod", "__idiv" }) do
  stub_meta[op] = function() return stub() end
end

local env = {
  string = string, table = table, math = math, utf8 = utf8,
  os = setmetatable(
    { getenv = os.getenv, date = os.date, time = os.time, clock = os.clock, difftime = os.difftime },
    { __index = function() return stub() end }
  ),
  pairs = pairs, ipairs = ipairs, next = next, select = select, type = type,
  tostring = tostring, tonumber = tonumber, rawget = rawget, rawset = rawset,
  rawequal = rawequal, rawlen = rawlen, setmetatable = setmetatable,
  getmetatable = getmetatable, pcall = pcall, xpcall = xpcall, error = error,
  assert = assert, print = function() end, coroutine = coroutine,
  _VERSION = _VERSION,
  om = stub(),
}
env._G = env
setmetatable(env, { __index = function() return stub() end })

-- The plugin's own modules (name.sub -> sub.lua or sub/init.lua) load from
-- its files; anything else (another plugin, a library) is a stub.
local loaded = {}
local function load_file(rel)
  local source = read(rel)
  if not source then return nil end
  local chunk, err = load(source, "@" .. name .. "/" .. rel, "t", env)
  if not chunk then error(err, 0) end
  return chunk
end
function env.require(mod)
  if loaded[mod] ~= nil then return loaded[mod] end
  local chunk
  if mod == name then
    chunk = load_file("init.lua")
  elseif mod:sub(1, #name + 1) == name .. "." then
    local sub = mod:sub(#name + 2):gsub("%.", "/")
    chunk = load_file(sub .. ".lua") or load_file(sub .. "/init.lua")
    if not chunk then error("module '" .. mod .. "' not found in " .. name, 2) end
  else
    return stub()
  end
  local value = chunk(mod)
  if value == nil then value = true end
  loaded[mod] = value
  return value
end

local module = env.require(name)
return type(module) == "table" and type(module.setup) == "function"
"#;

/// A path the plugin's `require` may read: inside its directory.
fn inside(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| dir.join(rel))
}

/// The first line of a Lua error, without mlua's prefix.
fn reason(err: &mlua::Error) -> String {
    let text = err.to_string();
    let line = text.lines().next().unwrap_or("").trim();
    line.strip_prefix("runtime error: ")
        .or_else(|| line.strip_prefix("syntax error: "))
        .unwrap_or(line)
        .to_string()
}

/// Loads the plugin in `dir` as `name` and says what it returned; an
/// error says why it does not load.
pub fn check(name: &str, dir: &Path) -> Result<Shape> {
    let run = || -> mlua::Result<bool> {
        let lua = Lua::new();
        lua.set_memory_limit(MEMORY)?;
        let looks = Arc::new(AtomicU32::new(0));
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(STEP),
            move |_, _| {
                if looks.fetch_add(1, Ordering::Relaxed) >= LOOKS {
                    return Err(mlua::Error::runtime(
                        "it does not finish loading (a loop at its top level?)",
                    ));
                }
                Ok(VmState::Continue)
            },
        )?;
        let dir = dir.to_path_buf();
        let read = lua.create_function(move |_, rel: String| {
            Ok(inside(&dir, &rel).and_then(|path| std::fs::read_to_string(path).ok()))
        })?;
        lua.load(SANDBOX)
            .set_name("=plugin-check")
            .call::<bool>((name, read))
    };
    match run() {
        Ok(setup) => Ok(Shape { setup }),
        Err(err) => bail!("{name} does not load: {}", reason(&err)),
    }
}

#[cfg(test)]
#[path = "check_tests.rs"]
mod tests;
