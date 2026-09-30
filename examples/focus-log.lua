-- Logs every focus change to the journal, with the time. Useful to find an
-- app's class for other rules, or to see where the afternoon went:
--
--   journalctl --user -u omaestro -f

om.on_focus({}, function(win)
  om.log("focus", win.class, win.title)
end)
