-- A named entry point. Fire it from a shell, a script, or anything that can
-- run a command:
--
--   om trigger hello
--
-- Copy this file to ~/.config/omaestro/rules.d/ and the daemon picks it up.

om.trigger("hello", function()
  om.notify("omaestro", "hello from a rule")
end)
