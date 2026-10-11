-- clock: the time, the date and the weather in large type in the middle of
-- the screen for a moment, when SUPER is tapped on its own. The weather
-- comes from wttr.in and is fetched in the background, so the card shows at
-- once.
--
--   om.use("clock").setup({})
--
-- Options (false switches one off):
--   tap      = "SUPER"             the key to tap: SUPER, SUPER_R, ALT, CTRL, SHIFT; "off": none
--   within   = 0.3                 seconds the key may be down for a tap
--   chord    = "SUPER + ALT + A"   shows the card too
--   seconds  = 1                   how long it stays up
--   time     = "%H:%M"             os.date formats; "%I:%M %p" for 12-hour
--   date     = "%A, %B %e"
--   weather  = true                false: the time and the date only
--   location = nil                 "Lisbon", "48.85,2.35"; nil: wttr.in goes by your IP
--   units    = "auto"              "metric", "imperial", or wttr.in's pick for the place
--   refresh  = "15m"               how often the weather is fetched
--   position = "center"            "top" or "bottom"

local M = {}

local function option(value, default)
  if value == nil then
    return default
  end
  return value
end

-- wttr.in's one-line answer, "Sunny, +22°C". The place goes in the path:
-- spaces as "+", anything else outside plain ASCII percent-encoded.
local function weather_url(location, units)
  local place = ""
  if location then
    place = location:trim():gsub("%s+", "+"):gsub("[^%w%-%.,+]", function(c)
      return string.format("%%%02X", c:byte())
    end)
  end
  local url = "https://wttr.in/" .. place .. "?format=%C,+%t"
  if units == "metric" then
    url = url .. "&m"
  elseif units == "imperial" then
    url = url .. "&u"
  end
  return url
end

function M.setup(opts)
  opts = opts or {}
  -- An om from before om.show and om.tap: say so on load, not on the
  -- first press.
  if not (om.show and om.tap) then
    error("clock needs a newer omaestro, one with om.show and om.tap: update om", 2)
  end
  local tap = option(opts.tap, "SUPER")
  local within = option(opts.within, 0.3)
  local chord = option(opts.chord, "SUPER + ALT + A")
  local seconds = option(opts.seconds, 1)
  local time_format = option(opts.time, "%H:%M")
  local date_format = option(opts.date, "%A, %B %e")
  local show_weather = option(opts.weather, true)
  local url = weather_url(opts.location, option(opts.units, "auto"))
  local refresh_every = option(opts.refresh, "15m")
  local position = option(opts.position, "center")

  -- A bad format fails here, on load, not on the first press.
  os.date(time_format)
  if date_format then
    os.date(date_format)
  end

  local weather -- the last good answer; nil until one came

  local function refresh()
    local ok, response = pcall(om.http, url, { timeout = 10 })
    if not ok then
      om.log("clock", "weather: " .. tostring(response))
      return false
    end
    local text = response.body:trim()
    -- One short line, or it is an error page.
    if not response.ok or text == "" or text:find("\n") or #text > 80 then
      om.log("clock", "weather: wttr.in answered " .. response.status .. ": " .. text:sub(1, 80))
      return false
    end
    weather = text
    return true
  end

  local function card()
    local lines = {}
    if date_format then
      -- %e pads the day with a space: "October  5".
      lines[#lines + 1] = os.date(date_format):gsub("%s%s+", " ")
    end
    if show_weather and weather then
      lines[#lines + 1] = weather
    end
    return { big = os.date(time_format), lines = lines }
  end

  if show_weather and refresh_every then
    om.every(refresh_every, refresh, { label = "Clock: fetch the weather every " .. refresh_every })
  end

  local function show()
    local shown = om.show(card(), { seconds = seconds, position = position })
    -- No weather yet (just loaded, or the last fetch failed): fetch it now
    -- and add it if the card is still up.
    if show_weather and not weather and refresh() then
      shown:update(card())
    end
  end

  if tap and tap ~= "off" then
    om.tap(tap, show, { within = within, label = "Show the time, date and weather (tap " .. tap .. ")" })
  end
  if chord then
    om.hotkey(chord, show, { label = "Show the time, date and weather" })
  end
  return M
end

return M
