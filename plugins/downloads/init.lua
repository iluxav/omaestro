-- downloads: a notification when something lands in ~/Downloads. om.on_file
-- watches the directory, recursively; editor swap files and backups are
-- skipped by the daemon.
--
--   om.use("downloads").setup({})
--
-- Options:
--   dir = os.getenv("HOME") .. "/Downloads"

local M = {}

function M.setup(opts)
  opts = opts or {}
  local dir = opts.dir or (os.getenv("HOME") .. "/Downloads")
  local exists = io.open(dir)
  if not exists then
    om.log("downloads: " .. dir .. " does not exist, nothing to watch")
    return M
  end
  exists:close()
  om.on_file(dir, function(change)
    if change.kind == "create" then
      om.notify("Downloaded", change.path:match("[^/]+$"))
    end
  end)
  return M
end

return M
