-- panel: SUPER+ALT+O opens and closes the rules panel that comes with the
-- Omarchy plugin: every rule with a switch, the override switch, a reload
-- button. The same command works from a bind in ~/.config/hypr/bindings.lua,
-- which also opens it when the daemon is down.
--
--   om.use("panel").setup({})
--
-- Options:
--   chord = "SUPER + ALT + O"

local M = {}

function M.setup(opts)
  opts = opts or {}
  om.hotkey(opts.chord or "SUPER + ALT + O", function()
    om.spawn("omarchy-shell shell toggle io.github.iluxav.omaestro")
  end, { label = "omaestro menu" })
  return M
end

return M
