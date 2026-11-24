--!strict
local Lighting = game:GetService("Lighting")
local Workspace = game:GetService("Workspace")

export type BuildResult = {
	hub: Model,
	prompts: { ProximityPrompt },
}

local WorldBuild = {}

local PALETTE = {
	Meadow = Color3.fromRGB(76, 152, 92),
	MeadowBorder = Color3.fromRGB(58, 126, 74),
	Marble = Color3.fromRGB(244, 241, 234),
	MarbleWarm = Color3.fromRGB(228, 222, 210),
	SlateTrim = Color3.fromRGB(54, 62, 82),
	Gold = Color3.fromRGB(238, 184, 68),
	GoldGlow = Color3.fromRGB(255, 214, 102),
	Crimson = Color3.fromRGB(184, 42, 52),
	CrimsonDark = Color3.fromRGB(128, 26, 36),
	PortalCyan = Color3.fromRGB(82, 222, 255),
	LanternGlow = Color3.fromRGB(255, 206, 124),
	Stone = Color3.fromRGB(168, 172, 180),
	StoneDark = Color3.fromRGB(108, 114, 126),
	Wood = Color3.fromRGB(104, 68, 46),
	SakuraLight = Color3.fromRGB(255, 194, 214),
	SakuraMid = Color3.fromRGB(244, 154, 184),
	EmeraldLeaf = Color3.fromRGB(64, 162, 94),
	EmeraldBright = Color3.fromRGB(92, 192, 118),
	CrystalBlue = Color3.fromRGB(118, 208, 255),
}

local function findChildByName(parent: Instance, targetName: string): Instance?
	for _, child in ipairs(parent:GetChildren()) do
		if child.Name == targetName then
			return child
		end
	end
	return nil
end

local function makePart(
	parent: Instance,
	name: string,
	size: Vector3,
	cf: CFrame,
	color: Color3,
	material: Enum.Material?,
	transparency: number?
): Part
	local part = Instance.new("Part")
	part.Name = name
	part.Size = size
	part.CFrame = cf
	part.Color = color
	part.Material = material or Enum.Material.SmoothPlastic
	part.Transparency = transparency or 0
	part.Anchored = true
	part.CanCollide = true
	part.TopSurface = Enum.SurfaceType.Smooth
	part.BottomSurface = Enum.SurfaceType.Smooth
	part.Parent = parent
	return part
end

local function makeUprightCylinder(
	parent: Instance,
	name: string,
	pos: Vector3,
	height: number,
	diameter: number,
	color: Color3,
	material: Enum.Material?,
	transparency: number?
): Part
	local part = Instance.new("Part")
	part.Name = name
	part.Shape = Enum.PartType.Cylinder
	part.Size = Vector3.new(height, diameter, diameter)
	part.CFrame = CFrame.new(pos) * CFrame.Angles(0, 0, math.rad(90))
	part.Color = color
	part.Material = material or Enum.Material.SmoothPlastic
	part.Transparency = transparency or 0
	part.Anchored = true
	part.CanCollide = true
	part.TopSurface = Enum.SurfaceType.Smooth
	part.BottomSurface = Enum.SurfaceType.Smooth
	part.Parent = parent
	return part
end

local function makeBall(
	parent: Instance,
	name: string,
	pos: Vector3,
	diameter: number,
	color: Color3,
	material: Enum.Material?,
	transparency: number?
): Part
	local part = Instance.new("Part")
	part.Name = name
	part.Shape = Enum.PartType.Ball
	part.Size = Vector3.new(diameter, diameter, diameter)
	part.CFrame = CFrame.new(pos)
	part.Color = color
	part.Material = material or Enum.Material.SmoothPlastic
	part.Transparency = transparency or 0
	part.Anchored = true
	part.CanCollide = true
	part.TopSurface = Enum.SurfaceType.Smooth
	part.BottomSurface = Enum.SurfaceType.Smooth
	part.Parent = parent
	return part
end

local function attachPrompt(
	parentPart: BasePart,
	propAction: string,
	objectText: string,
	actionText: string,
	prompts: { ProximityPrompt }
): ProximityPrompt
	local prompt = Instance.new("ProximityPrompt")
	prompt.Name = propAction .. "Prompt"
	prompt.ObjectText = objectText
	prompt.ActionText = actionText
	prompt.KeyboardKeyCode = Enum.KeyCode.E
	prompt.HoldDuration = 0
	prompt.MaxActivationDistance = 14
	prompt.RequiresLineOfSight = false
	prompt:SetAttribute("PropAction", propAction)
	prompt:SetAttribute("OpenProp", propAction)
	prompt.Parent = parentPart
	table.insert(prompts, prompt)
	return prompt
end

local function attachSign(parentPart: BasePart, title: string, subtitle: string, accent: Color3, offset: Vector3?)
	local gui = Instance.new("BillboardGui")
	gui.Name = "TitleSign"
	gui.Size = UDim2.fromScale(7.2, 2.1)
	gui.StudsOffset = offset or Vector3.new(0, 6, 0)
	gui.MaxDistance = 50
	gui.AlwaysOnTop = false
	gui.Parent = parentPart

	local frame = Instance.new("Frame")
	frame.Size = UDim2.fromScale(1, 1)
	frame.BackgroundColor3 = Color3.fromRGB(20, 24, 34)
	frame.BackgroundTransparency = 0.18
	frame.BorderSizePixel = 0
	frame.Parent = gui

	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0, 10)
	corner.Parent = frame

	local stroke = Instance.new("UIStroke")
	stroke.Color = accent
	stroke.Thickness = 2
	stroke.Parent = frame

	local titleLbl = Instance.new("TextLabel")
	titleLbl.Size = UDim2.fromScale(0.92, 0.52)
	titleLbl.Position = UDim2.fromScale(0.04, 0.06)
	titleLbl.BackgroundTransparency = 1
	titleLbl.Font = Enum.Font.GothamBold
	titleLbl.Text = title
	titleLbl.TextColor3 = Color3.fromRGB(255, 255, 255)
	titleLbl.TextScaled = true
	titleLbl.Parent = frame

	local subLbl = Instance.new("TextLabel")
	subLbl.Size = UDim2.fromScale(0.92, 0.36)
	subLbl.Position = UDim2.fromScale(0.04, 0.58)
	subLbl.BackgroundTransparency = 1
	subLbl.Font = Enum.Font.GothamMedium
	subLbl.Text = subtitle
	subLbl.TextColor3 = accent
	subLbl.TextScaled = true
	subLbl.Parent = frame
