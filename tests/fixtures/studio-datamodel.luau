-- Minimal DataModel mock for running Open-Ro's generated Luau with the Luau CLI (tests/luau).
local Instance_ = {}
local methods = {}
local function new(className, name)
  local self = { _props = { ClassName = className, Name = name or className, Source = (className:find("Script") and "" or nil) }, _children = {}, _parent = nil }
  return setmetatable(self, Instance_)
end
Instance_.__index = function(self, key)
  if methods[key] then return methods[key] end
  if key == "Parent" then return rawget(self, "_parent") end
  return rawget(self, "_props")[key]
end
Instance_.__newindex = function(self, key, value)
  if key == "Parent" then
    local old = rawget(self, "_parent")
    if old then for i, c in ipairs(old._children) do if c == self then table.remove(old._children, i) break end end end
    rawset(self, "_parent", value)
    if value then table.insert(value._children, self) end
  elseif key == "Source" and not rawget(self, "_props").ClassName:find("Script") then
    error("Source is not a valid member of " .. rawget(self, "_props").ClassName)
  else
    rawget(self, "_props")[key] = value
  end
end
function methods.GetChildren(self) local t = {} for i, c in ipairs(rawget(self, "_children")) do t[i] = c end return t end
function methods.FindFirstChild(self, n) for _, c in ipairs(rawget(self, "_children")) do if c.Name == n then return c end end return nil end
function methods.GetFullName(self)
  local parts, node = {}, self
  while node and node.ClassName ~= "DataModel" do table.insert(parts, 1, node.Name) node = node.Parent end
  return table.concat(parts, ".")
end
function methods.IsA(self, cls)
  if cls == "LuaSourceContainer" then return self.ClassName:find("Script") ~= nil end
  return self.ClassName == cls
end
local game = new("DataModel", "Game")
local function encode(v)
  local t = typeof(v)
  if t == "nil" then return "null" end
  if t == "boolean" or t == "number" then return tostring(v) end
  if t == "string" then
    return '"' .. v:gsub('[%c"\\]', function(c)
      local map = { ['"'] = '\\"', ['\\'] = '\\\\', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t' }
      return map[c] or string.format("\\u%04x", c:byte())
    end) .. '"'
  end
  if #v > 0 or next(v) == nil then local p = {} for i, x in ipairs(v) do p[i] = encode(x) end return "[" .. table.concat(p, ",") .. "]" end
  local p = {} for k, x in pairs(v) do table.insert(p, encode(tostring(k)) .. ":" .. encode(x)) end return "{" .. table.concat(p, ",") .. "}"
end
local RECORDED, COMMITTED = nil, nil
-- What TryBeginRecording returns; a test sets it to nil to act like Studio refusing to record.
local RECORDING_ID = "rec1"
local services = {
  HttpService = { JSONEncode = function(_, v) return encode(v) end },
  ChangeHistoryService = { TryBeginRecording = function(_, n) RECORDED = n return RECORDING_ID end, FinishRecording = function(_, id, op) COMMITTED = id end },
}
local created = {}
function methods.FindService(self, n) return created[n] end
function methods.GetService(self, n)
  if services[n] then return services[n] end
  if not created[n] then local s = new(n, n) s.Parent = game created[n] = s end
  return created[n]
end
local Enum = { FinishRecordingOperation = { Commit = "Commit" } }
local Instance = { new = function(c) return new(c) end }
-- fixture: ServerScriptService.Shop (Script), two ServerScriptService.Twin scripts, ReplicatedStorage.Shared.Util (ModuleScript), StarterGui.HUD (ScreenGui); no Teams service
local sss = methods.GetService(game, "ServerScriptService")
local shop = new("Script", "Shop") shop.Source = "print('old')\n-- ]] tricky ]=] end" shop.Parent = sss
local rs = methods.GetService(game, "ReplicatedStorage")
local shared = new("Folder", "Shared") shared.Parent = rs
local util = new("ModuleScript", "Util") util.Source = "return {}" util.Parent = shared
local sg = methods.GetService(game, "StarterGui")
local hud = new("ScreenGui", "HUD") hud.Parent = sg
local dup1 = new("Folder", "Dup") dup1.Parent = rs
local dup2 = new("Folder", "Dup") dup2.Parent = rs
local twin1 = new("Script", "Twin") twin1.Parent = sss
local twin2 = new("Script", "Twin") twin2.Parent = sss
methods.GetService(game, "Workspace")
