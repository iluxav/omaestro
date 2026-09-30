-- A reminder every 45 minutes. Intervals are "30s", "5m", "1h" or sums
-- like "1h30m". The first one fires after a full interval, not at load.

om.every("45m", function()
  om.notify("Stretch", "45 minutes at the desk. Stand up for a minute.")
end)