end

local function setupLighting()
	Lighting.ClockTime = 15.8
	Lighting.GeographicLatitude = 32
	Lighting.Brightness = 2.6
	Lighting.Ambient = Color3.fromRGB(132, 146, 174)
	Lighting.OutdoorAmbient = Color3.fromRGB(148, 164, 192)
	Lighting.ColorShift_Top = Color3.fromRGB(255, 240, 214)
	Lighting.ColorShift_Bottom = Color3.fromRGB(118, 134, 166)
	Lighting.EnvironmentDiffuseScale = 0.85
	Lighting.EnvironmentSpecularScale = 0.9
	Lighting.GlobalShadows = true

	local managedNames = {
		"NexusSky",
		"NexusAtmosphere",
		"NexusBloom",
		"NexusSunRays",
		"NexusColorCorrection",
	}
	for _, itemName in ipairs(managedNames) do
		local existing = findChildByName(Lighting, itemName)
		if existing then
			existing:Destroy()
		end
	end

	local sky = Instance.new("Sky")
	sky.Name = "NexusSky"
	sky.SunAngularSize = 18
	sky.MoonAngularSize = 8
	sky.CelestialBodiesShown = true
	sky.Parent = Lighting

	local atmosphere = Instance.new("Atmosphere")
	atmosphere.Name = "NexusAtmosphere"
	atmosphere.Density = 0.26
	atmosphere.Offset = 0.25
	atmosphere.Color = Color3.fromRGB(184, 216, 255)
	atmosphere.Decay = Color3.fromRGB(255, 224, 184)
	atmosphere.Glare = 0.22
	atmosphere.Haze = 1.2
	atmosphere.Parent = Lighting

	local bloom = Instance.new("BloomEffect")
	bloom.Name = "NexusBloom"
	bloom.Intensity = 0.45
	bloom.Size = 24
	bloom.Threshold = 1.1
	bloom.Parent = Lighting

	local sunRays = Instance.new("SunRaysEffect")
	sunRays.Name = "NexusSunRays"
	sunRays.Intensity = 0.12
	sunRays.Spread = 0.65
	sunRays.Parent = Lighting

	local cc = Instance.new("ColorCorrectionEffect")
	cc.Name = "NexusColorCorrection"
	cc.Saturation = 0.14
	cc.Contrast = 0.08
	cc.Brightness = 0.02
	cc.TintColor = Color3.fromRGB(255, 250, 242)
	cc.Parent = Lighting
end

local function hideDefaultBaseplate()
	local baseplate = findChildByName(Workspace, "Baseplate")
	if baseplate and baseplate:IsA("BasePart") then
		baseplate.Transparency = 1
		baseplate.CanCollide = false
		baseplate.CastShadow = false
	end
end

