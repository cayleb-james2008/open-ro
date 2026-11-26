-- OpenRO Headless Roblox Luau Runtime Sandbox Shim (Lune 0.11+)
-- Executes Rojo / LocalPlace scripts deterministically without Roblox Studio.

local fs = require("@lune/fs")
local serde = require("@lune/serde")
local process = require("@lune/process")
local task = require("@lune/task")
local luau = require("@lune/luau")
local datetime = require("@lune/datetime")

local inputPath = process.args[1] or "input.json"
local outputPath = process.args[2] or "report.json"

local rawInput = fs.readFile(inputPath)
local inputData = serde.decode("json", rawInput)

local consoleLines = {}
local consoleErrors = {}
local consoleWarnings = {}

local function formatArgs(...)
	local n = select("#", ...)
	if n == 0 then
		return ""
	end
	local parts = table.create(n)
	for i = 1, n do
		parts[i] = tostring(select(i, ...))
	end
	return table.concat(parts, "\t")
end

local function pushLine(line)
	if #consoleLines < 2000 then
		table.insert(consoleLines, line)
	end
end

local function pushWarning(msg)
	local s = tostring(msg)
	if #consoleWarnings < 500 then
		table.insert(consoleWarnings, s)
	end
	pushLine("[WARN] " .. s)
end

local function pushError(contextLabel, errVal)
	local s = tostring(errVal)
	local full = if contextLabel and contextLabel ~= "" and not string.find(s, contextLabel, 1, true)
		then string.format("[%s] %s", contextLabel, s)
		else s
	if #consoleErrors < 500 then
		table.insert(consoleErrors, full)
	end
	pushLine("[ERROR] " .. full)
end

local function safeCall(contextLabel, fn, ...)
	local args = table.pack(...)
	local results = table.pack(xpcall(function()
		return fn(table.unpack(args, 1, args.n))
	end, function(err)
		local tb = debug.traceback(nil, 2)
		if tb and tb ~= "" then
			return tostring(err) .. "\n" .. tb
		end
		return tostring(err)
	end))
	if not results[1] then
		pushError(contextLabel, results[2])
		return false, results[2]
	end
	return true, table.unpack(results, 2, results.n)
end

-- Weak type tag registry & coroutine player context
local TYPE_TAGS = setmetatable({}, { __mode = "k" })
local threadPlayerContext = setmetatable({}, { __mode = "k" })

local function tagType(obj, typeName)
	TYPE_TAGS[obj] = typeName
	return obj
end

local function robloxTypeof(val)
	local tag = TYPE_TAGS[val]
	if tag ~= nil then
		return tag
	end
	return type(val)
end

-- Wrapped task scheduler so async task errors are always captured
local wrappedTask = {}
function wrappedTask.wait(seconds)
	return task.wait(seconds)
end
function wrappedTask.spawn(fnOrThread, ...)
	if type(fnOrThread) == "thread" then
		return task.spawn(fnOrThread, ...)
	end
	local args = table.pack(...)
	return task.spawn(function()
		safeCall("task.spawn", fnOrThread, table.unpack(args, 1, args.n))
	end)
end
function wrappedTask.defer(fnOrThread, ...)
	if type(fnOrThread) == "thread" then
		return task.defer(fnOrThread, ...)
	end
	local args = table.pack(...)
	return task.defer(function()
		safeCall("task.defer", fnOrThread, table.unpack(args, 1, args.n))
	end)
end
function wrappedTask.delay(seconds, fnOrThread, ...)
	if type(fnOrThread) == "thread" then
		return task.delay(seconds, fnOrThread, ...)
	end
	local args = table.pack(...)
	return task.delay(seconds, function()
		safeCall("task.delay", fnOrThread, table.unpack(args, 1, args.n))
	end)
end
function wrappedTask.cancel(th)
	if task.cancel then
		return task.cancel(th)
	end
end
function wrappedTask.synchronize() end
function wrappedTask.desynchronize() end

-- ---------------------------------------------------------------------------
-- RBXScriptSignal & RBXScriptConnection
-- ---------------------------------------------------------------------------
local Signal = {}
Signal.__index = Signal

local function newSignal(name)
	local self = setmetatable({
		_name = name or "Signal",
		_conns = {},
		_waiters = {},
	}, Signal)
	return tagType(self, "RBXScriptSignal")
end

function Signal:Connect(fn, metaTag)
	assert(type(fn) == "function", "Signal:Connect expects a function")
	local co = coroutine.running()
	local ownerPlayer = if co then threadPlayerContext[co] else nil
	local effectiveMeta = metaTag or (if ownerPlayer then { player = ownerPlayer } else nil)
	local conn
	conn = tagType({
		Connected = true,
		_fn = fn,
		_meta = effectiveMeta,
		Disconnect = function(c)
			if not c.Connected then
				return
			end
			c.Connected = false
			for i = #self._conns, 1, -1 do
				if self._conns[i] == c then
					table.remove(self._conns, i)
					break
				end
			end
		end,
	}, "RBXScriptConnection")
	table.insert(self._conns, conn)
	return conn
end

function Signal:Once(fn, metaTag)
	local conn
	conn = self:Connect(function(...)
		if conn then
			conn:Disconnect()
		end
		fn(...)
	end, metaTag)
	return conn
end

function Signal:Wait()
	local co = coroutine.running()
	table.insert(self._waiters, co)
	return coroutine.yield()
end

function Signal:Fire(...)
	local snapshot = table.clone(self._conns)
	local args = table.pack(...)
	for _, conn in ipairs(snapshot) do
		if conn.Connected then
			task.spawn(function()
				if conn._meta and conn._meta.player then
					threadPlayerContext[coroutine.running()] = conn._meta.player
				end
				safeCall(self._name, conn._fn, table.unpack(args, 1, args.n))
			end)
		end
	end
	if #self._waiters > 0 then
		local waiters = self._waiters
		self._waiters = {}
		for _, co in ipairs(waiters) do
			task.spawn(co, table.unpack(args, 1, args.n))
		end
	end
end

function Signal:FireFiltered(predicate, ...)
	local snapshot = table.clone(self._conns)
	local args = table.pack(...)
	for _, conn in ipairs(snapshot) do
		if conn.Connected and (not predicate or predicate(conn._meta)) then
			task.spawn(function()
				if conn._meta and conn._meta.player then
					threadPlayerContext[coroutine.running()] = conn._meta.player
				end
				safeCall(self._name, conn._fn, table.unpack(args, 1, args.n))
			end)
		end
	end
	if #self._waiters > 0 then
		local waiters = self._waiters
		self._waiters = {}
		for _, co in ipairs(waiters) do
			task.spawn(co, table.unpack(args, 1, args.n))
		end
	end
end

-- ---------------------------------------------------------------------------
-- Roblox Value / Geometry / Enum Types
-- ---------------------------------------------------------------------------
local Vector3 = {}
local Vector3Meta = {}
Vector3Meta.__index = function(v, k)
	if k == "Magnitude" then
		return math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
	elseif k == "Unit" then
		local m = math.sqrt(v.X * v.X + v.Y * v.Y + v.Z * v.Z)
		if m > 0 then
			return Vector3.new(v.X / m, v.Y / m, v.Z / m)
		end
		return Vector3.new(0, 0, 0)
	elseif k == "x" then
		return v.X
	elseif k == "y" then
		return v.Y
	elseif k == "z" then
		return v.Z
	end
	return Vector3[k]
end
function Vector3Meta.__add(a, b)
	return Vector3.new(a.X + b.X, a.Y + b.Y, a.Z + b.Z)
end
function Vector3Meta.__sub(a, b)
	return Vector3.new(a.X - b.X, a.Y - b.Y, a.Z - b.Z)
end
function Vector3Meta.__unm(a)
	return Vector3.new(-a.X, -a.Y, -a.Z)
end
function Vector3Meta.__mul(a, b)
	if type(a) == "number" then
		return Vector3.new(a * b.X, a * b.Y, a * b.Z)
	elseif type(b) == "number" then
		return Vector3.new(a.X * b, a.Y * b, a.Z * b)
	end
	return Vector3.new(a.X * b.X, a.Y * b.Y, a.Z * b.Z)
end
function Vector3Meta.__div(a, b)
	if type(b) == "number" then
		return Vector3.new(a.X / b, a.Y / b, a.Z / b)
	end
	return Vector3.new(a.X / b.X, a.Y / b.Y, a.Z / b.Z)
end
function Vector3Meta.__eq(a, b)
	return a.X == b.X and a.Y == b.Y and a.Z == b.Z
end
function Vector3Meta.__tostring(v)
	return string.format("%g, %g, %g", v.X, v.Y, v.Z)
end
function Vector3.new(x, y, z)
	return tagType(setmetatable({ X = x or 0, Y = y or 0, Z = z or 0 }, Vector3Meta), "Vector3")
end
function Vector3:Dot(b)
	return self.X * b.X + self.Y * b.Y + self.Z * b.Z
end
function Vector3:Cross(b)
	return Vector3.new(
		self.Y * b.Z - self.Z * b.Y,
		self.Z * b.X - self.X * b.Z,
		self.X * b.Y - self.Y * b.X
	)
end
function Vector3:Lerp(b, alpha)
	return self + (b - self) * alpha
end
function Vector3:FuzzyEq(b, eps)
	eps = eps or 1e-5
	return math.abs(self.X - b.X) <= eps and math.abs(self.Y - b.Y) <= eps and math.abs(self.Z - b.Z) <= eps
end
Vector3.zero = Vector3.new(0, 0, 0)
Vector3.one = Vector3.new(1, 1, 1)
Vector3.xAxis = Vector3.new(1, 0, 0)
Vector3.yAxis = Vector3.new(0, 1, 0)
Vector3.zAxis = Vector3.new(0, 0, 1)

local Vector2 = {}
local Vector2Meta = {}
Vector2Meta.__index = function(v, k)
	if k == "Magnitude" then
		return math.sqrt(v.X * v.X + v.Y * v.Y)
	elseif k == "Unit" then
		local m = math.sqrt(v.X * v.X + v.Y * v.Y)
		if m > 0 then
			return Vector2.new(v.X / m, v.Y / m)
		end
		return Vector2.new(0, 0)
	end
	return Vector2[k]
end
function Vector2Meta.__add(a, b)
	return Vector2.new(a.X + b.X, a.Y + b.Y)
end
function Vector2Meta.__sub(a, b)
	return Vector2.new(a.X - b.X, a.Y - b.Y)
end
function Vector2Meta.__unm(a)
	return Vector2.new(-a.X, -a.Y)
end
function Vector2Meta.__mul(a, b)
	if type(a) == "number" then
		return Vector2.new(a * b.X, a * b.Y)
	elseif type(b) == "number" then
		return Vector2.new(a.X * b, a.Y * b)
	end
	return Vector2.new(a.X * b.X, a.Y * b.Y)
end
function Vector2Meta.__div(a, b)
	if type(b) == "number" then
		return Vector2.new(a.X / b, a.Y / b)
	end
	return Vector2.new(a.X / b.X, a.Y / b.Y)
end
function Vector2Meta.__eq(a, b)
	return a.X == b.X and a.Y == b.Y
end
function Vector2Meta.__tostring(v)
	return string.format("%g, %g", v.X, v.Y)
end
function Vector2.new(x, y)
	return tagType(setmetatable({ X = x or 0, Y = y or 0 }, Vector2Meta), "Vector2")
end
function Vector2:Dot(b)
	return self.X * b.X + self.Y * b.Y
end
function Vector2:Lerp(b, alpha)
	return self + (b - self) * alpha
end
Vector2.zero = Vector2.new(0, 0)
Vector2.one = Vector2.new(1, 1)

local CFrame = {}
local CFrameMeta = {}
CFrameMeta.__index = function(cf, k)
	if k == "Position" or k == "p" then
		return Vector3.new(cf._x, cf._y, cf._z)
	elseif k == "X" then
		return cf._x
	elseif k == "Y" then
		return cf._y
	elseif k == "Z" then
		return cf._z
	elseif k == "LookVector" then
		return Vector3.new(-cf._r02, -cf._r12, -cf._r22)
	elseif k == "RightVector" then
		return Vector3.new(cf._r00, cf._r10, cf._r20)
	elseif k == "UpVector" then
		return Vector3.new(cf._r01, cf._r11, cf._r21)
	end
	return CFrame[k]
end
local function makeCFrame(x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22)
	return tagType(setmetatable({
		_x = x or 0,
		_y = y or 0,
		_z = z or 0,
		_r00 = r00 or 1,
		_r01 = r01 or 0,
		_r02 = r02 or 0,
		_r10 = r10 or 0,
		_r11 = r11 or 1,
		_r12 = r12 or 0,
		_r20 = r20 or 0,
		_r21 = r21 or 0,
		_r22 = r22 or 1,
	}, CFrameMeta), "CFrame")
end
function CFrameMeta.__mul(a, b)
	if robloxTypeof(b) == "Vector3" then
		return Vector3.new(
			a._x + a._r00 * b.X + a._r01 * b.Y + a._r02 * b.Z,
			a._y + a._r10 * b.X + a._r11 * b.Y + a._r12 * b.Z,
			a._z + a._r20 * b.X + a._r21 * b.Y + a._r22 * b.Z
		)
	end
	local nx = a._x + a._r00 * b._x + a._r01 * b._y + a._r02 * b._z
	local ny = a._y + a._r10 * b._x + a._r11 * b._y + a._r12 * b._z
	local nz = a._z + a._r20 * b._x + a._r21 * b._y + a._r22 * b._z
	return makeCFrame(
		nx,
		ny,
		nz,
		a._r00 * b._r00 + a._r01 * b._r10 + a._r02 * b._r20,
		a._r00 * b._r01 + a._r01 * b._r11 + a._r02 * b._r21,
		a._r00 * b._r02 + a._r01 * b._r12 + a._r02 * b._r22,
		a._r10 * b._r00 + a._r11 * b._r10 + a._r12 * b._r20,
		a._r10 * b._r01 + a._r11 * b._r11 + a._r12 * b._r21,
		a._r10 * b._r02 + a._r11 * b._r12 + a._r12 * b._r22,
		a._r20 * b._r00 + a._r21 * b._r10 + a._r22 * b._r20,
		a._r20 * b._r01 + a._r21 * b._r11 + a._r22 * b._r21,
		a._r20 * b._r02 + a._r21 * b._r12 + a._r22 * b._r22
	)
