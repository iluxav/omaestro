-- system-events: what the machine is doing, as notifications and journal
-- lines: wake from sleep, USB devices plugged in, a low battery, network
-- changes. Each source runs only while something listens to it.
--
--   om.use("system-events").setup({})
--
-- Options (false switches one off):
--   wake        = true   notify on wake, with the time
--   usb         = true   notify when a device is plugged in
--   battery_low = 15     notify at this percent or below while discharging
--   network     = true   log network changes (nmcli's words)
--   sleep_log   = true   log going to sleep

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

function M.setup(opts)
  opts = opts or {}

  if option(opts.sleep_log, true) then
    om.on_sleep(function()
      om.log("going to sleep")
    end, { label = "Log going to sleep" })
  end

  if option(opts.wake, true) then
    om.on_wake(function()
      om.notify("Welcome back", os.date("%H:%M"))
    end, { label = "Notify on wake" })
  end

  if option(opts.usb, true) then
    -- {action = "add" | "remove", device = "/devices/.../usb1/1-3"}
    om.on_usb(function(dev)
      if dev.action == "add" then
        om.notify("USB", "plugged in: " .. dev.device:match("[^/]+$"))
      end
    end, { label = "Notify when a USB device is plugged in" })
  end

  local low = option(opts.battery_low, 15)
  if low then
    -- {percent = 42, status = "Discharging"}; fires when either changes.
    om.on_battery(function(b)
      if b.status == "Discharging" and b.percent <= low then
        om.notify("Battery", b.percent .. "%, find a charger")
      end
    end, { label = "Low-battery warning" })
  end

  if option(opts.network, true) then
    -- {line = "wlan0: connected"}
    om.on_network(function(n)
      om.log("network", n.line)
    end, { label = "Log network changes" })
  end
  return M
end

return M
