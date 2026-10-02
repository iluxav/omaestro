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

-- om.menu(chord, items, opts): a chord that opens a menu of named actions.
-- Items are { "label", function(ctx) ... end } (or { label =, fn = }), or a
-- function(ctx) returning them, called on each press (return nil to show
-- nothing). An item with remember = false is never moved to the top.
--
-- ctx, taken when the chord is pressed, before the menu takes the focus:
--   ctx.window      the window that had focus (nil on an empty workspace)
--   ctx.selection   the selected text, with opts.selection = true
--
-- opts:
--   title      the menu's label (default "Menu")
--   remember   the last item picked comes first, so the chord and Enter
--              repeat it (default true; kept in om.store per chord)
--   refocus    give ctx.window the focus back before the item runs
--              (default true)
--   selection  read the selection into ctx.selection (default false)
--
-- Returns the handle of the hotkey, with :remove().
local function menu_items(list, level)
  if type(list) ~= "table" then
    error("om.menu: items must be a list of { label, function }", level)
  end
  local items = {}
  for i, item in ipairs(list) do
    local label = type(item) == "table" and (item[1] or item.label)
    local fn = type(item) == "table" and (item[2] or item.fn)
    if type(label) ~= "string" or type(fn) ~= "function" then
      error("om.menu: item " .. i .. " needs a label and a function", level)
    end
    items[#items + 1] = { label = label, fn = fn, remember = item.remember ~= false }
  end
  return items
end

function om.menu(chord, items, opts)
  opts = opts or {}
  if type(chord) ~= "string" then
    error("om.menu: the first argument is a chord, like \"SUPER + ALT + P\"", 2)
  end
  local static = nil
  if type(items) ~= "function" then
    static = menu_items(items, 3)
  end
  local title = opts.title or "Menu"
  local remember = opts.remember ~= false
  local refocus = opts.refocus ~= false
  local key = "om.menu " .. chord

  return om.hotkey(chord, function()
    local ctx = { window = om.window() }
    if opts.selection then
      ctx.selection = om.selection()
    end
    local list = static
    if not list then
      local made = items(ctx)
      if made == nil then
        return
      end
      -- Level 0: no position inside the prelude; the notification names
      -- the rule that registered the menu instead.
      list = menu_items(made, 0)
    end
    if #list == 0 then
      return
    end

    local last = remember and om.store.get(key) or nil
    local labels = {}
    for _, item in ipairs(list) do
      if item.label == last and item.remember then
        table.insert(labels, 1, item.label)
      else
        labels[#labels + 1] = item.label
      end
    end

    local pick = om.choose(title, labels)
    if not pick then
      return
    end
    for _, item in ipairs(list) do
      if item.label == pick then
        if remember and item.remember then
          om.store.set(key, pick)
        end
        if refocus and ctx.window then
          ctx.window:focus()
        end
        return item.fn(ctx)
      end
    end
  end)
end

-- om.panel(): opens or closes the rules panel of the Omarchy plugin, the
-- way `om panel` does. One line in init.lua gives it a chord:
--   om.hotkey("SUPER + ALT + O", om.panel)
function om.panel()
  om.spawn("omarchy-shell shell toggle io.github.iluxav.omaestro")
end
