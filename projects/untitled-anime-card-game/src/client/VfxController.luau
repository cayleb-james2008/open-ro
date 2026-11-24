--!strict

local CollectionService = game:GetService("CollectionService")
local Lighting = game:GetService("Lighting")
local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local TweenService = game:GetService("TweenService")
local Workspace = game:GetService("Workspace")

local Theme = require(script.Parent:WaitForChild("Theme"))

local VfxController = {}

local started = false
local renderConn: RBXScriptConnection? = nil
local orbitParts: { { part: BasePart, baseCFrame: CFrame, speed: number, axis: Vector3, isCrystal: boolean } } = {}
local trackedInstances: { [Instance]: boolean } = {}

local weatherHost: Part?
local weatherEmitter: ParticleEmitter?
local elapsed = 0

type WeatherVisual = {
	color: Color3,
	tint: Color3,
	brightness: number,
	rate: number,
	speed: NumberRange,
	size: NumberSequence,
}

local WEATHER_VISUALS: { [string]: WeatherVisual } = {
	clear = {
		color = Color3.fromRGB(255, 224, 150),
		tint = Color3.fromRGB(255, 248, 236),
		brightness = 0.02,
		rate = 10,
		speed = NumberRange.new(1.5, 3.5),
		size = NumberSequence.new(0.22, 0.06),
	},
	golden = {
		color = Color3.fromRGB(255, 206, 72),
		tint = Color3.fromRGB(255, 236, 186),
		brightness = 0.08,
		rate = 28,
		speed = NumberRange.new(4, 8),
		size = NumberSequence.new(0.32, 0.08),
	},
	crimson = {
		color = Color3.fromRGB(245, 76, 94),
		tint = Color3.fromRGB(255, 206, 210),
		brightness = -0.04,
		rate = 26,
		speed = NumberRange.new(3, 6.5),
		size = NumberSequence.new(0.3, 0.08),
	},
	storm = {
		color = Color3.fromRGB(82, 224, 255),
		tint = Color3.fromRGB(214, 238, 255),
		brightness = 0.0,
		rate = 32,
		speed = NumberRange.new(6, 11),
		size = NumberSequence.new(0.26, 0.06),
	},
	sakura = {
		color = Color3.fromRGB(255, 162, 214),
		tint = Color3.fromRGB(255, 232, 244),
		brightness = 0.05,
		rate = 24,
		speed = NumberRange.new(2, 4.5),
		size = NumberSequence.new(0.34, 0.12),
	},
}

local function registerOrbitCandidate(inst: Instance)
	if trackedInstances[inst] or not inst:IsA("BasePart") then
		return
	end
	local isRing = inst:GetAttribute("AltarOrbitRing") == true or CollectionService:HasTag(inst, "AltarOrbitRing")
	local isCrystal = inst.Name == "RollAltarCrystal"
	if not isRing and not isCrystal then
		return
	end
	trackedInstances[inst] = true
	local idx = #orbitParts + 1
	local axis = if isCrystal
		then Vector3.new(0, 1, 0)
		elseif idx % 2 == 0 then Vector3.new(0.35, 1, 0.25).Unit
		else Vector3.new(-0.25, 1, 0.4).Unit
	table.insert(orbitParts, {
		part = inst,
		baseCFrame = inst.CFrame,
		speed = if isCrystal then 0.9 elseif idx % 2 == 0 then 1.35 else -1.1,
		axis = axis,
		isCrystal = isCrystal,
	})
end

local function findAltarPosition(): Vector3
	for _, entry in ipairs(orbitParts) do
		if entry.part.Parent then
			return entry.part.Position
		end
	end
	local ws: Instance = Workspace
	local crystal = ws:FindFirstChild("RollAltarCrystal", true)
	if crystal and crystal:IsA("BasePart") then
		return crystal.Position
	end
	local altar = ws:FindFirstChild("RollAltar", true)
	if altar and altar:IsA("BasePart") then
		return altar.Position + Vector3.new(0, 4, 0)
	elseif altar and altar:IsA("Model") then
		local primary = altar.PrimaryPart or altar:FindFirstChildWhichIsA("BasePart")
		if primary then
			return primary.Position + Vector3.new(0, 4, 0)
		end
	end
	local localPlayer = Players.LocalPlayer
	if localPlayer and localPlayer.Character then
		local hrp = localPlayer.Character:FindFirstChild("HumanoidRootPart")
		if hrp and hrp:IsA("BasePart") then
			return hrp.Position + Vector3.new(0, 2, 0)
		end
	end
	return Vector3.new(0, 6, 0)
end

