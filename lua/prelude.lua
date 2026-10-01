-- Loaded into every Lua state before the user's files. Helpers only: nothing
-- here touches the system. Strings get these as methods: ("  x "):trim().

function string.trim(s)
  return (s:match("^%s*(.-)%s*$"))
end

function string.starts_with(s, prefix)
  return s:sub(1, #prefix) == prefix
end

function string.ends_with(s, suffix)
  return suffix == "" or s:sub(-#suffix) == suffix
end

-- Splits on a plain separator (not a pattern). Without one, splits on runs
-- of whitespace and drops empty pieces.
function string.split(s, sep)
  local parts = {}
  if sep == nil or sep == "" then
    for piece in s:gmatch("%S+") do
      parts[#parts + 1] = piece
    end
    return parts
  end
  local from = 1
  while true do
    local first, last = s:find(sep, from, true)
    if not first then
      parts[#parts + 1] = s:sub(from)
      return parts
    end
    parts[#parts + 1] = s:sub(from, first - 1)
    from = last + 1
  end
end

-- om.layout(entries): arrange windows. Each entry: class and/or title (Lua
-- patterns), optional workspace (name or number), optional place (a name
-- or fractions, as window:place takes), optional all = true to place every
-- match instead of the first. Returns how many windows were placed.
function om.layout(entries)
  local placed = 0
  local windows = om.windows()
  for _, entry in ipairs(entries) do
    for _, win in ipairs(windows) do
      local class_ok = entry.class == nil or win.class:find(entry.class) ~= nil
      local title_ok = entry.title == nil or win.title:find(entry.title) ~= nil
      if class_ok and title_ok then
        if entry.workspace ~= nil then
          win:to_workspace(entry.workspace)
        end
        if entry.place ~= nil then
          win:place(entry.place)
        end
        placed = placed + 1
        if not entry.all then
          break
        end
      end
    end
  end
  return placed
end

-- om.use(name, url): require a module from ~/.config/omaestro/lib. When it
-- is not there and a git URL is given, clone it there first (into
-- lib/<name>), then require it. Returns what the module returns.
function om.use(name, url)
  local ok, result = pcall(require, name)
  if ok then
    return result
  end
  if not url then
    error(result, 2)
  end
  -- "user/repo" is short for GitHub, as in `om plugin add`.
  if not url:find("://", 1, true) and url:match("^[%w._-]+/[%w._-]+$") then
    url = "https://github.com/" .. url
  end
  local target = om.config_dir .. "/lib/" .. name
  om.shell("mkdir -p " .. om.config_dir .. "/lib && git clone --depth 1 '" .. url:gsub("'", "") .. "' '" .. target .. "'")
  package.loaded[name] = nil
  return require(name)
end
