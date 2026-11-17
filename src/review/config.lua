-- Wheelhouse reads rex-reviewer's config.lua as it is, with LuaJIT.
--   luajit -e <this> dump PATH            the config as JSON; functions as {"$fn": "path.to.it"}
--   luajit -e <this> call PATH KEY PR     KEY's function (e.g. effort.review) called with PR,
--                                         a Lua table literal; prints its result as JSON
local mode, path = arg[1], arg[2]

local function esc(s)
  return '"' .. s:gsub('[%c"\\]', function(c)
    local m = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t' }
    return m[c] or string.format('\\u%04x', c:byte())
  end) .. '"'
end

local function is_array(t)
  local n = 0
  for k in pairs(t) do
    if type(k) ~= 'number' or k < 1 or k % 1 ~= 0 then return false end
    n = n + 1
  end
  return n == #t
end

local function json(v, where, seen)
  local t = type(v)
  if t == 'nil' then return 'null' end
  if t == 'boolean' then return tostring(v) end
  if t == 'number' then
    if v ~= v or v == math.huge or v == -math.huge then return 'null' end
    if v % 1 == 0 and math.abs(v) < 2^53 then return string.format('%d', v) end
    return string.format('%.17g', v)
  end
  if t == 'string' then return esc(v) end
  if t == 'function' then return '{"$fn":' .. esc(where) .. '}' end
  if t == 'table' then
    seen = seen or {}
    if seen[v] then return 'null' end
    seen[v] = true
    local out = {}
    if next(v) == nil then seen[v] = nil return '{}' end
    if is_array(v) then
      for i, x in ipairs(v) do out[#out + 1] = json(x, where .. '[' .. i .. ']', seen) end
      seen[v] = nil
      return '[' .. table.concat(out, ',') .. ']'
    end
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = tostring(k) end
    table.sort(keys)
    for _, k in ipairs(keys) do
      local x = v[k]
      if x == nil then x = v[tonumber(k)] end
      out[#out + 1] = esc(k) .. ':' .. json(x, where == '' and k or (where .. '.' .. k), seen)
    end
    seen[v] = nil
    return '{' .. table.concat(out, ',') .. '}'
  end
  return 'null'
end

local ok, cfg = pcall(dofile, path)
if not ok then io.stderr:write(tostring(cfg), '\n') os.exit(2) end
if type(cfg) ~= 'table' then io.stderr:write(path, ' returns ', type(cfg), ', not a table\n') os.exit(2) end

if mode == 'dump' then
  print(json(cfg, ''))
elseif mode == 'call' then
  local f = cfg
  for part in arg[3]:gmatch('[^.]+') do
    f = type(f) == 'table' and (f[part] or f[tonumber(part)]) or nil
  end
  local pr = assert(loadstring('return ' .. arg[4]))()
  -- A PR's paths, matched as plain substrings, as rex-reviewer does.
  pr.touches = function(list)
    if type(list) == 'string' then list = { list } end
    for _, f in ipairs(pr.files or {}) do
      for _, p in ipairs(list) do
        if f:find(p, 1, true) then return true end
      end
    end
    return false
  end
  if type(f) == 'function' then
    local ok2, r = pcall(f, pr)
    if not ok2 then io.stderr:write(tostring(r), '\n') os.exit(3) end
    print(json(r, ''))
  else
    print(json(f, ''))
  end
else
  io.stderr:write('usage: dump PATH | call PATH KEY PR\n') os.exit(1)
end