local function ensureWeatherHost(): (Part, ParticleEmitter)
	if weatherHost and weatherHost.Parent and weatherEmitter and weatherEmitter.Parent then
		return weatherHost, weatherEmitter
	end
	local host = Instance.new("Part")
	host.Name = "UACGWeatherParticleHost"
	host.Anchored = true
	host.CanCollide = false
	host.CanQuery = false
	host.CanTouch = false
	host.Transparency = 1
	host.Size = Vector3.new(44, 2, 44)
	host.CFrame = CFrame.new(0, 24, 0)
	host.Parent = Workspace

	local emitter = Instance.new("ParticleEmitter")
	emitter.Name = "WeatherEmitter"
	emitter.Rate = 12
	emitter.Lifetime = NumberRange.new(3.5, 5.5)
	emitter.Speed = NumberRange.new(1.5, 3.5)
	emitter.SpreadAngle = Vector2.new(30, 30)
	emitter.EmissionDirection = Enum.NormalId.Bottom
	emitter.LightEmission = 0.65
	emitter.Size = NumberSequence.new(0.24, 0.08)
	emitter.Transparency = NumberSequence.new({
		NumberSequenceKeypoint.new(0, 1),
		NumberSequenceKeypoint.new(0.2, 0.25),
		NumberSequenceKeypoint.new(0.85, 0.45),
		NumberSequenceKeypoint.new(1, 1),
	})
	emitter.Parent = host

	weatherHost = host
	weatherEmitter = emitter
	return host, emitter
end

function VfxController.start()
	if started then
		return
	end
	started = true

	for _, inst in ipairs(Workspace:GetDescendants()) do
		registerOrbitCandidate(inst)
	end
	for _, inst in ipairs(CollectionService:GetTagged("AltarOrbitRing")) do
		registerOrbitCandidate(inst)
	end

	local ws: Instance = Workspace
	ws.DescendantAdded:Connect(function(inst: Instance)
		registerOrbitCandidate(inst)
	end)

	renderConn = RunService.RenderStepped:Connect(function(dt: number)
		elapsed += dt
		for i = #orbitParts, 1, -1 do
			local entry = orbitParts[i]
			if not entry.part.Parent then
				trackedInstances[entry.part] = nil
				table.remove(orbitParts, i)
			else
				local bob = if entry.isCrystal then math.sin(elapsed * 2.1) * 0.45 else 0
				local angle = elapsed * entry.speed
				local rot = CFrame.fromAxisAngle(entry.axis, angle)
				entry.part.CFrame = (entry.baseCFrame + Vector3.new(0, bob, 0)) * rot
			end
		end

		if weatherHost and weatherHost.Parent then
			local cam = Workspace.CurrentCamera
			if cam then
				weatherHost.CFrame = CFrame.new(cam.CFrame.Position + Vector3.new(0, 18, 0))
			end
		end
	end)
end