end
function CFrameMeta.__add(a, v)
	return makeCFrame(a._x + v.X, a._y + v.Y, a._z + v.Z, a._r00, a._r01, a._r02, a._r10, a._r11, a._r12, a._r20, a._r21, a._r22)
end
function CFrameMeta.__sub(a, v)
	return makeCFrame(a._x - v.X, a._y - v.Y, a._z - v.Z, a._r00, a._r01, a._r02, a._r10, a._r11, a._r12, a._r20, a._r21, a._r22)
end
function CFrameMeta.__tostring(cf)
	return string.format("%g, %g, %g", cf._x, cf._y, cf._z)
end
function CFrame.new(...)
	local n = select("#", ...)
	if n == 0 then
		return makeCFrame(0, 0, 0)
	elseif n == 1 then
		local a = ...
		if robloxTypeof(a) == "Vector3" then
			return makeCFrame(a.X, a.Y, a.Z)
		end
		return makeCFrame(0, 0, 0)
	elseif n == 2 then
		local pos, target = ...
		return CFrame.lookAt(pos, target)
	elseif n == 3 then
		local x, y, z = ...
		return makeCFrame(x, y, z)
	elseif n >= 12 then
		return makeCFrame(...)
	end
	return makeCFrame(0, 0, 0)
end
function CFrame.Angles(rx, ry, rz)
	rx, ry, rz = rx or 0, ry or 0, rz or 0
	local cx, sx = math.cos(rx), math.sin(rx)
	local cy, sy = math.cos(ry), math.sin(ry)
	local cz, sz = math.cos(rz), math.sin(rz)
	return makeCFrame(
		0,
		0,
		0,
		cy * cz,
		-cy * sz,
		sy,
		cx * sz + sx * sy * cz,
		cx * cz - sx * sy * sz,
		-sx * cy,
		sx * sz - cx * sy * cz,
		sx * cz + cx * sy * sz,
		cx * cy
	)
end
CFrame.fromEulerAnglesXYZ = CFrame.Angles
function CFrame.fromOrientation(rx, ry, rz)
	return CFrame.Angles(0, ry or 0, 0) * CFrame.Angles(rx or 0, 0, 0) * CFrame.Angles(0, 0, rz or 0)
end
function CFrame.fromAxisAngle(axis, angle)
	axis = axis or Vector3.yAxis
	angle = angle or 0
	local mag = axis.Magnitude
	if mag < 1e-6 then
		return CFrame.identity
	end
	local u = axis / mag
	local c = math.cos(angle)
	local s = math.sin(angle)
	local t = 1 - c
	return makeCFrame(
		0,
		0,
		0,
		t * u.X * u.X + c,
		t * u.X * u.Y - s * u.Z,
		t * u.X * u.Z + s * u.Y,
		t * u.X * u.Y + s * u.Z,
		t * u.Y * u.Y + c,
		t * u.Y * u.Z - s * u.X,
		t * u.X * u.Z - s * u.Y,
		t * u.Y * u.Z + s * u.X,
		t * u.Z * u.Z + c
	)
end
function CFrame.fromMatrix(pos, vX, vY, vZ)
	pos = pos or Vector3.zero
	vX = vX or Vector3.xAxis
	vY = vY or Vector3.yAxis
	vZ = vZ or vX:Cross(vY)
	return makeCFrame(pos.X, pos.Y, pos.Z, vX.X, vY.X, vZ.X, vX.Y, vY.Y, vZ.Y, vX.Z, vY.Z, vZ.Z)
end
function CFrame.lookAt(at, lookAt, up)
	up = up or Vector3.yAxis
	local f = (at - lookAt)
	if f.Magnitude < 1e-6 then
		return makeCFrame(at.X, at.Y, at.Z)
	end
	local zAxis = f.Unit
	local xAxis = up:Cross(zAxis)
	if xAxis.Magnitude < 1e-6 then
		xAxis = Vector3.xAxis
	else
		xAxis = xAxis.Unit
	end
	local yAxis = zAxis:Cross(xAxis)
	return makeCFrame(at.X, at.Y, at.Z, xAxis.X, yAxis.X, zAxis.X, xAxis.Y, yAxis.Y, zAxis.Y, xAxis.Z, yAxis.Z, zAxis.Z)
end
function CFrame:Inverse()
	local r00, r01, r02 = self._r00, self._r10, self._r20
	local r10, r11, r12 = self._r01, self._r11, self._r21
	local r20, r21, r22 = self._r02, self._r12, self._r22
	local tx = -(r00 * self._x + r01 * self._y + r02 * self._z)
	local ty = -(r10 * self._x + r11 * self._y + r12 * self._z)
	local tz = -(r20 * self._x + r21 * self._y + r22 * self._z)
	return makeCFrame(tx, ty, tz, r00, r01, r02, r10, r11, r12, r20, r21, r22)
end
function CFrame:Lerp(goal, alpha)
	return makeCFrame(
		self._x + (goal._x - self._x) * alpha,
		self._y + (goal._y - self._y) * alpha,
		self._z + (goal._z - self._z) * alpha,
		goal._r00,
		goal._r01,
		goal._r02,
		goal._r10,
		goal._r11,
		goal._r12,
		goal._r20,
		goal._r21,
		goal._r22
	)
end
function CFrame:ToWorldSpace(cf)
	return self * cf
end
function CFrame:ToObjectSpace(cf)
	return self:Inverse() * cf
end
function CFrame:ToEulerAnglesXYZ()
	return 0, 0, 0
end
function CFrame:GetComponents()
	return self._x, self._y, self._z, self._r00, self._r01, self._r02, self._r10, self._r11, self._r12, self._r20, self._r21, self._r22
end
CFrame.identity = makeCFrame(0, 0, 0)

local UDim = {}
local UDimMeta = {
	__index = UDim,
	__eq = function(a, b)
		return a.Scale == b.Scale and a.Offset == b.Offset
	end,
	__tostring = function(u)
		return string.format("%g, %g", u.Scale, u.Offset)
	end,
}
function UDim.new(scale, offset)
	return tagType(setmetatable({ Scale = scale or 0, Offset = offset or 0 }, UDimMeta), "UDim")
end

local UDim2 = {}
local UDim2Meta = {
	__index = UDim2,
	__add = function(a, b)
		return UDim2.new(a.X.Scale + b.X.Scale, a.X.Offset + b.X.Offset, a.Y.Scale + b.Y.Scale, a.Y.Offset + b.Y.Offset)
	end,
	__sub = function(a, b)
		return UDim2.new(a.X.Scale - b.X.Scale, a.X.Offset - b.X.Offset, a.Y.Scale - b.Y.Scale, a.Y.Offset - b.Y.Offset)
	end,
	__eq = function(a, b)
		return a.X == b.X and a.Y == b.Y
	end,
	__tostring = function(u)
		return string.format("{%g, %g}, {%g, %g}", u.X.Scale, u.X.Offset, u.Y.Scale, u.Y.Offset)
	end,
}
function UDim2.new(xs, xo, ys, yo)
	if robloxTypeof(xs) == "UDim" and robloxTypeof(xo) == "UDim" then
		return tagType(setmetatable({ X = xs, Y = xo, Width = xs, Height = xo }, UDim2Meta), "UDim2")
	end
	local x = UDim.new(xs or 0, xo or 0)
	local y = UDim.new(ys or 0, yo or 0)
	return tagType(setmetatable({ X = x, Y = y, Width = x, Height = y }, UDim2Meta), "UDim2")
end
function UDim2.fromScale(xs, ys)
	return UDim2.new(xs or 0, 0, ys or 0, 0)
end
function UDim2.fromOffset(xo, yo)
	return UDim2.new(0, xo or 0, 0, yo or 0)
end
function UDim2:Lerp(goal, alpha)
	return UDim2.new(
		self.X.Scale + (goal.X.Scale - self.X.Scale) * alpha,
		self.X.Offset + (goal.X.Offset - self.X.Offset) * alpha,
		self.Y.Scale + (goal.Y.Scale - self.Y.Scale) * alpha,
		self.Y.Offset + (goal.Y.Offset - self.Y.Offset) * alpha
	)
end

local Color3 = {}
local Color3Meta = {
	__index = Color3,
	__eq = function(a, b)
		return a.R == b.R and a.G == b.G and a.B == b.B
	end,
	__tostring = function(c)
		return string.format("%g, %g, %g", c.R, c.G, c.B)
	end,
}
function Color3.new(r, g, b)
	return tagType(setmetatable({ R = r or 0, G = g or 0, B = b or 0 }, Color3Meta), "Color3")
end
function Color3.fromRGB(r, g, b)
	return Color3.new((r or 0) / 255, (g or 0) / 255, (b or 0) / 255)
end
function Color3.fromHSV(h, s, v)
	local i = math.floor(h * 6)
	local f = h * 6 - i
	local p = v * (1 - s)
	local q = v * (1 - f * s)
	local t = v * (1 - (1 - f) * s)
	local mod = i % 6
	if mod == 0 then
		return Color3.new(v, t, p)
	elseif mod == 1 then
		return Color3.new(q, v, p)
	elseif mod == 2 then
		return Color3.new(p, v, t)
	elseif mod == 3 then
		return Color3.new(p, q, v)
	elseif mod == 4 then
		return Color3.new(t, p, v)
	end
	return Color3.new(v, p, q)
end
function Color3.fromHex(hex)
	local clean = tostring(hex):gsub("^#", "")
	if #clean == 6 then
		local r = tonumber(clean:sub(1, 2), 16) or 0
		local g = tonumber(clean:sub(3, 4), 16) or 0
		local b = tonumber(clean:sub(5, 6), 16) or 0
		return Color3.fromRGB(r, g, b)
	end
	return Color3.new(1, 1, 1)
end
function Color3:Lerp(goal, alpha)
	return Color3.new(
		self.R + (goal.R - self.R) * alpha,
		self.G + (goal.G - self.G) * alpha,
		self.B + (goal.B - self.B) * alpha
	)
end
function Color3:ToHex()
	return string.format(
		"%02x%02x%02x",
		math.clamp(math.round(self.R * 255), 0, 255),
		math.clamp(math.round(self.G * 255), 0, 255),
		math.clamp(math.round(self.B * 255), 0, 255)
	)
end
function Color3:ToHSV()
	return 0, 0, self.R
end

local BrickColor = {}
local BrickColorMeta = { __index = BrickColor }
function BrickColor.new(val)
	local name = if type(val) == "string" then val else "Medium stone grey"
	return tagType(setmetatable({ Name = name, Number = 194, Color = Color3.fromRGB(163, 162, 165) }, BrickColorMeta), "BrickColor")
end
function BrickColor.Random()
	return BrickColor.new("Bright blue")
end
function BrickColor.White()
	return BrickColor.new("White")
end
function BrickColor.Black()
	return BrickColor.new("Black")
end
function BrickColor.Red()
	return BrickColor.new("Bright red")
end
function BrickColor.Blue()
	return BrickColor.new("Bright blue")
end
function BrickColor.Green()
	return BrickColor.new("Bright green")
end
function BrickColor.Yellow()
	return BrickColor.new("Bright yellow")
end

local ColorSequenceKeypoint = {
	new = function(t, c)
		return tagType({ Time = t, Value = c }, "ColorSequenceKeypoint")
	end,
}
local ColorSequence = {
	new = function(a, b)
		local kps = if type(a) == "table" and a[1] ~= nil
			then a
			else { ColorSequenceKeypoint.new(0, a), ColorSequenceKeypoint.new(1, b or a) }
		return tagType({ Keypoints = kps }, "ColorSequence")
	end,
}
local NumberSequenceKeypoint = {
	new = function(t, v, env)
		return tagType({ Time = t, Value = v, Envelope = env or 0 }, "NumberSequenceKeypoint")
	end,
}
local NumberSequence = {
	new = function(a, b)
		local kps = if type(a) == "table" and a[1] ~= nil
			then a
			else { NumberSequenceKeypoint.new(0, a), NumberSequenceKeypoint.new(1, b or a) }
		return tagType({ Keypoints = kps }, "NumberSequence")
	end,
}
local NumberRange = {
	new = function(mn, mx)
		return tagType({ Min = mn or 0, Max = mx or mn or 0 }, "NumberRange")
	end,
}
local Rect = {
	new = function(x0, y0, x1, y1)
		return tagType({ Min = Vector2.new(x0 or 0, y0 or 0), Max = Vector2.new(x1 or 0, y1 or 0), Width = (x1 or 0) - (x0 or 0), Height = (y1 or 0) - (y0 or 0) }, "Rect")
	end,
}
local TweenInfo = {
	new = function(t, style, dir, rep, rev, delay)
		return tagType({
			Time = t or 1,
			EasingStyle = style,
			EasingDirection = dir,
			RepeatCount = rep or 0,
			Reverses = rev or false,
			DelayTime = delay or 0,
		}, "TweenInfo")
	end,
}
local Ray = {
	new = function(origin, direction)
		return tagType({ Origin = origin or Vector3.zero, Direction = direction or Vector3.zero }, "Ray")
	end,
}
local RaycastParams = {
	new = function()
		return tagType({ FilterDescendantsInstances = {}, FilterType = nil, IgnoreWater = false }, "RaycastParams")
	end,
}
local OverlapParams = {
	new = function()
		return tagType({ FilterDescendantsInstances = {}, FilterType = nil, MaxParts = 0 }, "OverlapParams")
	end,
}
local Random = {}
Random.__index = Random
function Random.new(seed)
	local s = seed or os.clock() * 1000
	return tagType(setmetatable({ _seed = s }, Random), "Random")
