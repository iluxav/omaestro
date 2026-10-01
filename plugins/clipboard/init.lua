-- clipboard: the last things you copied, and a notes file. SUPER+ALT+V
-- picks one of the last ten clips to paste; SUPER+ALT+N appends the
-- clipboard to ~/notes/clips.md with a timestamp. Only text is kept (an
-- image reads as ""). The history lives in om.store, so it survives restarts.
--
--   om.use("clipboard").setup({})
--
-- Options (false switches a chord off):
--   history = "SUPER + ALT + V"
--   keep    = 10
--   notes   = "SUPER + ALT + N"
--   file    = os.getenv("HOME") .. "/notes/clips.md"

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  local history = option(opts.history, "SUPER + ALT + V")
  local keep = opts.keep or 10
  if history then
    om.on_clipboard(function(text)
      if text == "" then
        return
      end
      local clips = om.store.get("clips", {})
      if clips[1] == text then
        return
      end
      table.insert(clips, 1, text)
      while #clips > keep do
        table.remove(clips)
      end
      om.store.set("clips", clips)
    end)
    om.hotkey(history, function()
      local clips = om.store.get("clips", {})
      if #clips == 0 then
        om.notify("Clips", "Nothing copied yet")
        return
      end
      local labels = {}
      for i, clip in ipairs(clips) do
        labels[i] = clip:gsub("%s+", " "):sub(1, 60)
      end
      local pick = om.choose("Paste", labels)
      for i, label in ipairs(labels) do
        if label == pick then
          om.paste(clips[i])
          return
        end
      end
    end)
  end

  local notes = option(opts.notes, "SUPER + ALT + N")
  local file = opts.file or (os.getenv("HOME") .. "/notes/clips.md")
  if notes then
    om.hotkey(notes, function()
      local text = om.clipboard():trim()
      if text == "" then
        om.notify("Clip", "Nothing (or no text) in the clipboard")
        return
      end
      local dir = file:match("^(.*)/[^/]*$")
      if dir then
        om.shell("mkdir -p '" .. dir:gsub("'", "'\\''") .. "'")
      end
      local out, err = io.open(file, "a")
      if not out then
        error("cannot open " .. file .. ": " .. tostring(err))
      end
      out:write("\n## ", os.date("%Y-%m-%d %H:%M"), "\n\n", text, "\n")
      out:close()
      om.notify("Clip", "Saved " .. #text .. " characters to " .. file:match("[^/]+$"))
    end)
  end
  return M
end

return M