function VfxController.playAltarSummon(rarity: string, reducedMotion: boolean?, reducedFlash: boolean?)
	local style = Theme.getRarity(rarity)
	local isHighTier = style.order >= 6
	local origin = findAltarPosition()
	local beamHeight = if isHighTier then 46 else 28
	local beamRadius = if isHighTier then 4.2 else 2.6
	local duration = if isHighTier then 1.35 else 1.0
	local baseTrans = if reducedFlash then 0.55 else 0.18

	local beam = Instance.new("Part")
	beam.Name = "SummonSkyBeam"
	beam.Shape = Enum.PartType.Cylinder
	beam.Material = Enum.Material.Neon
	beam.Color = style.color
	beam.Anchored = true
	beam.CanCollide = false
	beam.CanQuery = false
	beam.CanTouch = false
	beam.CastShadow = false
	beam.Transparency = baseTrans
	beam.Size = Vector3.new(4, beamRadius * 2, beamRadius * 2)
	beam.CFrame = CFrame.new(origin + Vector3.new(0, 2, 0)) * CFrame.Angles(0, 0, math.rad(90))
	beam.Parent = Workspace

	local beamTween =
		TweenService:Create(beam, TweenInfo.new(duration, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Size = Vector3.new(beamHeight, 0.4, 0.4),
			CFrame = CFrame.new(origin + Vector3.new(0, beamHeight * 0.5, 0)) * CFrame.Angles(0, 0, math.rad(90)),
			Transparency = 1,
		})
	beamTween:Play()

	local function spawnShockwave(delaySec: number, maxDiam: number, color: Color3)
		task.delay(delaySec, function()
			local ring = Instance.new("Part")
			ring.Name = "SummonShockwave"
			ring.Shape = Enum.PartType.Cylinder
			ring.Material = Enum.Material.Neon
			ring.Color = color
			ring.Anchored = true
			ring.CanCollide = false
			ring.CanQuery = false
			ring.CanTouch = false
			ring.CastShadow = false
			ring.Transparency = baseTrans
			ring.Size = Vector3.new(0.35, 3, 3)
			ring.CFrame = CFrame.new(origin - Vector3.new(0, 1.5, 0)) * CFrame.Angles(0, 0, math.rad(90))
			ring.Parent = Workspace
			TweenService:Create(ring, TweenInfo.new(0.75, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
				Size = Vector3.new(0.2, maxDiam, maxDiam),
				Transparency = 1,
			}):Play()
			task.delay(0.85, function()
				if ring.Parent then
					ring:Destroy()
				end
			end)
		end)
	end

	spawnShockwave(0, if isHighTier then 26 else 16, style.glow)
	if isHighTier then
		spawnShockwave(0.14, 34, style.color)
	end

	local burstHost = Instance.new("Part")
	burstHost.Name = "SummonBurstHost"
	burstHost.Anchored = true
	burstHost.CanCollide = false
	burstHost.CanQuery = false
	burstHost.CanTouch = false
	burstHost.Transparency = 1
	burstHost.Size = Vector3.new(2, 2, 2)
	burstHost.CFrame = CFrame.new(origin)
	burstHost.Parent = Workspace

	local emitter = Instance.new("ParticleEmitter")
	emitter.Color = ColorSequence.new(style.color, style.glow)
	emitter.LightEmission = if reducedFlash then 0.35 else 0.85
	emitter.Speed = NumberRange.new(12, if isHighTier then 28 else 20)
	emitter.SpreadAngle = Vector2.new(180, 180)
	emitter.Lifetime = NumberRange.new(0.6, 1.15)
	emitter.Size = NumberSequence.new(0.45, 0.05)
	emitter.Rate = 0
	emitter.Parent = burstHost
	emitter:Emit(if isHighTier then 42 else 24)

	if isHighTier and not reducedMotion then
		local cam = Workspace.CurrentCamera
		if cam then
			local baseFov = cam.FieldOfView
			local punch = TweenService:Create(
				cam,
				TweenInfo.new(0.14, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
				{ FieldOfView = math.clamp(baseFov - 4, 55, 85) }
			)
			punch:Play()
			task.delay(0.16, function()
				if cam.Parent then
					TweenService:Create(
						cam,
						TweenInfo.new(0.28, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
						{ FieldOfView = baseFov }
					):Play()
				end
			end)
		end
	end

	task.delay(duration + 0.25, function()
		if beam.Parent then
			beam:Destroy()
		end
		if burstHost.Parent then
			burstHost:Destroy()
		end
	end)
end

function VfxController.setWeather(weatherId: string, weatherName: string)
	local key = string.lower(weatherId .. " " .. weatherName)
	local visual = WEATHER_VISUALS.clear
	if string.find(key, "gold", 1, true) or string.find(key, "meteor", 1, true) then
		visual = WEATHER_VISUALS.golden
	elseif
		string.find(key, "crimson", 1, true)
		or string.find(key, "blood", 1, true)
		or string.find(key, "eclipse", 1, true)
	then
		visual = WEATHER_VISUALS.crimson
	elseif
		string.find(key, "storm", 1, true)
		or string.find(key, "surge", 1, true)
		or string.find(key, "chakra", 1, true)
	then
		visual = WEATHER_VISUALS.storm
	elseif
		string.find(key, "sakura", 1, true)
		or string.find(key, "starfall", 1, true)
		or string.find(key, "spirit", 1, true)
	then
		visual = WEATHER_VISUALS.sakura
	end

	local _, emitter = ensureWeatherHost()
	emitter.Color = ColorSequence.new(visual.color)
	emitter.Rate = visual.rate
	emitter.Speed = visual.speed
	emitter.Size = visual.size

	local lightingInst: Instance = Lighting
	local cc = lightingInst:FindFirstChild("UACGColorCorrection")
	if cc and cc:IsA("ColorCorrectionEffect") then
		TweenService:Create(cc, TweenInfo.new(0.8, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			TintColor = visual.tint,
			Brightness = visual.brightness,
		}):Play()
	end
end

function VfxController.springOpen(panel: GuiObject, reducedMotion: boolean?)
	local scale = panel:FindFirstChild("VfxSpringScale")
	local uiScale: UIScale
	if scale and scale:IsA("UIScale") then
		uiScale = scale
	else
		uiScale = Instance.new("UIScale")
		uiScale.Name = "VfxSpringScale"
		uiScale.Parent = panel
	end

	if reducedMotion then
		uiScale.Scale = 1
		return
	end

	uiScale.Scale = 0.90
	TweenService:Create(uiScale, TweenInfo.new(0.22, Enum.EasingStyle.Back, Enum.EasingDirection.Out), { Scale = 1 })
		:Play()
end

function VfxController.stop()
	if renderConn then
		renderConn:Disconnect()
		renderConn = nil
	end
	started = false
end

return VfxController