end
function Random:NextNumber(mn, mx)
	local r = math.random()
	if mn and mx then
		return mn + r * (mx - mn)
	elseif mn then
		return r * mn
	end
	return r
end
function Random:NextInteger(mn, mx)
	return math.random(mn, mx)
end
function Random:Clone()
	return Random.new(self._seed)
end

local DateTimeShim = {}
function DateTimeShim.now()
	local now = datetime.now()
	return tagType({
		UnixTimestamp = now.unixTimestamp,
		UnixTimestampMillis = now.unixTimestampMillis,
		ToIsoDate = function()
			return now:toIsoDate()
		end,
	}, "DateTime")
end
function DateTimeShim.fromUnixTimestamp(ts)
	return tagType({ UnixTimestamp = ts, UnixTimestampMillis = ts * 1000 }, "DateTime")
end
function DateTimeShim.fromUnixTimestampMillis(ms)
	return tagType({ UnixTimestamp = math.floor(ms / 1000), UnixTimestampMillis = ms }, "DateTime")
end

-- Dynamic Enum registry with explicit canonical items + fallback generator
local Enum = {}
local EnumTypes = {}

local function registerEnum(enumName, items)
	local itemsList = {}
	local enumObj = tagType({
		GetEnumItems = function()
			return table.clone(itemsList)
		end,
	}, "Enum")
	for itemName, val in pairs(items) do
		local item = tagType({
			Name = itemName,
			Value = val,
			EnumType = enumObj,
		}, "EnumItem")
		setmetatable(item, {
			__tostring = function(t)
				return "Enum." .. enumName .. "." .. t.Name
			end,
		})
		enumObj[itemName] = item
		table.insert(itemsList, item)
	end
	setmetatable(enumObj, {
		__index = function(t, itemName)
			local val = 0
			for i = 1, #itemName do
				val = (val * 31 + string.byte(itemName, i)) % 100000
			end
			local item = tagType({ Name = itemName, Value = val, EnumType = t }, "EnumItem")
			setmetatable(item, {
				__tostring = function(x)
					return "Enum." .. enumName .. "." .. x.Name
				end,
			})
			rawset(t, itemName, item)
			table.insert(itemsList, item)
			return item
		end,
	})
	EnumTypes[enumName] = enumObj
	Enum[enumName] = enumObj
	return enumObj
end

registerEnum("ProductPurchaseDecision", { NotProcessedYet = 0, PurchaseGranted = 1 })
registerEnum("CurrencyType", { Robux = 1, Tix = 2 })
registerEnum("InfoType", { Asset = 0, Product = 1, GamePass = 2, Subscription = 3, Bundle = 4 })
registerEnum("ZIndexBehavior", { Global = 0, Sibling = 1 })
registerEnum("ScreenInsets", { None = 0, CoreUISafeInsets = 1, DeviceSafeInsets = 2, TopbarSafeInsets = 3 })
registerEnum("SortOrder", { Name = 0, Custom = 1, LayoutOrder = 2 })
registerEnum("FillDirection", { Horizontal = 0, Vertical = 1 })
registerEnum("HorizontalAlignment", { Center = 0, Left = 1, Right = 2 })
registerEnum("VerticalAlignment", { Center = 0, Top = 1, Bottom = 2 })
registerEnum("TextXAlignment", { Left = 0, Right = 1, Center = 2 })
registerEnum("TextYAlignment", { Top = 0, Center = 1, Bottom = 2 })
registerEnum("ScaleType", { Stretch = 0, Slice = 1, Tile = 2, Fit = 3, Crop = 4 })
registerEnum("EasingStyle", { Linear = 0, Sine = 1, Back = 2, Quad = 3, Quart = 4, Quint = 5, Bounce = 6, Elastic = 7, Exponential = 8, Circular = 9, Cubic = 10 })
registerEnum("EasingDirection", { In = 0, Out = 1, InOut = 2 })
registerEnum("Material", { Plastic = 256, SmoothPlastic = 272, Neon = 288, Metal = 1088, Wood = 512, Slate = 800, Concrete = 816, Glass = 1568, ForceField = 1584 })
registerEnum("PartType", { Ball = 0, Block = 1, Cylinder = 2, Wedge = 3, CornerWedge = 4 })
registerEnum("MembershipType", { None = 0, BuildersClub = 1, TurboBuildersClub = 2, OutrageousBuildersClub = 3, Premium = 4 })
registerEnum("AnalyticsEconomyFlowType", { Sink = 1, Source = 2 })
registerEnum("UserInputType", { MouseButton1 = 0, MouseButton2 = 1, MouseButton3 = 2, Keyboard = 8, Touch = 7, Gamepad1 = 12 })
registerEnum("KeyCode", { Unknown = 0, Space = 32, E = 101, F = 102, Q = 113, R = 114, W = 119, A = 97, S = 115, D = 100, Escape = 27, Return = 13 })
registerEnum("Font", { Legacy = 0, Arial = 1, ArialBold = 2, SourceSans = 3, SourceSansBold = 4, Gotham = 17, GothamBold = 19, GothamBlack = 20, FredokaOne = 34 })

setmetatable(Enum, {
	__index = function(_, enumName)
		return registerEnum(enumName, {})
	end,
})

-- ---------------------------------------------------------------------------
-- Instance System + Services
-- ---------------------------------------------------------------------------
local INST_STATE = setmetatable({}, { __mode = "k" })
local TAG_TO_INSTANCES = {}
local TAG_ADDED_SIGNALS = {}
local TAG_REMOVED_SIGNALS = {}

