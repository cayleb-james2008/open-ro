import { MARKER, type ScriptClass } from './studioChecks';

/**
 * Luau that Open-Ro runs in Roblox Studio through the Studio MCP server's
 * Luau tool. The snippets are fixed code with the request embedded as Luau
 * literals; studioChecks.ts parses what comes back.
 */

/** Services read by a snapshot when the caller does not name any. */
export const DEFAULT_SNAPSHOT_SERVICES = [
  'Workspace', 'Players', 'Lighting', 'ReplicatedFirst', 'ReplicatedStorage', 'ServerScriptService',
  'ServerStorage', 'StarterGui', 'StarterPack', 'StarterPlayer', 'SoundService', 'Teams',
];

/** A Luau long string that cannot be closed early by its own contents. */
export function luauString(value: string): string {
  let level = 0;
  while (value.includes(`]${'='.repeat(level)}]`) || value.endsWith(`]${'='.repeat(level)}`)) {
    level += 1;
  }
  const eq = '='.repeat(level);
  // A newline right after the opening bracket is dropped by Luau, so add one when the value starts with a newline.
  return `[${eq}[${value.startsWith('\n') || value.startsWith('\r') ? '\n' : ''}${value}]${eq}]`;
}

function luauList(values: string[]): string {
  return `{${values.map(luauString).join(', ')}}`;
}

const PRELUDE = `local HttpService = game:GetService("HttpService")
local function finish(ok, value)
  local reply = HttpService:JSONEncode(ok and { ok = true, result = value } or { ok = false, error = tostring(value) })
  print("${MARKER}" .. reply)
  return "${MARKER}" .. reply
end
local function child(parent, name)
  local found = nil
  for _, item in ipairs(parent:GetChildren()) do
    if item.Name == name then
      if found ~= nil then error("Two instances named " .. name .. " under " .. parent:GetFullName(), 0) end
      found = item
    end
  end
  return found
end`;

/** Read-only walk of named services. A service Studio has not created is reported as missing. */
export function buildSnapshotLuau(input: { services: string[]; maxDepth: number; maxNodes: number }): string {
  return `${PRELUDE}
local services = ${luauList(input.services)}
local maxDepth, maxNodes = ${Math.trunc(input.maxDepth)}, ${Math.trunc(input.maxNodes)}
return finish(pcall(function()
  local out, count = { services = {}, truncated = false }, 0
  local function walk(inst, depth, into)
    for _, item in ipairs(inst:GetChildren()) do
      if count >= maxNodes then out.truncated = true; return end
      count += 1
      table.insert(into, { path = item:GetFullName(), className = item.ClassName, depth = depth })
      if depth < maxDepth then walk(item, depth + 1, into) end
    end
  end
  for _, name in ipairs(services) do
    local found, service = pcall(function() return game:FindService(name) end)
    local nodes = {}
    if found and service ~= nil then walk(service, 1, nodes) end
    table.insert(out.services, { name = name, present = found and service ~= nil, nodes = nodes })
  end
  return out
end))`;
}

/** Walk `segments` from game into `parent`. The two hooks say what a missing service or child does. */
function resolveLuau(missingService: string, missingChild: string): string {
  return `local parent = game:FindService(segments[1])
  if parent == nil then ${missingService} end
  for i = 2, #segments do
    local nextItem = child(parent, segments[i])
    if nextItem == nil then ${missingChild} end
    parent = nextItem
  end`;
}

/** Read-only read of one instance; returns Source when it is a script. */
export function buildReadLuau(segments: string[]): string {
  return `${PRELUDE}
local segments = ${luauList(segments)}
return finish(pcall(function()
  ${resolveLuau('return { exists = false, missing = segments[1] }', 'return { exists = false, missing = parent:GetFullName() .. "." .. segments[i] }')}
  local isScript = parent:IsA("LuaSourceContainer")
  return { exists = true, path = parent:GetFullName(), className = parent.ClassName, source = isScript and parent.Source or nil }
end))`;
}

/**
 * Write one script. It refuses when the script changed after the review
 * (expectedPrevious), records a ChangeHistoryService step so Ctrl+Z undoes
 * it in Studio, and reads the instance back after the write.
 */
export function buildWriteLuau(input: { segments: string[]; className: ScriptClass; source: string; expectedPrevious: string | null }): string {
  const parentSegments = input.segments.slice(0, -1);
  const name = input.segments[input.segments.length - 1];
  return `${PRELUDE}
local segments = ${luauList(parentSegments)}
local name, className, source = ${luauString(name)}, ${luauString(input.className)}, ${luauString(input.source)}
local expectedPrevious = ${input.expectedPrevious === null ? 'nil' : luauString(input.expectedPrevious)}
return finish(pcall(function()
  ${resolveLuau('error("Service " .. segments[1] .. " is missing", 0)', 'error("Missing parent " .. parent:GetFullName() .. "." .. segments[i], 0)')}
  local target = child(parent, name)
  if expectedPrevious == nil then
    if target ~= nil then error(target:GetFullName() .. " appeared after the review. Read it again.", 0) end
  elseif target == nil then
    error("The script was removed after the review.", 0)
  elseif target.ClassName ~= className then
    error(target:GetFullName() .. " is a " .. target.ClassName .. ", not a " .. className, 0)
  elseif target.Source ~= expectedPrevious then
    error(target:GetFullName() .. " changed after the review. Read it again.", 0)
  end
  local history = game:GetService("ChangeHistoryService")
  local recorded, recording = pcall(function() return history:TryBeginRecording("Open-Ro: " .. name) end)
  if target == nil then
    target = Instance.new(className)
    target.Name = name
    target.Source = source
    target.Parent = parent
  else
    target.Source = source
  end
  if recorded and recording then history:FinishRecording(recording, Enum.FinishRecordingOperation.Commit) end
  local check = child(parent, name)
  return { path = check and check:GetFullName() or nil, className = check and check.ClassName or nil, source = check and check.Source or nil, created = expectedPrevious == nil }
end))`;
}