local function buildCourtyard(hub: Model)
	local ground = Instance.new("Model")
	ground.Name = "CourtyardGround"
	ground.Parent = hub

	makePart(
		ground,
		"EmeraldMeadow",
		Vector3.new(240, 4, 240),
		CFrame.new(0, -2.5, 0),
		PALETTE.Meadow,
		Enum.Material.Grass
	)
	makePart(
		ground,
		"OuterGardenBorder",
		Vector3.new(88, 0.7, 102),
		CFrame.new(0, -0.35, 2),
		PALETTE.MeadowBorder,
		Enum.Material.Slate
	)

	-- Stepped marble plaza courtyard
	makePart(
		ground,
		"PlazaStepLower",
		Vector3.new(76, 0.6, 90),
		CFrame.new(0, -0.2, 2),
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makePart(
		ground,
		"PlazaDeckMain",
		Vector3.new(70, 0.5, 84),
		CFrame.new(0, 0, 2),
		PALETTE.Marble,
		Enum.Material.Marble
	)

	-- Ceremonial stepped courtyard inlays & gold trim
	makePart(
		ground,
		"InlayWalkway",
		Vector3.new(12, 0.14, 62),
		CFrame.new(0, 0.28, 3),
		PALETTE.MarbleWarm,
		Enum.Material.SmoothPlastic
	)
	makePart(
		ground,
		"WalkwayGoldTrimLeft",
		Vector3.new(0.45, 0.18, 62),
		CFrame.new(-6.2, 0.28, 3),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		ground,
		"WalkwayGoldTrimRight",
		Vector3.new(0.45, 0.18, 62),
		CFrame.new(6.2, 0.28, 3),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		ground,
		"CrossInlay",
		Vector3.new(48, 0.14, 10),
		CFrame.new(0, 0.28, 4),
		PALETTE.MarbleWarm,
		Enum.Material.SmoothPlastic
	)

	local corners = {
		Vector3.new(-22, 0.27, -14),
		Vector3.new(22, 0.27, -14),
		Vector3.new(-22, 0.27, 22),
		Vector3.new(22, 0.27, 22),
	}
	for idx, pos in ipairs(corners) do
		makePart(
			ground,
			"CornerTile_" .. tostring(idx),
			Vector3.new(14, 0.12, 14),
			CFrame.new(pos),
			PALETTE.MarbleWarm,
			Enum.Material.Marble
		)
		makePart(
			ground,
			"CornerGold_" .. tostring(idx),
			Vector3.new(10, 0.15, 10),
			CFrame.new(pos + Vector3.new(0, 0.02, 0)) * CFrame.Angles(0, math.rad(45), 0),
			PALETTE.Gold,
			Enum.Material.Metal
		)
	end
end

local function buildSpawnMedallion(hub: Model)
	local spawnModel = Instance.new("Model")
	spawnModel.Name = "SpawnMedallion"
	spawnModel.Parent = hub

	local center = Vector3.new(0, 0.26, 26)
	makeUprightCylinder(spawnModel, "OuterRing", center, 0.22, 13, PALETTE.SlateTrim, Enum.Material.Marble)
	makeUprightCylinder(
		spawnModel,
		"GoldBevel",
		center + Vector3.new(0, 0.06, 0),
		0.22,
		11.4,
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makeUprightCylinder(
		spawnModel,
		"RecessedMarble",
		center + Vector3.new(0, 0.03, 0),
		0.22,
		10.2,
		PALETTE.Marble,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		spawnModel,
		"InnerGlowSigil",
		center + Vector3.new(0, 0.05, 0),
		0.22,
		6.4,
		PALETTE.GoldGlow,
		Enum.Material.Neon,
		0.35
	)

	-- SpawnLocation faces -Z toward Roll Altar (z=2) and Torii Arch (z=-21)
	local spawnLoc = Instance.new("SpawnLocation")
	spawnLoc.Name = "NexusSpawn"
	spawnLoc.Size = Vector3.new(6, 0.4, 6)
	spawnLoc.CFrame = CFrame.lookAt(Vector3.new(0, 0.42, 26), Vector3.new(0, 0.42, 2))
	spawnLoc.Anchored = true
	spawnLoc.CanCollide = false
	spawnLoc.Transparency = 1
	spawnLoc.Duration = 0
	spawnLoc.Parent = spawnModel
end

local function buildRollAltar(hub: Model, prompts: { ProximityPrompt })
	local altar = Instance.new("Model")
	altar.Name = "RollAltar"
	altar.Parent = hub

	local center = Vector3.new(0, 0, 2)

	makeUprightCylinder(
		altar,
		"DaisTier1",
		center + Vector3.new(0, 0.55, 0),
		0.7,
		16,
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		altar,
		"DaisGold1",
		center + Vector3.new(0, 0.92, 0),
		0.16,
		15.4,
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makeUprightCylinder(
		altar,
		"DaisTier2",
		center + Vector3.new(0, 1.35, 0),
		0.75,
		12.2,
		PALETTE.Marble,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		altar,
		"DaisGold2",
		center + Vector3.new(0, 1.75, 0),
		0.16,
		11.6,
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makeUprightCylinder(
		altar,
		"DaisTier3",
		center + Vector3.new(0, 2.15, 0),
		0.7,
		8.2,
		PALETTE.SlateTrim,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		altar,
		"AltarGlowRing",
		center + Vector3.new(0, 2.55, 0),
		0.14,
		7.4,
		PALETTE.GoldGlow,
		Enum.Material.Neon
	)

	makeUprightCylinder(
		altar,
		"PedestalCore",
		center + Vector3.new(0, 3.6, 0),
		2.1,
		3.2,
		PALETTE.Gold,
		Enum.Material.Metal
	)
	local crystal = makePart(
		altar,
		"SummonPrism",
		Vector3.new(2.2, 2.2, 2.2),
		CFrame.new(center + Vector3.new(0, 6.2, 0)) * CFrame.Angles(math.rad(35), math.rad(45), math.rad(20)),
		PALETTE.GoldGlow,
		Enum.Material.Neon,
		0.1
	)
	crystal.CanCollide = false

	local prismLight = Instance.new("PointLight")
	prismLight.Color = PALETTE.GoldGlow
	prismLight.Brightness = 1.6
	prismLight.Range = 22
	prismLight.Shadows = false
	prismLight.Parent = crystal

	for idx = 1, 4 do
		local angle = math.rad(45 + (idx - 1) * 90)
		local offset = Vector3.new(math.cos(angle) * 6.8, 0, math.sin(angle) * 6.8)
		makeUprightCylinder(
			altar,
			"AltarPillar_" .. tostring(idx),
			center + offset + Vector3.new(0, 2.4, 0),
			3.4,
			1.1,
			PALETTE.Marble,
			Enum.Material.Marble
		)
		makeUprightCylinder(
			altar,
			"AltarPillarCap_" .. tostring(idx),
			center + offset + Vector3.new(0, 4.2, 0),
			0.35,
			1.45,
			PALETTE.Gold,
			Enum.Material.Metal
		)
		makeBall(
			altar,
			"AltarOrb_" .. tostring(idx),
			center + offset + Vector3.new(0, 4.9, 0),
			0.9,
			PALETTE.GoldGlow,
			Enum.Material.Neon
		)
	end

	local promptAnchor = makePart(
		altar,
		"RollAltarAnchor",
		Vector3.new(4, 4, 4),
		CFrame.new(center + Vector3.new(0, 3.2, 0)),
		PALETTE.Gold,
		Enum.Material.SmoothPlastic,
		1
	)
	promptAnchor.CanCollide = false
	attachPrompt(promptAnchor, "Roll", "Nexus Roll Altar", "Summon Cards", prompts)
end

local function buildToriiPortal(hub: Model, prompts: { ProximityPrompt })
	local torii = Instance.new("Model")
	torii.Name = "World1PortalArch"
	torii.Parent = hub

	local center = Vector3.new(0, 0, -21)

	-- Raised ceremonial approach step
	makePart(
		torii,
		"ToriiPlatform",
		Vector3.new(22, 0.7, 9),
		CFrame.new(center + Vector3.new(0, 0.6, 0)),
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makePart(
		torii,
		"ToriiPlatformGold",
		Vector3.new(22.4, 0.2, 9.4),
		CFrame.new(center + Vector3.new(0, 0.92, 0)),
		PALETTE.Gold,
		Enum.Material.Metal
	)

	-- Twin towering crimson & gold Torii columns
	for _, xSide in ipairs({ -7.4, 7.4 }) do
		local tag = if xSide < 0 then "Left" else "Right"
		makeUprightCylinder(
			torii,
			"StonePlinth_" .. tag,
			center + Vector3.new(xSide, 1.85, 0),
			1.7,
			3.6,
			PALETTE.SlateTrim,
			Enum.Material.Marble
		)
		makeUprightCylinder(
			torii,
			"GoldCollarLow_" .. tag,
			center + Vector3.new(xSide, 2.85, 0),
			0.4,
			3.0,
			PALETTE.Gold,
			Enum.Material.Metal
		)
		makeUprightCylinder(
			torii,
			"CrimsonPillar_" .. tag,
			center + Vector3.new(xSide, 9.8, 0),
			13.6,
			2.35,
			PALETTE.Crimson,
			Enum.Material.SmoothPlastic
		)
		makeUprightCylinder(
			torii,
			"GoldCollarHigh_" .. tag,
			center + Vector3.new(xSide, 15.8, 0),
			0.45,
			2.75,
			PALETTE.Gold,
			Enum.Material.Metal
		)
	end

	-- Lower tie-beam (nuki) + central crest strut
	makePart(
		torii,
		"NukiBeam",
		Vector3.new(20.6, 1.25, 1.45),
		CFrame.new(center + Vector3.new(0, 13.0, 0)),
		PALETTE.Crimson,
		Enum.Material.SmoothPlastic
	)
	makePart(
		torii,
		"NukiGoldEndLeft",
		Vector3.new(0.45, 1.45, 1.65),
		CFrame.new(center + Vector3.new(-10.4, 13.0, 0)),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		torii,
		"NukiGoldEndRight",
		Vector3.new(0.45, 1.45, 1.65),
		CFrame.new(center + Vector3.new(10.4, 13.0, 0)),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		torii,
		"GakuzukaPlaque",
		Vector3.new(2.8, 2.8, 1.7),
		CFrame.new(center + Vector3.new(0, 14.9, 0)),
		PALETTE.SlateTrim,
		Enum.Material.Metal
	)
	makePart(
		torii,
		"PlaqueGoldFrame",
		Vector3.new(2.2, 2.2, 1.85),
		CFrame.new(center + Vector3.new(0, 14.9, 0)),
		PALETTE.GoldGlow,
		Enum.Material.Neon
	)

	-- Upper Torii roof beams (shimaki & flared kasagi)
	makePart(
		torii,
		"ShimakiBeam",
		Vector3.new(22.4, 1.25, 2.1),
		CFrame.new(center + Vector3.new(0, 16.5, 0)),
		PALETTE.Crimson,
		Enum.Material.SmoothPlastic
	)
	makePart(
		torii,
		"KasagiCap",
		Vector3.new(25.2, 1.2, 2.7),
		CFrame.new(center + Vector3.new(0, 17.65, 0)),
		PALETTE.CrimsonDark,
		Enum.Material.SmoothPlastic
	)
	makePart(
		torii,
		"KasagiWingLeft",
		Vector3.new(4.2, 1.15, 2.75),
		CFrame.new(center + Vector3.new(-13.8, 18.25, 0)) * CFrame.Angles(0, 0, math.rad(-14)),
		PALETTE.CrimsonDark,
		Enum.Material.SmoothPlastic
	)
	makePart(
		torii,
		"KasagiWingRight",
		Vector3.new(4.2, 1.15, 2.75),
		CFrame.new(center + Vector3.new(13.8, 18.25, 0)) * CFrame.Angles(0, 0, math.rad(14)),
		PALETTE.CrimsonDark,
		Enum.Material.SmoothPlastic
	)
	makePart(
		torii,
		"KasagiGoldRidge",
		Vector3.new(24.8, 0.35, 1.2),
		CFrame.new(center + Vector3.new(0, 18.35, 0)),
		PALETTE.Gold,
		Enum.Material.Metal
	)

	-- Shimmering portal veil
	local veil = makePart(
		torii,
		"PortalVeil",
		Vector3.new(12.2, 11.2, 0.45),
		CFrame.new(center + Vector3.new(0, 6.7, 0)),
		PALETTE.PortalCyan,
		Enum.Material.Neon,
		0.28
	)
	veil.CanCollide = false

	local portalLight = Instance.new("PointLight")
	portalLight.Color = PALETTE.PortalCyan
	portalLight.Brightness = 1.4
	portalLight.Range = 22
	portalLight.Shadows = false
	portalLight.Parent = veil

	attachPrompt(veil, "Battle", "World 1: Leaf Village Gate", "Enter Battle", prompts)
end

local function buildCardStandee(hub: Model, prompts: { ProximityPrompt })
	local standee = Instance.new("Model")
	standee.Name = "CardStandee"
	standee.Parent = hub

	local origin = CFrame.lookAt(Vector3.new(20, 0.25, 6), Vector3.new(0, 0.25, 6))

	makePart(
		standee,
		"StandeeBase",
		Vector3.new(7.4, 0.7, 5.2),
		origin * CFrame.new(0, 0.35, 0),
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makePart(
		standee,
		"StandeeTrim",
		Vector3.new(7.6, 0.2, 5.4),
		origin * CFrame.new(0, 0.72, 0),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		standee,
		"EaselLeft",
		Vector3.new(0.65, 8.6, 0.65),
		origin * CFrame.new(-2.8, 4.9, 0.4),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		standee,
		"EaselRight",
		Vector3.new(0.65, 8.6, 0.65),
		origin * CFrame.new(2.8, 4.9, 0.4),
		PALETTE.Gold,
		Enum.Material.Metal
	)

	local frame = makePart(
		standee,
		"DisplayCardFrame",
		Vector3.new(5.8, 8.4, 0.6),
		origin * CFrame.new(0, 5.2, 0) * CFrame.Angles(math.rad(-8), 0, 0),
		PALETTE.SlateTrim,
		Enum.Material.Metal
	)
	makePart(
		standee,
		"DisplayCardGlow",
		Vector3.new(4.9, 7.3, 0.7),
		origin * CFrame.new(0, 5.2, 0) * CFrame.Angles(math.rad(-8), 0, 0),
		PALETTE.CrystalBlue,
		Enum.Material.Neon,
		0.18
	)
	makePart(
		standee,
		"DisplayCardCrest",
		Vector3.new(1.3, 1.3, 0.9),
		origin * CFrame.new(0, 9.45, 0.55) * CFrame.Angles(0, 0, math.rad(45)),
		PALETTE.GoldGlow,
		Enum.Material.Neon
	)

	attachPrompt(frame, "Deck", "Deck & Collection", "Manage Deck", prompts)
end

local function buildShopKiosk(hub: Model, prompts: { ProximityPrompt })
	local kiosk = Instance.new("Model")
	kiosk.Name = "ShopKiosk"
	kiosk.Parent = hub

	local origin = CFrame.lookAt(Vector3.new(-20, 0.25, 6), Vector3.new(0, 0.25, 6))

	makePart(
		kiosk,
		"KioskFloor",
		Vector3.new(8.6, 0.6, 6.8),
		origin * CFrame.new(0, 0.3, 0),
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	local counter = makePart(
		kiosk,
		"KioskCounter",
		Vector3.new(7.8, 2.8, 2.2),
		origin * CFrame.new(0, 1.9, -1.6),
		PALETTE.Marble,
		Enum.Material.Marble
	)
	makePart(
		kiosk,
		"CounterGoldTop",
		Vector3.new(8.2, 0.35, 2.5),
		origin * CFrame.new(0, 3.4, -1.6),
		PALETTE.Gold,
		Enum.Material.Metal
	)

	for _, xOff in ipairs({ -3.6, 3.6 }) do
		for _, zOff in ipairs({ -2.4, 2.4 }) do
			makePart(
				kiosk,
				"KioskPost",
				Vector3.new(0.65, 7.6, 0.65),
				origin * CFrame.new(xOff, 4.3, zOff),
				PALETTE.Wood,
				Enum.Material.Wood
			)
		end
	end

	-- Crimson & gold pitched awning roof
	makePart(
		kiosk,
		"AwningFront",
		Vector3.new(9.6, 0.6, 4.4),
		origin * CFrame.new(0, 8.6, -1.8) * CFrame.Angles(math.rad(-22), 0, 0),
		PALETTE.Crimson,
		Enum.Material.SmoothPlastic
	)
	makePart(
		kiosk,
		"AwningBack",
		Vector3.new(9.6, 0.6, 4.4),
		origin * CFrame.new(0, 8.6, 1.8) * CFrame.Angles(math.rad(22), 0, 0),
		PALETTE.CrimsonDark,
		Enum.Material.SmoothPlastic
	)
	makePart(
		kiosk,
		"AwningGoldRidge",
		Vector3.new(9.8, 0.55, 0.7),
		origin * CFrame.new(0, 9.4, 0),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		kiosk,
		"GemDisplay",
		Vector3.new(1.1, 1.1, 1.1),
		origin * CFrame.new(0, 4.3, -1.6) * CFrame.Angles(math.rad(35), math.rad(45), 0),
		PALETTE.PortalCyan,
		Enum.Material.Neon
	)

	attachPrompt(counter, "Shop", "Nexus Bazaar", "Open Shop", prompts)
end

local function buildStoneLantern(parent: Instance, name: string, pos: Vector3)
	local lantern = Instance.new("Model")
	lantern.Name = name
	lantern.Parent = parent

	makePart(
		lantern,
		"Base",
		Vector3.new(2.2, 0.6, 2.2),
		CFrame.new(pos + Vector3.new(0, 0.55, 0)),
		PALETTE.StoneDark,
		Enum.Material.Slate
	)
	makeUprightCylinder(lantern, "Post", pos + Vector3.new(0, 2.35, 0), 3.0, 0.9, PALETTE.Stone, Enum.Material.Slate)
	makePart(
		lantern,
		"Plate",
		Vector3.new(1.9, 0.35, 1.9),
		CFrame.new(pos + Vector3.new(0, 4.0, 0)),
		PALETTE.StoneDark,
		Enum.Material.Slate
	)
	local core = makePart(
		lantern,
		"GlowCore",
		Vector3.new(1.15, 1.1, 1.15),
		CFrame.new(pos + Vector3.new(0, 4.7, 0)),
		PALETTE.LanternGlow,
		Enum.Material.Neon
	)
	local light = Instance.new("PointLight")
	light.Color = PALETTE.LanternGlow
	light.Brightness = 1.05
	light.Range = 16
	light.Shadows = false
	light.Parent = core

	makePart(
		lantern,
		"Cap",
		Vector3.new(2.4, 0.55, 2.4),
		CFrame.new(pos + Vector3.new(0, 5.5, 0)),
		PALETTE.StoneDark,
		Enum.Material.Slate
	)
	makeBall(lantern, "Finial", pos + Vector3.new(0, 6.0, 0), 0.7, PALETTE.Gold, Enum.Material.Metal)
end

local function buildSculptedTree(parent: Instance, name: string, pos: Vector3, isSakura: boolean, scale: number)
	local tree = Instance.new("Model")
	tree.Name = name
	tree.Parent = parent

	makeUprightCylinder(
		tree,
		"PlanterRing",
		pos + Vector3.new(0, 0.45, 0),
		0.7,
		5.6 * scale,
		PALETTE.Stone,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		tree,
		"Trunk",
		pos + Vector3.new(0, 3.6 * scale, 0),
		6.4 * scale,
		1.5 * scale,
		PALETTE.Wood,
		Enum.Material.Wood
	)

	local primaryColor = if isSakura then PALETTE.SakuraMid else PALETTE.EmeraldLeaf
	local highlightColor = if isSakura then PALETTE.SakuraLight else PALETTE.EmeraldBright

	local canopies = {
		{ offset = Vector3.new(0, 7.6 * scale, 0), size = 6.2 * scale, color = primaryColor },
		{ offset = Vector3.new(-2.4 * scale, 6.6 * scale, 1.3 * scale), size = 4.5 * scale, color = highlightColor },
		{ offset = Vector3.new(2.3 * scale, 6.8 * scale, -1.2 * scale), size = 4.6 * scale, color = highlightColor },
		{ offset = Vector3.new(0.4 * scale, 9.2 * scale, 0.5 * scale), size = 4.1 * scale, color = highlightColor },
	}
	for idx, cluster in ipairs(canopies) do
		makeBall(
			tree,
			"Canopy_" .. tostring(idx),
			pos + cluster.offset,
			cluster.size,
			cluster.color,
			Enum.Material.SmoothPlastic
		)
	end
end

local function buildPerimeterBalustrades(hub: Model)
	local wallModel = Instance.new("Model")
	wallModel.Name = "PerimeterBalustrades"
	wallModel.Parent = hub

	local segments = {
		{ size = Vector3.new(1.2, 1.6, 78), pos = Vector3.new(-34.2, 0.9, 2) },
		{ size = Vector3.new(1.2, 1.6, 78), pos = Vector3.new(34.2, 0.9, 2) },
		{ size = Vector3.new(24, 1.6, 1.2), pos = Vector3.new(-22.5, 0.9, -39) },
		{ size = Vector3.new(24, 1.6, 1.2), pos = Vector3.new(22.5, 0.9, -39) },
	}
	for idx, seg in ipairs(segments) do
		makePart(
			wallModel,
			"Balustrade_" .. tostring(idx),
			seg.size,
			CFrame.new(seg.pos),
			PALETTE.MarbleWarm,
			Enum.Material.Marble
		)
	end

	local crystalPosts = {
		Vector3.new(-34.2, 0, -39),
		Vector3.new(34.2, 0, -39),
		Vector3.new(-34.2, 0, 41),
		Vector3.new(34.2, 0, 41),
	}
	for idx, postPos in ipairs(crystalPosts) do
		makePart(
			wallModel,
			"CornerPost_" .. tostring(idx),
			Vector3.new(2.2, 3.4, 2.2),
			CFrame.new(postPos + Vector3.new(0, 1.7, 0)),
			PALETTE.SlateTrim,
			Enum.Material.Marble
		)
		makeBall(
			wallModel,
			"CornerOrb_" .. tostring(idx),
			postPos + Vector3.new(0, 4.0, 0),
			1.2,
			PALETTE.GoldGlow,
			Enum.Material.Neon
		)
	end
end

type WorldPortalSpec = {
	prop: string,
	name: string,
	title: string,
	subtitle: string,
	pos: Vector3,
	primary: Color3,
	secondary: Color3,
	accent: Color3,
	style: string,
}

local function buildWorldPortals(hub: Model, prompts: { ProximityPrompt })
	local group = Instance.new("Model")
	group.Name = "WorldPortals2To5"
	group.Parent = hub

	local specs: { WorldPortalSpec } = {
		{
			prop = "World2",
			name = "World2CapsuleGate",
			title = "World 2: West City & Namek",
			subtitle = "Capsule Gate",
			pos = Vector3.new(-15, 0.25, -33),
			primary = Color3.fromRGB(240, 244, 250),
			secondary = Color3.fromRGB(48, 128, 224),
			accent = Color3.fromRGB(96, 224, 152),
			style = "capsule",
		},
		{
			prop = "World3",
			name = "World3PirateArch",
			title = "World 3: Grand Line",
			subtitle = "Pirate Arch",
			pos = Vector3.new(15, 0.25, -33),
			primary = Color3.fromRGB(116, 74, 46),
			secondary = Color3.fromRGB(228, 178, 64),
			accent = Color3.fromRGB(72, 198, 255),
			style = "pirate",
		},
		{
			prop = "World4",
			name = "World4JujutsuTorii",
			title = "World 4: Tokyo Jujutsu",
			subtitle = "Cursed Torii",
			pos = Vector3.new(-14, 0.25, 34),
			primary = Color3.fromRGB(42, 34, 56),
			secondary = Color3.fromRGB(124, 58, 214),
			accent = Color3.fromRGB(178, 110, 255),
			style = "torii",
		},
		{
			prop = "World5",
			name = "World5SenkaimonGate",
			title = "World 5: Soul Society",
			subtitle = "Senkaimon Gate",
			pos = Vector3.new(14, 0.25, 34),
			primary = Color3.fromRGB(238, 240, 246),
			secondary = Color3.fromRGB(36, 40, 52),
			accent = Color3.fromRGB(110, 214, 255),
			style = "senkaimon",
		},
	}

	local centerTarget = Vector3.new(0, 0.25, 2)
	for _, spec in ipairs(specs) do
		local model = Instance.new("Model")
		model.Name = spec.name
		model.Parent = group

		local origin = CFrame.lookAt(spec.pos, centerTarget)
		makePart(
			model,
			"Plinth",
			Vector3.new(10.5, 0.6, 5.4),
			origin * CFrame.new(0, 0.3, 0),
			PALETTE.MarbleWarm,
			Enum.Material.Marble
		)
		makePart(
			model,
			"Trim",
			Vector3.new(10.8, 0.18, 5.7),
			origin * CFrame.new(0, 0.65, 0),
			spec.secondary,
			Enum.Material.Metal
		)

		for _, xSide in ipairs({ -3.8, 3.8 }) do
			makePart(
				model,
				"Pillar",
				Vector3.new(1.3, 8.2, 1.3),
				origin * CFrame.new(xSide, 4.7, 0),
				spec.primary,
				Enum.Material.SmoothPlastic
			)
			makePart(
				model,
				"Cap",
				Vector3.new(1.7, 0.6, 1.7),
				origin * CFrame.new(xSide, 9.0, 0),
				spec.secondary,
				Enum.Material.Metal
			)
		end

		if spec.style == "capsule" then
			makePart(
				model,
				"DomeArch",
				Vector3.new(10.2, 1.6, 1.8),
				origin * CFrame.new(0, 9.8, 0),
				spec.primary,
				Enum.Material.SmoothPlastic
			)
			makeBall(
				model,
				"CapsuleOrb",
				(origin * CFrame.new(0, 11.4, 0)).Position,
				2.2,
				spec.secondary,
				Enum.Material.SmoothPlastic
			)
		elseif spec.style == "pirate" then
			makePart(
				model,
				"CrossBeam",
				Vector3.new(10.8, 1.3, 1.6),
				origin * CFrame.new(0, 9.6, 0),
				spec.primary,
				Enum.Material.Wood
			)
			makePart(
				model,
				"GoldCrest",
				Vector3.new(2.4, 2.0, 1.8),
				origin * CFrame.new(0, 11.1, 0),
				spec.secondary,
				Enum.Material.Metal
			)
		elseif spec.style == "torii" then
			makePart(
				model,
				"Nuki",
				Vector3.new(10.6, 0.9, 1.2),
				origin * CFrame.new(0, 7.6, 0),
				spec.primary,
				Enum.Material.SmoothPlastic
			)
			makePart(
				model,
				"Kasagi",
				Vector3.new(12.2, 1.1, 1.9),
				origin * CFrame.new(0, 9.7, 0),
				spec.secondary,
				Enum.Material.SmoothPlastic
			)
		else
			makePart(
				model,
				"Lintel",
				Vector3.new(10.4, 1.5, 1.8),
				origin * CFrame.new(0, 9.8, 0),
				spec.secondary,
				Enum.Material.Slate
			)
			makePart(
				model,
				"Crest",
				Vector3.new(3.0, 1.4, 2.0),
				origin * CFrame.new(0, 11.1, 0),
				spec.accent,
				Enum.Material.Neon
			)
		end

		local veil = makePart(
			model,
			"PortalVeil",
			Vector3.new(6.2, 7.6, 0.4),
			origin * CFrame.new(0, 4.7, 0),
			spec.accent,
			Enum.Material.Neon,
			0.25
		)
		veil.CanCollide = false

		local light = Instance.new("PointLight")
		light.Color = spec.accent
		light.Brightness = 1.2
		light.Range = 16
		light.Shadows = false
		light.Parent = veil

		attachSign(veil, spec.title, spec.subtitle, spec.accent, Vector3.new(0, 6.2, 0))
		attachPrompt(veil, spec.prop, spec.title, "Enter " .. spec.prop, prompts)
	end
end

local function buildEndgameShrines(hub: Model, prompts: { ProximityPrompt })
	local group = Instance.new("Model")
	group.Name = "EndgameShrines"
	group.Parent = hub

	local shrines = {
		{
			prop = "Tower",
			name = "HeavensArenaObelisk",
			title = "Heavens Arena",
			subtitle = "100-Floor Tower",
			pos = Vector3.new(-22, 0.35, -14),
			body = Color3.fromRGB(236, 232, 224),
			accent = Color3.fromRGB(255, 204, 72),
			shape = "obelisk",
		},
		{
			prop = "Raid",
			name = "RaidBreachGate",
			title = "Raid Breach",
			subtitle = "Co-op Boss Raids",
			pos = Vector3.new(22, 0.35, -14),
			body = Color3.fromRGB(58, 36, 46),
			accent = Color3.fromRGB(255, 86, 98),
			shape = "gate",
		},
		{
			prop = "Ranked",
			name = "RankedArenaPlinth",
			title = "Ranked Arena",
			subtitle = "Async PvP Ladder",
			pos = Vector3.new(-22, 0.35, 22),
			body = Color3.fromRGB(48, 62, 92),
			accent = Color3.fromRGB(92, 198, 255),
			shape = "plinth",
		},
		{
			prop = "Rebirth",
			name = "RebirthSanctuary",
			title = "Rebirth Sanctuary",
			subtitle = "Ascension & Skill Tree",
			pos = Vector3.new(22, 0.35, 22),
			body = Color3.fromRGB(242, 236, 248),
			accent = Color3.fromRGB(204, 128, 255),
			shape = "sanctuary",
		},
	}

	local centerTarget = Vector3.new(0, 0.35, 2)
	for _, sh in ipairs(shrines) do
		local model = Instance.new("Model")
		model.Name = sh.name
		model.Parent = group

		local origin = CFrame.lookAt(sh.pos, centerTarget)
		makeUprightCylinder(
			model,
			"Base",
			sh.pos + Vector3.new(0, 0.4, 0),
			0.7,
			7.6,
			PALETTE.SlateTrim,
			Enum.Material.Marble
		)
		makeUprightCylinder(
			model,
			"GlowRing",
			sh.pos + Vector3.new(0, 0.8, 0),
			0.16,
			6.8,
			sh.accent,
			Enum.Material.Neon
		)

		local anchor: Part
		if sh.shape == "obelisk" then
			anchor = makePart(
				model,
				"ObeliskShaft",
				Vector3.new(2.2, 8.6, 2.2),
				origin * CFrame.new(0, 5.1, 0),
				sh.body,
				Enum.Material.Marble
			)
			makePart(
				model,
				"ObeliskTip",
				Vector3.new(1.5, 1.5, 1.5),
				origin * CFrame.new(0, 10.2, 0) * CFrame.Angles(math.rad(45), math.rad(45), 0),
				sh.accent,
				Enum.Material.Neon
			)
		elseif sh.shape == "gate" then
			makePart(
				model,
				"LeftSpike",
				Vector3.new(1.2, 6.8, 1.2),
				origin * CFrame.new(-2.2, 4.2, 0) * CFrame.Angles(0, 0, math.rad(-8)),
				sh.body,
				Enum.Material.Slate
			)
			makePart(
				model,
				"RightSpike",
				Vector3.new(1.2, 6.8, 1.2),
				origin * CFrame.new(2.2, 4.2, 0) * CFrame.Angles(0, 0, math.rad(8)),
				sh.body,
				Enum.Material.Slate
			)
			anchor = makePart(
				model,
				"BreachCore",
				Vector3.new(3.2, 5.2, 0.5),
				origin * CFrame.new(0, 4.0, 0),
				sh.accent,
				Enum.Material.Neon,
				0.2
			)
		elseif sh.shape == "plinth" then
			makeUprightCylinder(
				model,
				"Column",
				sh.pos + Vector3.new(0, 2.8, 0),
				4.0,
				2.6,
				sh.body,
				Enum.Material.Marble
			)
			anchor = makePart(
				model,
				"TrophyCrystal",
				Vector3.new(2.1, 2.1, 2.1),
				origin * CFrame.new(0, 6.0, 0) * CFrame.Angles(math.rad(35), math.rad(45), 0),
				sh.accent,
				Enum.Material.Neon
			)
		else
			makeUprightCylinder(
				model,
				"AltarColumn",
				sh.pos + Vector3.new(0, 2.6, 0),
				3.6,
				2.8,
				sh.body,
				Enum.Material.Marble
			)
			anchor = makeBall(
				model,
				"AscensionOrb",
				sh.pos + Vector3.new(0, 5.8, 0),
				2.4,
				sh.accent,
				Enum.Material.Neon,
				0.1
			)
		end

		attachSign(anchor, sh.title, sh.subtitle, sh.accent, Vector3.new(0, 4.2, 0))
		attachPrompt(anchor, sh.prop, sh.title, "Open " .. sh.prop, prompts)
	end
end

function WorldBuild.build(): BuildResult
	local existingHub = findChildByName(Workspace, "NexusPlazaHub")
	if existingHub then
		existingHub:Destroy()
	end

	hideDefaultBaseplate()
	setupLighting()

	local hub = Instance.new("Model")
	hub.Name = "NexusPlazaHub"

	local prompts: { ProximityPrompt } = {}

	buildCourtyard(hub)
	buildSpawnMedallion(hub)
	buildRollAltar(hub, prompts)
	buildToriiPortal(hub, prompts)
	buildCardStandee(hub, prompts)
	buildShopKiosk(hub, prompts)
	buildWorldPortals(hub, prompts)
	buildEndgameShrines(hub, prompts)
	buildPerimeterBalustrades(hub)

	local lanternsModel = Instance.new("Model")
	lanternsModel.Name = "StoneLanterns"
	lanternsModel.Parent = hub
	local lanternPositions = {
		Vector3.new(-8.2, 0, 18),
		Vector3.new(8.2, 0, 18),
		Vector3.new(-8.2, 0, -10),
		Vector3.new(8.2, 0, -10),
		Vector3.new(-22, 0, -22),
		Vector3.new(22, 0, -22),
	}
	for idx, lPos in ipairs(lanternPositions) do
		buildStoneLantern(lanternsModel, "StoneLantern_" .. tostring(idx), lPos)
	end

	local treesModel = Instance.new("Model")
	treesModel.Name = "PlazaTrees"
	treesModel.Parent = hub
	local treeSpecs = {
		{ pos = Vector3.new(-28, 0, -28), sakura = true, scale = 1.15 },
		{ pos = Vector3.new(28, 0, -28), sakura = true, scale = 1.15 },
		{ pos = Vector3.new(-42, 0, -8), sakura = false, scale = 1.25 },
		{ pos = Vector3.new(42, 0, -8), sakura = false, scale = 1.25 },
		{ pos = Vector3.new(-40, 0, 20), sakura = true, scale = 1.1 },
		{ pos = Vector3.new(40, 0, 20), sakura = true, scale = 1.1 },
		{ pos = Vector3.new(-24, 0, 36), sakura = false, scale = 1.0 },
		{ pos = Vector3.new(24, 0, 36), sakura = false, scale = 1.0 },
	}
	for idx, spec in ipairs(treeSpecs) do
		buildSculptedTree(treesModel, "PlazaTree_" .. tostring(idx), spec.pos, spec.sakura, spec.scale)
	end

	hub.Parent = Workspace
	return {
		hub = hub,
		prompts = prompts,
	}
end

return WorldBuild