local CLASS_ANCESTORS = {
	Script = { LuaSourceContainer = true, Instance = true },
	LocalScript = { LuaSourceContainer = true, Instance = true },
	ModuleScript = { LuaSourceContainer = true, Instance = true },
	Part = { BasePart = true, PVInstance = true, Instance = true },
	SpawnLocation = { BasePart = true, PVInstance = true, Instance = true },
	MeshPart = { BasePart = true, PVInstance = true, Instance = true },
	WedgePart = { BasePart = true, PVInstance = true, Instance = true },
	TrussPart = { BasePart = true, PVInstance = true, Instance = true },
	CornerWedgePart = { BasePart = true, PVInstance = true, Instance = true },
	Seat = { BasePart = true, PVInstance = true, Instance = true },
	VehicleSeat = { BasePart = true, PVInstance = true, Instance = true },
	Model = { PVInstance = true, Instance = true },
	ScreenGui = { LayerCollector = true, GuiBase2d = true, GuiBase = true, Instance = true },
	BillboardGui = { LayerCollector = true, GuiBase2d = true, GuiBase = true, Instance = true },
	SurfaceGui = { LayerCollector = true, GuiBase2d = true, GuiBase = true, Instance = true },
	Frame = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	ScrollingFrame = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	TextLabel = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	ImageLabel = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	TextBox = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	ViewportFrame = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	CanvasGroup = { GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	TextButton = { GuiButton = true, GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	ImageButton = { GuiButton = true, GuiObject = true, GuiBase2d = true, GuiBase = true, Instance = true },
	UICorner = { UIComponent = true, Instance = true },
	UIStroke = { UIComponent = true, Instance = true },
	UIListLayout = { UILayout = true, UIGridStyleLayout = true, UIComponent = true, Instance = true },
	UIGridLayout = { UILayout = true, UIGridStyleLayout = true, UIComponent = true, Instance = true },
	UIPadding = { UIComponent = true, Instance = true },
	UIAspectRatioConstraint = { UIConstraint = true, UIComponent = true, Instance = true },
	UISizeConstraint = { UIConstraint = true, UIComponent = true, Instance = true },
	UIGradient = { UIComponent = true, Instance = true },
	UIScale = { UIComponent = true, Instance = true },
	IntValue = { ValueBase = true, Instance = true },
	NumberValue = { ValueBase = true, Instance = true },
	StringValue = { ValueBase = true, Instance = true },
	BoolValue = { ValueBase = true, Instance = true },
	ObjectValue = { ValueBase = true, Instance = true },
	Color3Value = { ValueBase = true, Instance = true },
	Vector3Value = { ValueBase = true, Instance = true },
	CFrameValue = { ValueBase = true, Instance = true },
}

local CLASS_DEFAULTS = {
	Script = { Enabled = true, Disabled = false, Source = "" },
	LocalScript = { Enabled = true, Disabled = false, Source = "" },
	ModuleScript = { Source = "" },
	ScreenGui = { Enabled = true, ResetOnSpawn = true, IgnoreGuiInset = false, DisplayOrder = 0, ZIndexBehavior = Enum.ZIndexBehavior.Sibling },
	Frame = { Visible = true, Size = UDim2.new(0, 100, 0, 100), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, ZIndex = 1 },
	ScrollingFrame = { Visible = true, Size = UDim2.new(0, 100, 0, 100), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, CanvasSize = UDim2.new(0, 0, 2, 0), ZIndex = 1 },
	TextLabel = { Visible = true, Text = "", Size = UDim2.new(0, 100, 0, 30), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, TextTransparency = 0, ZIndex = 1 },
	ImageLabel = { Visible = true, Image = "", Size = UDim2.new(0, 100, 0, 100), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, ImageTransparency = 0, ZIndex = 1 },
	TextButton = { Visible = true, Active = true, Text = "", Size = UDim2.new(0, 100, 0, 40), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, ZIndex = 1 },
	ImageButton = { Visible = true, Active = true, Image = "", Size = UDim2.new(0, 100, 0, 100), Position = UDim2.new(), AnchorPoint = Vector2.zero, BackgroundTransparency = 0, ZIndex = 1 },
	TextBox = { Visible = true, Active = true, Text = "", PlaceholderText = "", Size = UDim2.new(0, 100, 0, 30), Position = UDim2.new(), AnchorPoint = Vector2.zero, ZIndex = 1 },
	UIScale = { Scale = 1 },
	UICorner = { CornerRadius = UDim.new(0, 4) },
	UIStroke = { Thickness = 1, Transparency = 0 },
	IntValue = { Value = 0 },
	NumberValue = { Value = 0 },
	StringValue = { Value = "" },
	BoolValue = { Value = false },
	Color3Value = { Value = Color3.new(1, 1, 1) },
	Vector3Value = { Value = Vector3.zero },
	CFrameValue = { Value = CFrame.identity },
	Part = { Anchored = false, CanCollide = true, Transparency = 0, Size = Vector3.new(4, 1, 2), Position = Vector3.zero, CFrame = CFrame.identity },
	SpawnLocation = { Anchored = true, CanCollide = true, Transparency = 0, Size = Vector3.new(6, 1, 6), Position = Vector3.zero, CFrame = CFrame.identity },
	MeshPart = { Anchored = false, CanCollide = true, Transparency = 0, Size = Vector3.new(4, 4, 4), Position = Vector3.zero, CFrame = CFrame.identity },
	Humanoid = { Health = 100, MaxHealth = 100, WalkSpeed = 16, JumpPower = 50, JumpHeight = 7.2 },
	Camera = { ViewportSize = Vector2.new(1280, 720), FieldOfView = 70, CFrame = CFrame.identity },
	ProximityPrompt = { Enabled = true, ActionText = "Interact", ObjectText = "", HoldDuration = 0, MaxActivationDistance = 10 },
	Sound = { Volume = 0.5, Playing = false, Looped = false, TimePosition = 0 },
}

-- Forward declarations for client context & counters
local getActiveClientPlayer
local remotesFiredCount = 0
local dataModel -- assigned below

local InstanceMethods = {}

local function getState(inst)
	return INST_STATE[inst]
end

local function isInstance(v)
	return INST_STATE[v] ~= nil
end

local function addTagInternal(inst, tag)
	local st = INST_STATE[inst]
	if not st or st.tags[tag] then
		return
	end
	st.tags[tag] = true
	local list = TAG_TO_INSTANCES[tag]
	if not list then
		list = {}
		TAG_TO_INSTANCES[tag] = list
	end
	table.insert(list, inst)
	local sig = TAG_ADDED_SIGNALS[tag]
	if sig then
		sig:Fire(inst)
	end
end

local function removeTagInternal(inst, tag)
	local st = INST_STATE[inst]
	if not st or not st.tags[tag] then
		return
	end
	st.tags[tag] = nil
	local list = TAG_TO_INSTANCES[tag]
	if list then
		for i = #list, 1, -1 do
			if list[i] == inst then
				table.remove(list, i)
			end
		end
	end
	local sig = TAG_REMOVED_SIGNALS[tag]
	if sig then
		sig:Fire(inst)
	end
end

function InstanceMethods:GetChildren()
	local st = INST_STATE[self]
	return table.clone(st.children)
end

function InstanceMethods:GetDescendants()
	local out = {}
	local function walk(node)
		local st = INST_STATE[node]
		for _, ch in ipairs(st.children) do
			table.insert(out, ch)
			walk(ch)
		end
	end
	walk(self)
	return out
end

function InstanceMethods:FindFirstChild(name, recursive)
	local st = INST_STATE[self]
	for _, ch in ipairs(st.children) do
		if INST_STATE[ch].props.Name == name then
			return ch
		end
	end
	if recursive then
		for _, ch in ipairs(st.children) do
			local found = ch:FindFirstChild(name, true)
			if found then
				return found
			end
		end
	end
	return nil
end

function InstanceMethods:FindFirstChildOfClass(className)
	local st = INST_STATE[self]
	for _, ch in ipairs(st.children) do
		if INST_STATE[ch].className == className then
			return ch
		end
	end
	return nil
end

function InstanceMethods:FindFirstChildWhichIsA(className, recursive)
	local st = INST_STATE[self]
	for _, ch in ipairs(st.children) do
		if ch:IsA(className) then
			return ch
		end
	end
	if recursive then
		for _, ch in ipairs(st.children) do
			local found = ch:FindFirstChildWhichIsA(className, true)
			if found then
				return found
			end
		end
	end
	return nil
end

function InstanceMethods:WaitForChild(name, timeout)
	local found = self:FindFirstChild(name)
	if found then
		return found
	end
	local cap = math.min(timeout or 2, 2)
	local deadline = os.clock() + cap
	while os.clock() < deadline do
		task.wait(0.01)
		found = self:FindFirstChild(name)
		if found then
			return found
		end
	end
	if timeout == nil then
		pushWarning(string.format("Infinite yield possible on '%s:WaitForChild(\"%s\")'", self:GetFullName(), tostring(name)))
	end
	return nil
end

function InstanceMethods:IsA(className)
	local st = INST_STATE[self]
	if st.className == className or className == "Instance" then
		return true
	end
	local anc = CLASS_ANCESTORS[st.className]
	return anc ~= nil and anc[className] == true
end

function InstanceMethods:IsDescendantOf(ancestor)
	local cur = INST_STATE[self].parent
	while cur do
		if cur == ancestor then
			return true
		end
		cur = INST_STATE[cur].parent
	end
	return false
end

function InstanceMethods:IsAncestorOf(descendant)
	if not isInstance(descendant) then
		return false
	end
	return descendant:IsDescendantOf(self)
end

function InstanceMethods:GetFullName()
	local parts = {}
	local cur = self
	while cur and cur ~= dataModel do
		local st = INST_STATE[cur]
		table.insert(parts, 1, st.props.Name or st.className)
		cur = st.parent
	end
	return table.concat(parts, ".")
end

function InstanceMethods:GetAttribute(name)
	return INST_STATE[self].attributes[name]
end

function InstanceMethods:SetAttribute(name, value)
	local st = INST_STATE[self]
	st.attributes[name] = value
	local sig = st.attrSignals[name]
	if sig then
		sig:Fire()
	end
	if st.AttributeChanged then
		st.AttributeChanged:Fire(name)
	end
end

function InstanceMethods:GetAttributes()
	return table.clone(INST_STATE[self].attributes)
end

function InstanceMethods:GetAttributeChangedSignal(name)
	local st = INST_STATE[self]
	if not st.attrSignals[name] then
		st.attrSignals[name] = newSignal(self:GetFullName() .. ".AttributeChanged(" .. tostring(name) .. ")")
	end
	return st.attrSignals[name]
end

function InstanceMethods:GetPropertyChangedSignal(prop)
	local st = INST_STATE[self]
	if not st.propSignals[prop] then
		st.propSignals[prop] = newSignal(self:GetFullName() .. ".PropertyChanged(" .. tostring(prop) .. ")")
	end
	return st.propSignals[prop]
end

function InstanceMethods:AddTag(tag)
	addTagInternal(self, tag)
end
function InstanceMethods:RemoveTag(tag)
	removeTagInternal(self, tag)
end
function InstanceMethods:HasTag(tag)
	return INST_STATE[self].tags[tag] == true
end
function InstanceMethods:GetTags()
	local out = {}
	for t in pairs(INST_STATE[self].tags) do
		table.insert(out, t)
	end
	return out
end

local createInstance -- forward declaration

function InstanceMethods:Clone()
	local st = INST_STATE[self]
	local copy = createInstance(st.className)
	local copySt = INST_STATE[copy]
	for k, v in pairs(st.props) do
		copySt.props[k] = v
	end
	for k, v in pairs(st.attributes) do
		copySt.attributes[k] = v
	end
	for tag in pairs(st.tags) do
		addTagInternal(copy, tag)
	end
	copySt.sourceFile = st.sourceFile
	for _, ch in ipairs(st.children) do
		local chCopy = ch:Clone()
		chCopy.Parent = copy
	end
	return copy
end

function InstanceMethods:ClearAllChildren()
	local children = self:GetChildren()
	for _, ch in ipairs(children) do
		ch:Destroy()
	end
end

function InstanceMethods:Destroy()
	local st = INST_STATE[self]
	if st.destroyed then
		return
	end
	st.destroyed = true
	self.Parent = nil
	for tag in pairs(table.clone(st.tags)) do
		removeTagInternal(self, tag)
	end
	for _, ch in ipairs(self:GetChildren()) do
		ch:Destroy()
	end
	if st.Destroying then
		st.Destroying:Fire()
	end
end

-- Class-specific methods
local CLASS_METHODS = {
	RemoteEvent = {
		FireServer = function(self, ...)
			remotesFiredCount += 1
			local player = getActiveClientPlayer()
			local st = INST_STATE[self]
			st.OnServerEvent:Fire(player, ...)
		end,
		FireClient = function(self, player, ...)
			remotesFiredCount += 1
			local st = INST_STATE[self]
			st.OnClientEvent:FireFiltered(function(meta)
				return meta == nil or meta.player == player
			end, ...)
		end,
		FireAllClients = function(self, ...)
			remotesFiredCount += 1
			local st = INST_STATE[self]
			st.OnClientEvent:Fire(...)
		end,
	},
	RemoteFunction = {
		InvokeServer = function(self, ...)
			remotesFiredCount += 1
			local player = getActiveClientPlayer()
			local st = INST_STATE[self]
			local cb = st.props.OnServerInvoke
			if type(cb) ~= "function" then
				error(string.format("RemoteFunction '%s' has no OnServerInvoke callback", self:GetFullName()))
			end
			return cb(player, ...)
		end,
		InvokeClient = function(self, player, ...)
			remotesFiredCount += 1
			local st = INST_STATE[self]
			local cb = st.props.OnClientInvoke
			if type(cb) ~= "function" then
				error(string.format("RemoteFunction '%s' has no OnClientInvoke callback", self:GetFullName()))
			end
			return cb(...)
		end,
	},
	BindableEvent = {
		Fire = function(self, ...)
			INST_STATE[self].Event:Fire(...)
		end,
	},
	BindableFunction = {
		Invoke = function(self, ...)
			local cb = INST_STATE[self].props.OnInvoke
			if type(cb) ~= "function" then
				return nil
			end
			return cb(...)
		end,
	},
	Sound = {
		Play = function(self)
			INST_STATE[self].props.Playing = true
		end,
		Stop = function(self)
			INST_STATE[self].props.Playing = false
		end,
		Pause = function(self)
			INST_STATE[self].props.Playing = false
		end,
	},
	Humanoid = {
		MoveTo = function(self, pos)
			INST_STATE[self]._moveTarget = pos
		end,
		TakeDamage = function(self, amount)
			local st = INST_STATE[self]
			st.props.Health = math.max(0, (st.props.Health or 100) - (amount or 0))
		end,
	},
	Model = {
		GetPivot = function(self)
			return INST_STATE[self].props.WorldPivot or CFrame.identity
		end,
		PivotTo = function(self, cf)
			INST_STATE[self].props.WorldPivot = cf
		end,
		SetPrimaryPartCFrame = function(self, cf)
			INST_STATE[self].props.WorldPivot = cf
		end,
		GetBoundingBox = function()
			return CFrame.identity, Vector3.new(4, 4, 4)
		end,
	},
	BasePart = {
		GetPivot = function(self)
			return INST_STATE[self].props.CFrame or CFrame.identity
		end,
		PivotTo = function(self, cf)
			INST_STATE[self].props.CFrame = cf
		end,
	},
}

local function collectSubtree(root, out)
	table.insert(out, root)
	for _, ch in ipairs(INST_STATE[root].children) do
		collectSubtree(ch, out)
	end
end

local function setParentInternal(inst, newParent)
	local st = INST_STATE[inst]
	local oldParent = st.parent
	if oldParent == newParent then
		return
	end
	local subtree
	if oldParent then
		local oldSt = INST_STATE[oldParent]
		for i, ch in ipairs(oldSt.children) do
			if ch == inst then
				table.remove(oldSt.children, i)
				break
			end
		end
		if oldSt.ChildRemoved then
			oldSt.ChildRemoved:Fire(inst)
		end
		subtree = {}
		collectSubtree(inst, subtree)
		local anc = oldParent
		while anc do
			local ancSt = INST_STATE[anc]
			if ancSt.DescendantRemoving and #ancSt.DescendantRemoving._conns > 0 then
				for _, node in ipairs(subtree) do
					ancSt.DescendantRemoving:Fire(node)
				end
			end
			anc = ancSt.parent
		end
	end
	st.parent = newParent
	if newParent then
		local newSt = INST_STATE[newParent]
		table.insert(newSt.children, inst)
		if newSt.ChildAdded then
			newSt.ChildAdded:Fire(inst)
		end
		if not subtree then
			subtree = {}
			collectSubtree(inst, subtree)
		end
		local anc = newParent
		while anc do
			local ancSt = INST_STATE[anc]
			if ancSt.DescendantAdded and #ancSt.DescendantAdded._conns > 0 then
				for _, node in ipairs(subtree) do
					ancSt.DescendantAdded:Fire(node)
				end
			end
			anc = ancSt.parent
		end
	end
	if st.AncestryChanged then
		st.AncestryChanged:Fire(inst, newParent)
	end
end

local InstanceMeta = {
	__index = function(inst, key)
		local st = INST_STATE[inst]
		if key == "ClassName" then
			return st.className
		elseif key == "Parent" then
			return st.parent
		end
		-- Check instance signals
		if st.signals[key] then
			return st.signals[key]
		end
		-- Check common Instance methods first (before any IsA check to avoid __index recursion)
		if InstanceMethods[key] then
			return InstanceMethods[key]
		end
		-- Check class-specific methods
		local clsMethods = CLASS_METHODS[st.className]
		if clsMethods and clsMethods[key] then
			return clsMethods[key]
		end
		local anc = CLASS_ANCESTORS[st.className]
		if (st.className == "BasePart" or (anc and anc.BasePart)) and CLASS_METHODS.BasePart[key] then
			return CLASS_METHODS.BasePart[key]
		end
		-- Check custom handler on state (e.g. Player:Kick, DataModel:GetService)
		if st.customMethods and st.customMethods[key] then
			return st.customMethods[key]
		end
		-- Check properties
		local prop = st.props[key]
		if prop ~= nil then
			return prop
		end
		-- Check direct child by name
		for _, ch in ipairs(st.children) do
			if INST_STATE[ch].props.Name == key then
				return ch
			end
		end
		return nil
	end,
	__newindex = function(inst, key, val)
		local st = INST_STATE[inst]
		if key == "Parent" then
			setParentInternal(inst, val)
			return
		end
		if st.customSetters and st.customSetters[key] then
			st.customSetters[key](inst, val)
			return
		end
		st.props[key] = val
		if key == "Position" and inst:IsA("BasePart") and robloxTypeof(val) == "Vector3" then
			st.props.CFrame = CFrame.new(val)
		elseif key == "CFrame" and inst:IsA("BasePart") and robloxTypeof(val) == "CFrame" then
			st.props.Position = val.Position
		end
		if st.propSignals[key] then
			st.propSignals[key]:Fire()
		end
		if st.Changed then
			st.Changed:Fire(key)
		end
	end,
	__tostring = function(inst)
		local st = INST_STATE[inst]
		return tostring(st.props.Name or st.className)
	end,
}

createInstance = function(className, parent, initialName)
	local inst = {}
	local props = {
		Name = initialName or className,
	}
	local defs = CLASS_DEFAULTS[className]
	if defs then
		for k, v in pairs(defs) do
			props[k] = v
		end
	end
	local st = {
		className = className,
		parent = nil,
		children = {},
		props = props,
		attributes = {},
		tags = {},
		signals = {},
		propSignals = {},
		attrSignals = {},
		customMethods = nil,
		customSetters = nil,
		destroyed = false,
		sourceFile = nil,
	}
	st.Changed = newSignal(className .. ".Changed")
	st.ChildAdded = newSignal(className .. ".ChildAdded")
	st.ChildRemoved = newSignal(className .. ".ChildRemoved")
	st.DescendantAdded = newSignal(className .. ".DescendantAdded")
	st.DescendantRemoving = newSignal(className .. ".DescendantRemoving")
	st.AncestryChanged = newSignal(className .. ".AncestryChanged")
	st.AttributeChanged = newSignal(className .. ".AttributeChanged")
	st.Destroying = newSignal(className .. ".Destroying")
	st.signals.Changed = st.Changed
	st.signals.ChildAdded = st.ChildAdded
	st.signals.ChildRemoved = st.ChildRemoved
	st.signals.DescendantAdded = st.DescendantAdded
	st.signals.DescendantRemoving = st.DescendantRemoving
	st.signals.AncestryChanged = st.AncestryChanged
	st.signals.AttributeChanged = st.AttributeChanged
	st.signals.Destroying = st.Destroying

	if className == "RemoteEvent" then
		st.OnServerEvent = newSignal("RemoteEvent.OnServerEvent")
		st.OnClientEvent = newSignal("RemoteEvent.OnClientEvent")
		st.signals.OnServerEvent = st.OnServerEvent
		st.signals.OnClientEvent = st.OnClientEvent
	elseif className == "BindableEvent" then
		st.Event = newSignal("BindableEvent.Event")
		st.signals.Event = st.Event
	elseif className == "TextButton" or className == "ImageButton" then
		st.MouseButton1Click = newSignal(className .. ".MouseButton1Click")
		st.MouseButton1Down = newSignal(className .. ".MouseButton1Down")
		st.MouseButton1Up = newSignal(className .. ".MouseButton1Up")
		st.MouseEnter = newSignal(className .. ".MouseEnter")
		st.MouseLeave = newSignal(className .. ".MouseLeave")
		st.Activated = newSignal(className .. ".Activated")
		st.signals.MouseButton1Click = st.MouseButton1Click
		st.signals.MouseButton1Down = st.MouseButton1Down
		st.signals.MouseButton1Up = st.MouseButton1Up
		st.signals.MouseEnter = st.MouseEnter
		st.signals.MouseLeave = st.MouseLeave
		st.signals.Activated = st.Activated
	elseif className == "ProximityPrompt" then
		st.Triggered = newSignal("ProximityPrompt.Triggered")
		st.signals.Triggered = st.Triggered
	elseif className == "ClickDetector" then
		st.MouseClick = newSignal("ClickDetector.MouseClick")
		st.signals.MouseClick = st.MouseClick
	elseif className == "Humanoid" then
		st.Died = newSignal("Humanoid.Died")
		st.signals.Died = st.Died
	elseif className == "BasePart" or className == "Part" or className == "SpawnLocation" or className == "MeshPart" then
		st.Touched = newSignal(className .. ".Touched")
		st.TouchEnded = newSignal(className .. ".TouchEnded")
		st.signals.Touched = st.Touched
		st.signals.TouchEnded = st.TouchEnded
	elseif className == "Player" then
		st.CharacterAdded = newSignal("Player.CharacterAdded")
		st.CharacterRemoving = newSignal("Player.CharacterRemoving")
		st.signals.CharacterAdded = st.CharacterAdded
		st.signals.CharacterRemoving = st.CharacterRemoving
	end

	INST_STATE[inst] = st
	setmetatable(inst, InstanceMeta)
	tagType(inst, "Instance")

	if parent then
		setParentInternal(inst, parent)
	end
	return inst
end

local InstanceAPI = {
	new = function(className, parent)
		return createInstance(className, parent)
	end,
}

-- ---------------------------------------------------------------------------
-- Services Implementation
-- ---------------------------------------------------------------------------
local services = {}
local bindToCloseCallbacks = {}
local isLoadedFlag = true

dataModel = createInstance("DataModel", nil, inputData.project_name or "Game")
INST_STATE[dataModel].props.GameId = 1
INST_STATE[dataModel].props.PlaceId = 1
INST_STATE[dataModel].props.JobId = "openro-sandbox-1"

local function deepCopyValue(v)
	if type(v) ~= "table" then
		return v
	end
	if TYPE_TAGS[v] ~= nil then
		return v
	end
	local copy = {}
	for k, val in pairs(v) do
		copy[deepCopyValue(k)] = deepCopyValue(val)
	end
	return copy
end

-- DataStoreService (persists across rejoin within the sandbox run)
local dataStores = {}
local orderedDataStores = {}

local function getOrCreateStore(registry, name, scope)
	local key = tostring(name) .. "\0" .. tostring(scope or "global")
	if not registry[key] then
		registry[key] = { _name = name, _scope = scope or "global", _data = {} }
	end
	return registry[key]
end

local function makeDataStore(name, scope)
	local store = getOrCreateStore(dataStores, name, scope)
	local api = createInstance("DataStore", nil, name)
	INST_STATE[api].customMethods = {
		GetAsync = function(_, k)
			return deepCopyValue(store._data[tostring(k)])
		end,
		SetAsync = function(_, k, v)
			store._data[tostring(k)] = deepCopyValue(v)
			return "v1"
		end,
		UpdateAsync = function(_, k, transform)
			local sk = tostring(k)
			local current = deepCopyValue(store._data[sk])
			local updated = transform(current)
			if updated ~= nil then
				store._data[sk] = deepCopyValue(updated)
			end
			return deepCopyValue(store._data[sk])
		end,
		IncrementAsync = function(_, k, delta)
			local sk = tostring(k)
			local prev = tonumber(store._data[sk]) or 0
			local nextVal = prev + (delta or 1)
			store._data[sk] = nextVal
			return nextVal
		end,
		RemoveAsync = function(_, k)
			local sk = tostring(k)
			local prev = deepCopyValue(store._data[sk])
			store._data[sk] = nil
			return prev
		end,
	}
	return api
end

local function makeOrderedDataStore(name, scope)
	local store = getOrCreateStore(orderedDataStores, name, scope)
	local api = createInstance("OrderedDataStore", nil, name)
	INST_STATE[api].customMethods = {
		GetAsync = function(_, k)
			return store._data[tostring(k)]
		end,
		SetAsync = function(_, k, v)
			store._data[tostring(k)] = tonumber(v) or 0
			return "v1"
		end,
		UpdateAsync = function(_, k, transform)
			local sk = tostring(k)
			local updated = transform(store._data[sk])
			if updated ~= nil then
				store._data[sk] = tonumber(updated) or 0
			end
			return store._data[sk]
		end,
		IncrementAsync = function(_, k, delta)
			local sk = tostring(k)
			local prev = tonumber(store._data[sk]) or 0
			local nextVal = prev + (delta or 1)
			store._data[sk] = nextVal
			return nextVal
		end,
		RemoveAsync = function(_, k)
			local sk = tostring(k)
			local prev = store._data[sk]
			store._data[sk] = nil
			return prev
		end,
		GetSortedAsync = function(_, ascending, pageSize, minValue, maxValue)
			local rows = {}
			for k, v in pairs(store._data) do
				local num = tonumber(v)
				if num then
					if (not minValue or num >= minValue) and (not maxValue or num <= maxValue) then
						table.insert(rows, { key = k, value = num })
					end
				end
			end
			table.sort(rows, function(a, b)
				if ascending then
					return a.value < b.value
				end
				return a.value > b.value
			end)
			local limit = pageSize or 50
			local page = {}
			for i = 1, math.min(#rows, limit) do
				page[i] = rows[i]
			end
			return {
				IsFinished = true,
				GetCurrentPage = function()
					return page
				end,
				AdvanceToNextPageAsync = function() end,
			}
		end,
	}
	return api
end

-- Core Services
local Workspace = createInstance("Workspace", dataModel, "Workspace")
INST_STATE[Workspace].props.Gravity = 196.2
local CurrentCamera = createInstance("Camera", Workspace, "CurrentCamera")
INST_STATE[Workspace].props.CurrentCamera = CurrentCamera
INST_STATE[Workspace].customMethods = {
	GetPartsInPart = function()
		return {}
	end,
	GetPartBoundsInBox = function()
		return {}
	end,
	GetPartBoundsInRadius = function()
		return {}
	end,
	Raycast = function()
		return nil
	end,
}

local Players = createInstance("Players", dataModel, "Players")
INST_STATE[Players].PlayerAdded = newSignal("Players.PlayerAdded")
INST_STATE[Players].PlayerRemoving = newSignal("Players.PlayerRemoving")
INST_STATE[Players].signals.PlayerAdded = INST_STATE[Players].PlayerAdded
INST_STATE[Players].signals.PlayerRemoving = INST_STATE[Players].PlayerRemoving
INST_STATE[Players].props.RespawnTime = 5
INST_STATE[Players].customMethods = {
	GetPlayers = function(self)
		local list = {}
		for _, ch in ipairs(self:GetChildren()) do
			if INST_STATE[ch].className == "Player" then
				table.insert(list, ch)
			end
		end
		return list
	end,
	GetPlayerByUserId = function(self, uid)
		for _, ch in ipairs(self:GetChildren()) do
			if INST_STATE[ch].className == "Player" and INST_STATE[ch].props.UserId == uid then
				return ch
			end
		end
		return nil
	end,
	GetPlayerFromCharacter = function(self, char)
		for _, ch in ipairs(self:GetChildren()) do
			if INST_STATE[ch].className == "Player" and INST_STATE[ch].props.Character == char then
				return ch
			end
		end
		return nil
	end,
	GetUserThumbnailAsync = function()
		return "", true
	end,
}

local ReplicatedStorage = createInstance("ReplicatedStorage", dataModel, "ReplicatedStorage")
local ReplicatedFirst = createInstance("ReplicatedFirst", dataModel, "ReplicatedFirst")
local ServerScriptService = createInstance("ServerScriptService", dataModel, "ServerScriptService")
local ServerStorage = createInstance("ServerStorage", dataModel, "ServerStorage")
local StarterGui = createInstance("StarterGui", dataModel, "StarterGui")
INST_STATE[StarterGui].customMethods = {
	SetCore = function() end,
	SetCoreGuiEnabled = function() end,
	GetCore = function()
		return nil
	end,
}
local StarterPack = createInstance("StarterPack", dataModel, "StarterPack")
local StarterPlayer = createInstance("StarterPlayer", dataModel, "StarterPlayer")
local StarterPlayerScripts = createInstance("StarterPlayerScripts", StarterPlayer, "StarterPlayerScripts")
local StarterCharacterScripts = createInstance("StarterCharacterScripts", StarterPlayer, "StarterCharacterScripts")
local Lighting = createInstance("Lighting", dataModel, "Lighting")
INST_STATE[Lighting].props.ClockTime = 14
INST_STATE[Lighting].props.Brightness = 2
local SoundService = createInstance("SoundService", dataModel, "SoundService")
local Teams = createInstance("Teams", dataModel, "Teams")

local DataStoreService = createInstance("DataStoreService", dataModel, "DataStoreService")
INST_STATE[DataStoreService].customMethods = {
	GetDataStore = function(_, name, scope)
		return makeDataStore(name, scope)
	end,
	GetOrderedDataStore = function(_, name, scope)
		return makeOrderedDataStore(name, scope)
	end,
	GetGlobalDataStore = function()
		return makeDataStore("GlobalDataStore", "global")
	end,
}

local playerOwnedPasses = {} -- [userId] = { [passId] = true }
local MarketplaceService = createInstance("MarketplaceService", dataModel, "MarketplaceService")
INST_STATE[MarketplaceService].PromptProductPurchaseFinished = newSignal("MarketplaceService.PromptProductPurchaseFinished")
INST_STATE[MarketplaceService].PromptGamePassPurchaseFinished = newSignal("MarketplaceService.PromptGamePassPurchaseFinished")
INST_STATE[MarketplaceService].signals.PromptProductPurchaseFinished = INST_STATE[MarketplaceService].PromptProductPurchaseFinished
INST_STATE[MarketplaceService].signals.PromptGamePassPurchaseFinished = INST_STATE[MarketplaceService].PromptGamePassPurchaseFinished
INST_STATE[MarketplaceService].customMethods = {
	UserOwnsGamePassAsync = function(_, userId, passId)
		local passes = playerOwnedPasses[userId]
		return passes ~= nil and passes[passId] == true
	end,
	GetProductInfo = function(_, id, _infoType)
		return {
			Name = "Product_" .. tostring(id),
			Description = "",
			ProductId = id,
			AssetId = id,
			PriceInRobux = 0,
			IsForSale = true,
		}
	end,
	PromptProductPurchase = function(_, player, productId)
		-- In headless test, prompting records intent without blocking
		INST_STATE[MarketplaceService].PromptProductPurchaseFinished:Fire(player.UserId, productId, false)
	end,
	PromptGamePassPurchase = function(_, player, passId)
		local owns = playerOwnedPasses[player.UserId] and playerOwnedPasses[player.UserId][passId] == true
		INST_STATE[MarketplaceService].PromptGamePassPurchaseFinished:Fire(player, passId, owns)
	end,
}

local RunService = createInstance("RunService", dataModel, "RunService")
INST_STATE[RunService].Heartbeat = newSignal("RunService.Heartbeat")
INST_STATE[RunService].RenderStepped = newSignal("RunService.RenderStepped")
INST_STATE[RunService].Stepped = newSignal("RunService.Stepped")
INST_STATE[RunService].signals.Heartbeat = INST_STATE[RunService].Heartbeat
INST_STATE[RunService].signals.RenderStepped = INST_STATE[RunService].RenderStepped
INST_STATE[RunService].signals.Stepped = INST_STATE[RunService].Stepped
INST_STATE[RunService].customMethods = {
	IsServer = function()
		return true
	end,
	IsClient = function()
		return false
	end,
	IsStudio = function()
		return false
	end,
	IsRunning = function()
		return true
	end,
	BindToRenderStep = function() end,
	UnbindFromRenderStep = function() end,
}

local CollectionService = createInstance("CollectionService", dataModel, "CollectionService")
INST_STATE[CollectionService].customMethods = {
	AddTag = function(_, inst, tag)
		addTagInternal(inst, tag)
	end,
	RemoveTag = function(_, inst, tag)
		removeTagInternal(inst, tag)
	end,
	HasTag = function(_, inst, tag)
		local st = INST_STATE[inst]
		return st ~= nil and st.tags[tag] == true
	end,
	GetTags = function(_, inst)
		local st = INST_STATE[inst]
		if not st then
			return {}
		end
		return inst:GetTags()
	end,
	GetTagged = function(_, tag)
		return table.clone(TAG_TO_INSTANCES[tag] or {})
	end,
	GetInstanceAddedSignal = function(_, tag)
		if not TAG_ADDED_SIGNALS[tag] then
			TAG_ADDED_SIGNALS[tag] = newSignal("CollectionService.InstanceAdded(" .. tostring(tag) .. ")")
		end
		return TAG_ADDED_SIGNALS[tag]
	end,
	GetInstanceRemovedSignal = function(_, tag)
		if not TAG_REMOVED_SIGNALS[tag] then
			TAG_REMOVED_SIGNALS[tag] = newSignal("CollectionService.InstanceRemoved(" .. tostring(tag) .. ")")
		end
		return TAG_REMOVED_SIGNALS[tag]
	end,
}

local TweenService = createInstance("TweenService", dataModel, "TweenService")
INST_STATE[TweenService].customMethods = {
	Create = function(_, target, tweenInfo, props)
		local completed = newSignal("Tween.Completed")
		local tween = {
			Completed = completed,
			PlaybackState = nil,
			Play = function()
				if type(props) == "table" and isInstance(target) then
					for k, v in pairs(props) do
						pcall(function()
							target[k] = v
						end)
					end
				end
				task.defer(function()
					completed:Fire()
				end)
			end,
			Cancel = function() end,
			Pause = function() end,
			Destroy = function() end,
		}
		return tween
	end,
}

local guidSeq = 0
local HttpService = createInstance("HttpService", dataModel, "HttpService")
INST_STATE[HttpService].customMethods = {
	JSONEncode = function(_, val)
		return serde.encode("json", val)
	end,
	JSONDecode = function(_, str)
		return serde.decode("json", str)
	end,
	GenerateGUID = function(_, wrap)
		guidSeq += 1
		local raw = string.format("%08X-0000-4000-8000-%012X", guidSeq, guidSeq)
		if wrap == false then
			return raw
		end
		return "{" .. raw .. "}"
	end,
	UrlEncode = function(_, str)
		return tostring(str):gsub("([^%w%-_%.~])", function(c)
			return string.format("%%%02X", string.byte(c))
		end)
	end,
}

local Debris = createInstance("Debris", dataModel, "Debris")
INST_STATE[Debris].customMethods = {
	AddItem = function(_, item, lifetime)
		local delaySec = tonumber(lifetime) or 10
		if delaySec <= 0 then
			if isInstance(item) then
				item:Destroy()
			end
			return
		end
		wrappedTask.delay(math.min(delaySec, 0.5), function()
			if isInstance(item) and not INST_STATE[item].destroyed then
				item:Destroy()
			end
		end)
	end,
}

local UserInputService = createInstance("UserInputService", dataModel, "UserInputService")
INST_STATE[UserInputService].InputBegan = newSignal("UserInputService.InputBegan")
INST_STATE[UserInputService].InputEnded = newSignal("UserInputService.InputEnded")
INST_STATE[UserInputService].signals.InputBegan = INST_STATE[UserInputService].InputBegan
INST_STATE[UserInputService].signals.InputEnded = INST_STATE[UserInputService].InputEnded
INST_STATE[UserInputService].props.TouchEnabled = false
INST_STATE[UserInputService].props.KeyboardEnabled = true
INST_STATE[UserInputService].props.MouseEnabled = true
INST_STATE[UserInputService].customMethods = {
	IsKeyDown = function()
		return false
	end,
	GetFocusedTextBox = function()
		return nil
	end,
}

local ProximityPromptService = createInstance("ProximityPromptService", dataModel, "ProximityPromptService")
INST_STATE[ProximityPromptService].PromptTriggered = newSignal("ProximityPromptService.PromptTriggered")
INST_STATE[ProximityPromptService].signals.PromptTriggered = INST_STATE[ProximityPromptService].PromptTriggered

local AnalyticsService = createInstance("AnalyticsService", dataModel, "AnalyticsService")
INST_STATE[AnalyticsService].customMethods = {
	LogOnboardingFunnelStepEvent = function() end,
	LogFunnelStepEvent = function() end,
	LogEconomyEvent = function() end,
	LogCustomEvent = function() end,
}

local GuiService = createInstance("GuiService", dataModel, "GuiService")
INST_STATE[GuiService].customMethods = {
	GetGuiInset = function()
		return Vector2.new(0, 58), Vector2.new(0, 0)
	end,
}

local TextService = createInstance("TextService", dataModel, "TextService")
INST_STATE[TextService].customMethods = {
	GetTextSize = function(_, text, size, _font, frameSize)
		local len = #tostring(text or "")
		return Vector2.new(math.min(len * (size or 14) * 0.55, frameSize and frameSize.X or 1000), (size or 14) * 1.2)
	end,
}

local function registerService(svc)
	local name = INST_STATE[svc].props.Name
	services[name] = svc
	return svc
end

registerService(Workspace)
registerService(Players)
registerService(ReplicatedStorage)
registerService(ReplicatedFirst)
registerService(ServerScriptService)
registerService(ServerStorage)
registerService(StarterGui)
registerService(StarterPack)
registerService(StarterPlayer)
registerService(Lighting)
registerService(SoundService)
registerService(Teams)
registerService(DataStoreService)
registerService(MarketplaceService)
registerService(RunService)
registerService(CollectionService)
registerService(TweenService)
registerService(HttpService)
registerService(Debris)
registerService(UserInputService)
registerService(ProximityPromptService)
registerService(AnalyticsService)
registerService(GuiService)
registerService(TextService)

-- DataModel methods
INST_STATE[dataModel].Loaded = newSignal("DataModel.Loaded")
INST_STATE[dataModel].signals.Loaded = INST_STATE[dataModel].Loaded
INST_STATE[dataModel].customMethods = {
	GetService = function(_, name)
		if services[name] then
			return services[name]
		end
		local existing = dataModel:FindFirstChild(name)
		if existing then
			services[name] = existing
			return existing
		end
		local stub = createInstance(name, dataModel, name)
		services[name] = stub
		return stub
	end,
	FindService = function(_, name)
		return services[name]
	end,
	BindToClose = function(_, fn)
		if type(fn) == "function" then
			table.insert(bindToCloseCallbacks, fn)
		end
	end,
	IsLoaded = function()
		return isLoadedFlag
	end,
}

-- ---------------------------------------------------------------------------
-- Tree Materialization from JSON
-- ---------------------------------------------------------------------------
local function jsonToRobloxValue(val, propKey)
	if type(val) ~= "table" then
		return val
	end
	if val.Vector3 and type(val.Vector3) == "table" then
		return Vector3.new(val.Vector3[1], val.Vector3[2], val.Vector3[3])
	end
	if val.Color3 and type(val.Color3) == "table" then
		return Color3.new(val.Color3[1], val.Color3[2], val.Color3[3])
	end
	if val.Color3uint8 and type(val.Color3uint8) == "table" then
		return Color3.fromRGB(val.Color3uint8[1], val.Color3uint8[2], val.Color3uint8[3])
	end
	if val.CFrame and type(val.CFrame) == "table" then
		local pos = val.CFrame.position or { 0, 0, 0 }
		return CFrame.new(pos[1] or 0, pos[2] or 0, pos[3] or 0)
	end
	if val.UDim2 and type(val.UDim2) == "table" then
		return UDim2.new(val.UDim2[1] or 0, val.UDim2[2] or 0, val.UDim2[3] or 0, val.UDim2[4] or 0)
	end
	if propKey == "Position" or propKey == "Size" then
		if #val == 3 and type(val[1]) == "number" then
			return Vector3.new(val[1], val[2], val[3])
		end
	elseif propKey == "Color" then
		if #val == 3 and type(val[1]) == "number" then
			return Color3.new(val[1], val[2], val[3])
		end
	end
	return val
end

local function buildTreeNode(nodeData, parentInst)
	local name = nodeData.name or nodeData.class_name or "Instance"
	local className = nodeData.class_name or "Folder"
	local inst
	local existing = parentInst and parentInst:FindFirstChild(name)
	if existing and (parentInst == dataModel or parentInst == StarterPlayer) then
		inst = existing
	else
		inst = createInstance(className, parentInst, name)
	end
	local st = INST_STATE[inst]
	if nodeData.source ~= nil then
		st.props.Source = nodeData.source
	end
	if nodeData.source_file ~= nil then
		st.sourceFile = nodeData.source_file
	end
	if type(nodeData.props) == "table" then
		for k, v in pairs(nodeData.props) do
			st.props[k] = jsonToRobloxValue(v, k)
		end
	end
	if type(nodeData.children) == "table" then
		for _, chData in ipairs(nodeData.children) do
			buildTreeNode(chData, inst)
		end
	end
	return inst
end

if type(inputData.tree) == "table" and type(inputData.tree.services) == "table" then
	for _, svcNode in ipairs(inputData.tree.services) do
		local svcName = svcNode.name or svcNode.class_name
		local existingSvc = services[svcName]
		if existingSvc then
			if type(svcNode.props) == "table" then
				for k, v in pairs(svcNode.props) do
					INST_STATE[existingSvc].props[k] = jsonToRobloxValue(v, k)
				end
			end
			if type(svcNode.children) == "table" then
				for _, ch in ipairs(svcNode.children) do
					buildTreeNode(ch, existingSvc)
				end
			end
		else
			local created = buildTreeNode(svcNode, dataModel)
			services[svcName] = created
		end
	end
end

-- ---------------------------------------------------------------------------
-- Script Execution, Module Caching & Per-Context Environments
-- ---------------------------------------------------------------------------
local moduleCache = {}
local moduleLoading = {}
local sharedTable = {}
local globalTable = {}

-- Active client player lookup (uses threadPlayerContext declared above)

getActiveClientPlayer = function()
	local co = coroutine.running()
	if co and threadPlayerContext[co] then
		return threadPlayerContext[co]
	end
	local list = Players:GetPlayers()
	return list[1]
end

local function makeContextWrappers(isClient, localPlayer)
	if not isClient then
		return game or dataModel, Players, RunService
	end
	-- Client proxy for Players (adds LocalPlayer)
	local playersProxy = setmetatable({}, {
		__index = function(_, k)
			if k == "LocalPlayer" then
				return localPlayer
			end
			local v = Players[k]
			if type(v) == "function" then
				return function(_, ...)
					return v(Players, ...)
				end
			end
			return v
		end,
	})
	tagType(playersProxy, "Instance")

	-- Client proxy for RunService (IsClient=true, IsServer=false)
	local runProxy = setmetatable({}, {
		__index = function(_, k)
			if k == "IsClient" then
				return function()
					return true
				end
			elseif k == "IsServer" then
				return function()
					return false
				end
			end
			local v = RunService[k]
			if type(v) == "function" then
				return function(_, ...)
					return v(RunService, ...)
				end
			end
			return v
		end,
	})
	tagType(runProxy, "Instance")

	-- Client proxy for game:GetService("Players" | "RunService")
	local gameProxy = setmetatable({}, {
		__index = function(_, k)
			if k == "GetService" then
				return function(_, svcName)
					if svcName == "Players" then
						return playersProxy
					elseif svcName == "RunService" then
						return runProxy
					end
					return dataModel:GetService(svcName)
				end
			elseif k == "Players" then
				return playersProxy
			elseif k == "RunService" then
				return runProxy
			end
			local v = dataModel[k]
			if type(v) == "function" then
				return function(_, ...)
					return v(dataModel, ...)
				end
			end
			return v
		end,
	})
	tagType(gameProxy, "Instance")

	return gameProxy, playersProxy, runProxy
end

local buildScriptEnv -- forward declaration
local requireModule

requireModule = function(target, isClient, localPlayer)
	if type(target) == "string" then
		error("String require is not supported for Roblox ModuleScripts: " .. target)
	end
	if not isInstance(target) or INST_STATE[target].className ~= "ModuleScript" then
		error("Attempted to require a non-ModuleScript: " .. tostring(target))
	end
	local cacheKey = tostring(target) .. (if isClient then ":client:" .. tostring(localPlayer and localPlayer.UserId or 0) else ":server")
	if moduleCache[cacheKey] ~= nil then
		return moduleCache[cacheKey]
	end
	if moduleLoading[cacheKey] then
		error("Cyclic require detected for " .. target:GetFullName())
	end
	moduleLoading[cacheKey] = true
	local st = INST_STATE[target]
	local debugName = st.sourceFile or target:GetFullName()
	local env = buildScriptEnv(target, isClient, localPlayer)
	local compiled, loadErr = luau.load(st.props.Source or "", {
		debugName = debugName,
		environment = env,
	})
	if type(compiled) ~= "function" then
		moduleLoading[cacheKey] = nil
		error(tostring(loadErr))
	end
	local ok, ret = xpcall(compiled, function(err)
		return tostring(err) .. "\n" .. debug.traceback(nil, 2)
	end)
	moduleLoading[cacheKey] = nil
	if not ok then
		error(ret, 0)
	end
	moduleCache[cacheKey] = ret
	return ret
end

buildScriptEnv = function(scriptInst, isClient, localPlayer)
	local gameObj, _playersObj, _runObj = makeContextWrappers(isClient, localPlayer)

	local envTask = {
		wait = wrappedTask.wait,
		cancel = wrappedTask.cancel,
		synchronize = wrappedTask.synchronize,
		desynchronize = wrappedTask.desynchronize,
		spawn = function(fnOrThread, ...)
			if type(fnOrThread) == "thread" then
				return task.spawn(fnOrThread, ...)
			end
			local args = table.pack(...)
			return task.spawn(function()
				if isClient and localPlayer then
					threadPlayerContext[coroutine.running()] = localPlayer
				end
				safeCall(scriptInst and scriptInst:GetFullName() or "task.spawn", fnOrThread, table.unpack(args, 1, args.n))
			end)
		end,
		defer = function(fnOrThread, ...)
			if type(fnOrThread) == "thread" then
				return task.defer(fnOrThread, ...)
			end
			local args = table.pack(...)
			return task.defer(function()
				if isClient and localPlayer then
					threadPlayerContext[coroutine.running()] = localPlayer
				end
				safeCall(scriptInst and scriptInst:GetFullName() or "task.defer", fnOrThread, table.unpack(args, 1, args.n))
			end)
		end,
		delay = function(sec, fnOrThread, ...)
			if type(fnOrThread) == "thread" then
				return task.delay(sec, fnOrThread, ...)
			end
			local args = table.pack(...)
			return task.delay(sec, function()
				if isClient and localPlayer then
					threadPlayerContext[coroutine.running()] = localPlayer
				end
				safeCall(scriptInst and scriptInst:GetFullName() or "task.delay", fnOrThread, table.unpack(args, 1, args.n))
			end)
		end,
	}

	local env = {
		assert = assert,
		error = error,
		getmetatable = getmetatable,
		setmetatable = setmetatable,
		ipairs = ipairs,
		pairs = pairs,
		next = next,
		pcall = pcall,
		xpcall = xpcall,
		rawequal = rawequal,
		rawget = rawget,
		rawlen = rawlen,
		rawset = rawset,
		select = select,
		tonumber = tonumber,
		tostring = tostring,
		type = type,
		typeof = robloxTypeof,
		unpack = unpack or table.unpack,
		math = math,
		string = string,
		table = table,
		bit32 = bit32,
		utf8 = utf8,
		coroutine = coroutine,
		buffer = buffer,
		os = os,
		debug = debug,
		_VERSION = _VERSION,
		shared = sharedTable,
		_G = globalTable,
		game = gameObj,
		workspace = Workspace,
		script = scriptInst,
		Instance = InstanceAPI,
		Enum = Enum,
		Vector3 = Vector3,
		Vector2 = Vector2,
		CFrame = CFrame,
		UDim = UDim,
		UDim2 = UDim2,
		Color3 = Color3,
		ColorSequence = ColorSequence,
		ColorSequenceKeypoint = ColorSequenceKeypoint,
		NumberSequence = NumberSequence,
		NumberSequenceKeypoint = NumberSequenceKeypoint,
		NumberRange = NumberRange,
		Rect = Rect,
		BrickColor = BrickColor,
		TweenInfo = TweenInfo,
		Ray = Ray,
		RaycastParams = RaycastParams,
		OverlapParams = OverlapParams,
		Random = Random,
		DateTime = DateTimeShim,
		task = envTask,
		wait = envTask.wait,
		spawn = envTask.spawn,
		delay = envTask.delay,
		tick = function()
			return os.time() + (os.clock() % 1)
		end,
		time = os.clock,
		elapsedTime = os.clock,
		print = function(...)
			pushLine(formatArgs(...))
		end,
		warn = function(...)
			pushWarning(formatArgs(...))
		end,
		require = function(target)
			return requireModule(target, isClient, localPlayer)
		end,
	}
	return env
end

local function executeScriptInstance(scriptInst, isClient, localPlayer)
	local st = INST_STATE[scriptInst]
	if st.props.Disabled == true or st.props.Enabled == false then
		return true
	end
	local label = st.sourceFile or scriptInst:GetFullName()
	local env = buildScriptEnv(scriptInst, isClient, localPlayer)
	local okCompile, fnOrErr = pcall(luau.load, st.props.Source or "", {
		debugName = label,
		environment = env,
	})
	if not okCompile then
		pushError(label, fnOrErr)
		return false
	end
	task.spawn(function()
		if isClient and localPlayer then
			threadPlayerContext[coroutine.running()] = localPlayer
		end
		safeCall(label, fnOrErr)
	end)
	return true
end

local function collectScripts(root, className)
	local list = {}
	local function walk(node)
		local st = INST_STATE[node]
		if st.className == className then
			table.insert(list, node)
		end
		for _, ch in ipairs(st.children) do
			walk(ch)
		end
	end
	walk(root)
	table.sort(list, function(a, b)
		return a:GetFullName() < b:GetFullName()
	end)
	return list
end

local function tickFrames(frames, dt)
	local count = frames or 3
	local stepDt = dt or (1 / 60)
	for _ = 1, count do
		INST_STATE[RunService].Stepped:Fire(os.clock(), stepDt)
		INST_STATE[RunService].Heartbeat:Fire(stepDt)
		INST_STATE[RunService].RenderStepped:Fire(stepDt)
		task.wait(0.005)
	end
end

-- ---------------------------------------------------------------------------
-- Player Lifecycle & Journey Execution
-- ---------------------------------------------------------------------------
local activePlayersById = {}

local function spawnPlayer(spec)
	local uid = tonumber(spec.user_id) or 1001
	local name = tostring(spec.name or ("Player_" .. tostring(uid)))
	if type(spec.owns_passes) == "table" then
		local set = {}
		for _, pid in ipairs(spec.owns_passes) do
			local n = tonumber(pid)
			if n then
				set[n] = true
			end
		end
		playerOwnedPasses[uid] = set
	elseif not playerOwnedPasses[uid] then
		playerOwnedPasses[uid] = {}
	end

	local player = createInstance("Player", nil, name)
	local pst = INST_STATE[player]
	pst.props.UserId = uid
	pst.props.DisplayName = tostring(spec.display_name or name)
	pst.props.MembershipType = Enum.MembershipType.None
	pst.customMethods = {
		Kick = function(self, msg)
			pushWarning(string.format("Player %s kicked: %s", self.Name, tostring(msg or "")))
		end,
		IsInGroup = function()
			return false
		end,
		GetRankInGroup = function()
			return 0
		end,
	}

	local playerGui = createInstance("PlayerGui", player, "PlayerGui")
	local backpack = createInstance("Backpack", player, "Backpack")
	local playerScripts = createInstance("PlayerScripts", player, "PlayerScripts")

	for _, child in ipairs(StarterGui:GetChildren()) do
		local clone = child:Clone()
		clone.Parent = playerGui
	end
	for _, child in ipairs(StarterPack:GetChildren()) do
		local clone = child:Clone()
		clone.Parent = backpack
	end
	for _, child in ipairs(StarterPlayerScripts:GetChildren()) do
		local clone = child:Clone()
		clone.Parent = playerScripts
	end

	-- Character model in Workspace
	local char = createInstance("Model", Workspace, name)
	local hrp = createInstance("Part", char, "HumanoidRootPart")
	INST_STATE[hrp].props.Size = Vector3.new(2, 2, 1)
	local hum = createInstance("Humanoid", char, "Humanoid")
	INST_STATE[char].props.PrimaryPart = hrp
	pst.props.Character = char

	player.Parent = Players
	activePlayersById[uid] = player

	INST_STATE[Players].PlayerAdded:Fire(player)
	pst.CharacterAdded:Fire(char)
	tickFrames(2)

	-- Boot client LocalScripts for this player
	local clientScripts = {}
	for _, ls in ipairs(collectScripts(playerScripts, "LocalScript")) do
		table.insert(clientScripts, ls)
	end
	for _, ls in ipairs(collectScripts(playerGui, "LocalScript")) do
		table.insert(clientScripts, ls)
	end
	for _, ls in ipairs(clientScripts) do
		executeScriptInstance(ls, true, player)
	end
	tickFrames(4)
	return player
end

local function removePlayer(uid)
	local player = activePlayersById[uid]
	if not player then
		return nil
	end
	INST_STATE[Players].PlayerRemoving:Fire(player)
	tickFrames(3)
	if player.Character then
		player.Character:Destroy()
	end
	player:Destroy()
	activePlayersById[uid] = nil
	return player
end

local function triggerBindToClose()
	for idx, cb in ipairs(bindToCloseCallbacks) do
		safeCall("BindToClose#" .. tostring(idx), cb)
	end
	tickFrames(3)
end

local function snapshotLeaderstats()
	local out = {}
	for _, p in ipairs(Players:GetPlayers()) do
		local ls = p:FindFirstChild("leaderstats")
		if ls then
			local stats = {}
			for _, stat in ipairs(ls:GetChildren()) do
				local st = INST_STATE[stat]
				if st.props.Value ~= nil then
					stats[st.props.Name] = st.props.Value
				end
			end
			out[tostring(p.UserId)] = stats
		end
	end
	return out
end

local function snapshotDataStores()
	local out = {}
	for _, store in pairs(dataStores) do
		if next(store._data) ~= nil then
			out[store._name] = deepCopyValue(store._data)
		end
	end
	for _, store in pairs(orderedDataStores) do
		if next(store._data) ~= nil then
			out["ordered:" .. store._name] = deepCopyValue(store._data)
		end
	end
	return out
end

local function serializeGuiNode(inst)
	local st = INST_STATE[inst]
	local node = {
		name = st.props.Name,
		class_name = st.className,
	}
	if st.props.Visible ~= nil then
		node.visible = st.props.Visible == true
	end
	if st.props.Enabled ~= nil then
		node.enabled = st.props.Enabled == true
	end
	if type(st.props.Text) == "string" and st.props.Text ~= "" then
		node.text = st.props.Text
	end
	if st.props.Active ~= nil then
		node.active = st.props.Active == true
	end
	local children = {}
	for _, ch in ipairs(st.children) do
		table.insert(children, serializeGuiNode(ch))
	end
	node.children = children
	return node
end

local function snapshotPlayerGuiTree()
	local trees = {}
	for _, p in ipairs(Players:GetPlayers()) do
		local pg = p:FindFirstChild("PlayerGui")
		if pg then
			local children = {}
			for _, ch in ipairs(pg:GetChildren()) do
				table.insert(children, serializeGuiNode(ch))
			end
			table.insert(trees, {
				player_id = p.UserId,
				player_name = p.Name,
				children = children,
			})
		end
	end
	return trees
end

local function resolveInstancePath(rawPath, startInst)
	local clean = tostring(rawPath or "")
	clean = clean:gsub("^game%.", ""):gsub("^game/", "")
	local segs = {}
	for part in string.gmatch(clean, "[^%.%/]+") do
		table.insert(segs, part)
	end
	if #segs == 0 then
		return nil
	end
	local cur = startInst or dataModel
	if cur == dataModel then
		local first = segs[1]
		local svc = services[first] or dataModel:FindFirstChild(first)
		if svc then
			cur = svc
			table.remove(segs, 1)
		end
	end
	for _, seg in ipairs(segs) do
		cur = cur:FindFirstChild(seg)
		if not cur then
			return nil
		end
	end
	return cur
end

local function findGuiNode(player, guiPath)
	local pg = player and player:FindFirstChild("PlayerGui")
	if not pg then
		return nil
	end
	local clean = tostring(guiPath or "")
	clean = clean:gsub("^PlayerGui%.", ""):gsub("^PlayerGui/", "")
	local direct = resolveInstancePath(clean, pg)
	if direct then
		return direct
	end
	-- Fallback: match last segment recursively if caller passed short name
	local last = clean:match("([^%.%/]+)$")
	if last then
		return pg:FindFirstChild(last, true)
	end
	return nil
end

local function leaderstatsEqual(a, b)
	if type(a) ~= "table" or type(b) ~= "table" then
		return a == b
	end
	for k, v in pairs(a) do
		if b[k] ~= v then
			return false
		end
	end
	for k, v in pairs(b) do
		if a[k] ~= v then
			return false
		end
	end
	return true
end

local function deepEquals(a, b)
	if a == b then
		return true
	end
	if type(a) ~= "table" or type(b) ~= "table" then
		return false
	end
	for k, v in pairs(a) do
		if not deepEquals(v, b[k]) then
			return false
		end
	end
	for k in pairs(b) do
		if a[k] == nil then
			return false
		end
	end
	return true
end

local receiptsProcessed = {}
local receiptSeq = 0
local stepResults = {}
local assertionResults = {}

local function runPurchaseReceipt(playerId, productId, purchaseId, replayDuplicate)
	local cb = INST_STATE[MarketplaceService].props.ProcessReceipt
	if type(cb) ~= "function" then
		return false, nil, "MarketplaceService.ProcessReceipt is not registered"
	end
	receiptSeq += 1
	local pid = purchaseId or ("rcpt_" .. tostring(receiptSeq))
	local receiptInfo = {
		PurchaseId = pid,
		PlayerId = playerId,
		ProductId = productId,
		CurrencySpent = 0,
		CurrencyType = Enum.CurrencyType.Robux,
		PlaceIdWherePurchased = 1,
	}

	local lsBefore = snapshotLeaderstats()
	local dsBefore = serde.encode("json", snapshotDataStores())
	local ok1, decision1 = safeCall("ProcessReceipt#1", cb, receiptInfo)
	tickFrames(2)
	if not ok1 then
		return false, nil, tostring(decision1)
	end
	local granted1 = (decision1 == Enum.ProductPurchaseDecision.PurchaseGranted)
	local lsAfter1 = snapshotLeaderstats()
	local dsAfter1 = serde.encode("json", snapshotDataStores())

	table.insert(receiptsProcessed, {
		purchase_id = pid,
		product_id = productId,
		player_id = playerId,
		decision = tostring(decision1),
		duplicate_replay = false,
		granted = granted1,
	})

	if not granted1 then
		return false, { decision = tostring(decision1) }, "ProcessReceipt did not return Enum.ProductPurchaseDecision.PurchaseGranted"
	end

	if replayDuplicate then
		local ok2, decision2 = safeCall("ProcessReceipt#2(duplicate)", cb, receiptInfo)
		tickFrames(2)
		if not ok2 then
			return false, nil, tostring(decision2)
		end
		local granted2 = (decision2 == Enum.ProductPurchaseDecision.PurchaseGranted)
		local lsAfter2 = snapshotLeaderstats()
		local dsAfter2 = serde.encode("json", snapshotDataStores())

		local uidKey = tostring(playerId)
		local lsChanged1 = not leaderstatsEqual(lsBefore[uidKey] or {}, lsAfter1[uidKey] or {})
		local lsChanged2 = not leaderstatsEqual(lsAfter1[uidKey] or {}, lsAfter2[uidKey] or {})
		local dsChanged1 = (dsBefore ~= dsAfter1)
		local dsChanged2 = (dsAfter1 ~= dsAfter2)

		local idempotent = granted2 and (not (lsChanged1 and lsChanged2)) and (not (dsChanged1 and dsChanged2))
		table.insert(receiptsProcessed, {
			purchase_id = pid,
			product_id = productId,
			player_id = playerId,
			decision = tostring(decision2),
			duplicate_replay = true,
			idempotent = idempotent,
		})
		if not granted2 then
			return false, { decision = tostring(decision2), idempotent = false }, "Duplicate ProcessReceipt replay did not return PurchaseGranted"
		end
		if not idempotent then
			return false, { decision = tostring(decision2), idempotent = false }, "Duplicate PurchaseId was granted more than once (idempotency violation)"
		end
		return true, { decision = tostring(decision2), purchase_id = pid, idempotent = true }, nil
	end

	return true, { decision = tostring(decision1), purchase_id = pid }, nil
end

local function discoverProductIds()
	local ids = {}
	if type(inputData.product_ids) == "table" then
		for _, id in ipairs(inputData.product_ids) do
			local n = tonumber(id)
			if n then
				table.insert(ids, n)
			end
		end
	end
	-- Scan ReplicatedStorage ModuleScripts named Products or ProductIds
	for _, mod in ipairs(collectScripts(ReplicatedStorage, "ModuleScript")) do
		local name = INST_STATE[mod].props.Name
		if name == "Products" or name == "ProductIds" then
			local ok, tbl = pcall(function()
				return requireModule(mod, false, nil)
			end)
			if ok and type(tbl) == "table" then
				local function scanTable(t, depth)
					if depth > 4 then
						return
					end
					for k, v in pairs(t) do
						if type(v) == "number" and v > 0 then
							table.insert(ids, v)
						elseif type(v) == "table" then
							if type(v.robloxId) == "number" and v.robloxId > 0 then
								table.insert(ids, v.robloxId)
							elseif type(v.productId) == "number" and v.productId > 0 then
								table.insert(ids, v.productId)
							else
								scanTable(v, depth + 1)
							end
						end
					end
				end
				scanTable(tbl, 1)
			end
		end
	end
	return ids
end

local function executeStep(idx, step)
	local t0 = os.clock()
	local op = tostring(step.op or "")
	local ok = true
	local resVal = nil
	local errMsg = nil

	if op == "wait" then
		local ms = tonumber(step.ms) or ((tonumber(step.seconds) or 0.05) * 1000)
		local sec = math.clamp(ms / 1000, 0.001, 5)
		local frames = math.max(1, math.min(20, math.floor(sec * 30)))
		tickFrames(frames, sec / frames)
		resVal = { waited_ms = ms }
	elseif op == "fire_server" then
		local remotePath = tostring(step.remote or "")
		local remote = resolveInstancePath(remotePath) or ReplicatedStorage:FindFirstChild(remotePath, true)
		if not remote or not remote:IsA("RemoteEvent") then
			ok = false
			errMsg = "RemoteEvent not found: " .. remotePath
		else
			local pid = tonumber(step.player_id) or 1001
			local player = activePlayersById[pid] or Players:GetPlayers()[1]
			if not player then
				ok = false
				errMsg = "Player not found for fire_server: " .. tostring(pid)
			else
				local args = if type(step.args) == "table" then step.args else {}
				remotesFiredCount += 1
				INST_STATE[remote].OnServerEvent:Fire(player, table.unpack(args))
				tickFrames(3)
				resVal = { remote = remote:GetFullName(), player_id = player.UserId }
			end
		end
	elseif op == "invoke_server" then
		local remotePath = tostring(step.remote or "")
		local remote = resolveInstancePath(remotePath) or ReplicatedStorage:FindFirstChild(remotePath, true)
		if not remote or not remote:IsA("RemoteFunction") then
			ok = false
			errMsg = "RemoteFunction not found: " .. remotePath
		else
			local pid = tonumber(step.player_id) or 1001
			local player = activePlayersById[pid] or Players:GetPlayers()[1]
			if not player then
				ok = false
				errMsg = "Player not found for invoke_server: " .. tostring(pid)
			else
				local cb = INST_STATE[remote].props.OnServerInvoke
				if type(cb) ~= "function" then
					ok = false
					errMsg = "RemoteFunction has no OnServerInvoke: " .. remote:GetFullName()
				else
					local args = if type(step.args) == "table" then step.args else {}
					remotesFiredCount += 1
					local callOk, callRes = safeCall("InvokeServer:" .. remote:GetFullName(), cb, player, table.unpack(args))
					tickFrames(2)
					if not callOk then
						ok = false
						errMsg = tostring(callRes)
					else
						resVal = callRes
					end
				end
			end
		end
	elseif op == "purchase_receipt" then
		local pid = tonumber(step.player_id) or 1001
		local productId = tonumber(step.product_id) or 0
		local purchaseId = step.purchase_id and tostring(step.purchase_id) or nil
		local replayDup = step.replay_duplicate == true
		local prOk, prRes, prErr = runPurchaseReceipt(pid, productId, purchaseId, replayDup)
		ok = prOk
		resVal = prRes
		errMsg = prErr
	elseif op == "click_gui" then
		local pid = tonumber(step.player_id) or 1001
		local player = activePlayersById[pid] or Players:GetPlayers()[1]
		local guiPath = tostring(step.gui_path or "")
		local target = findGuiNode(player, guiPath)
		if not target then
			ok = false
			errMsg = "GUI node not found in PlayerGui: " .. guiPath
		else
			local st = INST_STATE[target]
			if st.MouseButton1Down then
				st.MouseButton1Down:Fire()
			end
			if st.MouseButton1Click then
				st.MouseButton1Click:Fire()
			end
			if st.Activated then
				st.Activated:Fire()
			end
			if st.MouseButton1Up then
				st.MouseButton1Up:Fire()
			end
			tickFrames(3)
			resVal = { gui_path = target:GetFullName(), class_name = st.className }
		end
	elseif op == "rejoin" then
		local pid = tonumber(step.player_id) or 1001
		local existing = activePlayersById[pid]
		local spec = {
			user_id = pid,
			name = existing and existing.Name or ("Player_" .. tostring(pid)),
		}
		if existing then
			removePlayer(pid)
		end
		triggerBindToClose()
		spawnPlayer(spec)
		resVal = { player_id = pid, rejoined = true }
	elseif op == "assert_luau" then
		local aName = tostring(step.name or ("assert_" .. tostring(idx)))
		local code = tostring(step.luau or "")
		local env = buildScriptEnv(nil, false, Players:GetPlayers()[1])
		local lsSnap = snapshotLeaderstats()
		local dsSnap = snapshotDataStores()
		local grantedReceipts = {}
		for _, r in ipairs(receiptsProcessed) do
			if not (r.duplicate_replay == true and r.idempotent == true) then
				table.insert(grantedReceipts, r)
			end
		end
		env.leaderstats = if next(lsSnap) ~= nil then lsSnap else nil
		env.datastores = if next(dsSnap) ~= nil then dsSnap else nil
		env.receipts_processed = grantedReceipts
		env.receipt_events = receiptsProcessed
		env.remotes_fired = remotesFiredCount
		env.state_snapshot = {
			leaderstats = lsSnap,
			datastores = dsSnap,
			receipts_processed = receiptsProcessed,
			remotes_fired = remotesFiredCount,
		}
		local okCompile, fnOrErr = pcall(luau.load, code, {
			debugName = "assert_luau:" .. aName,
			environment = env,
		})
		if not okCompile then
			ok = false
			errMsg = tostring(fnOrErr)
			pushError("assert_luau:" .. aName, errMsg)
		else
			local runOk, val = safeCall("assert_luau:" .. aName, fnOrErr)
			if not runOk then
				ok = false
				errMsg = tostring(val)
			else
				resVal = val
				local exp = step.expect
				if type(exp) == "table" then
					if exp.equals ~= nil and not deepEquals(val, exp.equals) then
						ok = false
						errMsg = string.format("expected %s, got %s", tostring(exp.equals), tostring(val))
					elseif exp.truthy == true and not val then
						ok = false
						errMsg = "expected truthy value, got " .. tostring(val)
					elseif exp.contains ~= nil and not string.find(tostring(val), tostring(exp.contains), 1, true) then
						ok = false
						errMsg = string.format("expected '%s' to contain '%s'", tostring(val), tostring(exp.contains))
					end
				elseif not val then
					ok = false
					errMsg = "assertion returned falsy: " .. tostring(val)
				end
			end
		end
		table.insert(assertionResults, {
			name = aName,
			passed = ok,
			value = resVal,
			error = errMsg,
		})
	else
		ok = false
		errMsg = "Unknown journey op: " .. op
	end

	table.insert(stepResults, {
		index = idx,
		op = op,
		passed = ok,
		duration_ms = math.max(0, math.round((os.clock() - t0) * 1000)),
		result = resVal,
		error = errMsg,
	})
	return ok
end

-- ---------------------------------------------------------------------------
-- Main Execution Flow
-- ---------------------------------------------------------------------------
local runStart = os.clock()

-- 1. Boot Server Scripts (ServerScriptService + Workspace enabled Scripts)
local serverScripts = collectScripts(ServerScriptService, "Script")
for _, wsScript in ipairs(collectScripts(Workspace, "Script")) do
	table.insert(serverScripts, wsScript)
end
for _, s in ipairs(serverScripts) do
	executeScriptInstance(s, false, nil)
end
tickFrames(5)

-- 2. Spawn Initial Players & Client Scripts
local playerSpecs = inputData.players
if type(playerSpecs) ~= "table" or #playerSpecs == 0 then
	playerSpecs = { { user_id = 1001, name = "TestPlayer" } }
end
for _, spec in ipairs(playerSpecs) do
	spawnPlayer(spec)
end

-- 3. Execute explicit journey OR default auto-journey
local isAutoJourney = (inputData.auto_journey == true) or (type(inputData.journey) ~= "table")

if isAutoJourney then
	local primaryPid = tonumber(playerSpecs[1].user_id) or 1001

	-- Step A: ProcessReceipt idempotency check if ProcessReceipt is registered
	if type(INST_STATE[MarketplaceService].props.ProcessReceipt) == "function" then
		local candidates = discoverProductIds()
		if #candidates == 0 then
			table.insert(candidates, 1001)
		end
		local chosenId = candidates[1]
		-- Probe first candidate or try candidates until one grants
		local t0 = os.clock()
		local prOk, prRes, prErr = runPurchaseReceipt(primaryPid, chosenId, "auto_rcpt_1", true)
		table.insert(stepResults, {
			index = #stepResults,
			op = "purchase_receipt",
			passed = prOk,
			duration_ms = math.max(0, math.round((os.clock() - t0) * 1000)),
			result = prRes,
			error = prErr,
		})
		table.insert(assertionResults, {
			name = "process_receipt_idempotency",
			passed = prOk,
			value = prRes,
			error = prErr,
		})
	end

	-- Step B: Snapshot state before leave -> PlayerRemoving -> BindToClose -> Rejoin
	local lsBeforeLeave = snapshotLeaderstats()
	local dsBeforeLeave = snapshotDataStores()

	local tLeave = os.clock()
	removePlayer(primaryPid)
	triggerBindToClose()
	table.insert(stepResults, {
		index = #stepResults,
		op = "bind_to_close",
		passed = true,
		duration_ms = math.max(0, math.round((os.clock() - tLeave) * 1000)),
		result = { callbacks = #bindToCloseCallbacks },
		error = nil,
	})

	local tRejoin = os.clock()
	spawnPlayer(playerSpecs[1])
	table.insert(stepResults, {
		index = #stepResults,
		op = "rejoin",
		passed = true,
		duration_ms = math.max(0, math.round((os.clock() - tRejoin) * 1000)),
		result = { player_id = primaryPid, rejoined = true },
		error = nil,
	})

	local lsAfterRejoin = snapshotLeaderstats()
	local dsAfterRejoin = snapshotDataStores()
	local hadLeaderstats = lsBeforeLeave[tostring(primaryPid)] ~= nil and next(lsBeforeLeave[tostring(primaryPid)]) ~= nil
	local hasLeaderstatsAfter = lsAfterRejoin[tostring(primaryPid)] ~= nil and next(lsAfterRejoin[tostring(primaryPid)]) ~= nil
	local hadDataStores = next(dsBeforeLeave) ~= nil
	local hasDataStoresAfter = next(dsAfterRejoin) ~= nil

	local persistPassed = true
	local persistErr = nil
	if hadLeaderstats and not hasLeaderstatsAfter then
		persistPassed = false
		persistErr = "leaderstats were not restored after rejoin"
	elseif hadLeaderstats and not leaderstatsEqual(lsBeforeLeave[tostring(primaryPid)], lsAfterRejoin[tostring(primaryPid)]) then
		persistPassed = false
		persistErr = "leaderstats changed or reset across rejoin"
	elseif hadDataStores and not hasDataStoresAfter then
		persistPassed = false
		persistErr = "DataStore state lost across rejoin"
	end

	table.insert(assertionResults, {
		name = "rejoin_persistence",
		passed = persistPassed,
		value = {
			had_leaderstats = hadLeaderstats,
			had_datastores = hadDataStores,
		},
		error = persistErr,
	})
else
	for idx, step in ipairs(inputData.journey) do
		executeStep(idx - 1, step)
	end
end

-- 4. Build final structured report
local guiTrees = snapshotPlayerGuiTree()
local hasGui = false
for _, pt in ipairs(guiTrees) do
	if type(pt.children) == "table" and #pt.children > 0 then
		hasGui = true
		break
	end
end

local allStepsPassed = true
for _, s in ipairs(stepResults) do
	if not s.passed then
		allStepsPassed = false
		break
	end
end
local allAssertionsPassed = true
for _, a in ipairs(assertionResults) do
	if not a.passed then
		allAssertionsPassed = false
		break
	end
end

local passed = (#consoleErrors == 0) and allStepsPassed and allAssertionsPassed

local actTested = (remotesFiredCount > 0) or (#receiptsProcessed > 0) or (#stepResults > 0)
local hudTested = hasGui or (#collectScripts(StarterGui, "LocalScript") > 0) or (#StarterGui:GetChildren() > 0)

local report = {
	ok = true,
	engine = "lune_roblox_shim",
	passed = passed,
	duration_ms = math.max(1, math.round((os.clock() - runStart) * 1000)),
	steps = stepResults,
	assertions = assertionResults,
	console = {
		lines = consoleLines,
		errors = consoleErrors,
		warnings = consoleWarnings,
	},
	state_snapshot = {
		leaderstats = snapshotLeaderstats(),
		datastores = snapshotDataStores(),
		receipts_processed = receiptsProcessed,
		remotes_fired = remotesFiredCount,
		player_gui_tree = guiTrees,
	},
	core_loop = {
		move = {
			status = "not tested (headless sandbox)",
			tested = false,
			passed = false,
		},
		act = {
			status = if not actTested
				then "not tested"
				elseif passed then "passed" else "failed",
			tested = actTested,
			passed = actTested and allStepsPassed and (#consoleErrors == 0),
		},
		hud = {
			status = if not hudTested
				then "not tested"
				elseif #consoleErrors == 0 then "rendered" else "errors during render",
			tested = hudTested,
			passed = hudTested and (#consoleErrors == 0),
		},
	},
}

fs.writeFile(outputPath, serde.encode("json", report))
process.exit(0)
