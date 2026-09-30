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
