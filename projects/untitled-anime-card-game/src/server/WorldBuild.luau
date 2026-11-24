--!strict
local CollectionService = game:GetService("CollectionService")
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
	Obsidian = Color3.fromRGB(26, 22, 34),
	Navy = Color3.fromRGB(24, 44, 82),
	Brass = Color3.fromRGB(204, 158, 66),
	LotusPink = Color3.fromRGB(236, 176, 255),
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
	Lighting.ClockTime = 16.6
	Lighting.GeographicLatitude = 32
	Lighting.Brightness = 2.6
	Lighting.Ambient = Color3.fromRGB(152, 138, 128)
	Lighting.OutdoorAmbient = Color3.fromRGB(174, 156, 140)
	Lighting.ColorShift_Top = Color3.fromRGB(255, 224, 178)
	Lighting.ColorShift_Bottom = Color3.fromRGB(122, 118, 146)
	Lighting.EnvironmentDiffuseScale = 0.75
	Lighting.EnvironmentSpecularScale = 0.8
	Lighting.GlobalShadows = true

	local managedNames = {
		"NexusSky",
		"NexusAtmosphere",
		"NexusBloom",
		"NexusSunRays",
		"NexusColorCorrection",
		"UACGColorCorrection",
	}
	for _, itemName in ipairs(managedNames) do
		local existing = findChildByName(Lighting, itemName)
		if existing then
			existing:Destroy()
		end
	end

	local sky = Instance.new("Sky")
	sky.Name = "NexusSky"
	sky.SunAngularSize = 20
	sky.MoonAngularSize = 8
	sky.CelestialBodiesShown = true
	sky.Parent = Lighting

	local atmosphere = Instance.new("Atmosphere")
	atmosphere.Name = "NexusAtmosphere"
	atmosphere.Density = 0.24
	atmosphere.Offset = 0.22
	atmosphere.Color = Color3.fromRGB(255, 226, 176)
	atmosphere.Decay = Color3.fromRGB(255, 188, 128)
	atmosphere.Glare = 0.45
	atmosphere.Haze = 1.4
	atmosphere.Parent = Lighting

	local bloom = Instance.new("BloomEffect")
	bloom.Name = "NexusBloom"
	bloom.Intensity = 0.35
	bloom.Size = 24
	bloom.Threshold = 1.15
	bloom.Parent = Lighting

	local sunRays = Instance.new("SunRaysEffect")
	sunRays.Name = "NexusSunRays"
	sunRays.Intensity = 0.14
	sunRays.Spread = 0.65
	sunRays.Parent = Lighting

	local cc = Instance.new("ColorCorrectionEffect")
	cc.Name = "UACGColorCorrection"
	cc.Saturation = 0.12
	cc.Contrast = 0.06
	cc.Brightness = 0.02
	cc.TintColor = Color3.fromRGB(255, 248, 236)
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
		Vector3.new(260, 4, 260),
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
	makePart(
		ground,
		"CrossGoldNorth",
		Vector3.new(48, 0.17, 0.35),
		CFrame.new(0, 0.29, -1.1),
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		ground,
		"CrossGoldSouth",
		Vector3.new(48, 0.17, 0.35),
		CFrame.new(0, 0.29, 9.1),
		PALETTE.Gold,
		Enum.Material.Metal
	)

	-- Radial mosaic ring around central altar
	makeUprightCylinder(
		ground,
		"CenterPlazaMosaic",
		Vector3.new(0, 0.24, 2),
		0.14,
		24,
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		ground,
		"CenterPlazaGoldRing",
		Vector3.new(0, 0.26, 2),
		0.14,
		22.6,
		PALETTE.Gold,
		Enum.Material.Metal
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

local function buildShowcaseStandee(hub: Model)
	local standee = Instance.new("Model")
	standee.Name = "ShowcaseStandee"
	standee.Parent = hub

	local origin = CFrame.lookAt(Vector3.new(11.5, 0.25, -4), Vector3.new(0, 0.25, 10))

	makeUprightCylinder(
		standee,
		"PedestalBase",
		(origin * CFrame.new(0, 0.45, 0)).Position,
		0.9,
		6.4,
		PALETTE.MarbleWarm,
		Enum.Material.Marble
	)
	makeUprightCylinder(
		standee,
		"PedestalGoldRim",
		(origin * CFrame.new(0, 0.95, 0)).Position,
		0.2,
		6.0,
		PALETTE.Gold,
		Enum.Material.Metal
	)
	makePart(
		standee,
		"PlinthColumn",
		Vector3.new(3.8, 2.2, 2.2),
		origin * CFrame.new(0, 2.1, 0),
		PALETTE.SlateTrim,
		Enum.Material.Marble
	)
	local trimFrame = makePart(
		standee,
		"ShowcaseTrimFrame",
		Vector3.new(4.9, 6.2, 0.45),
		origin * CFrame.new(0, 6.3, 0),
		PALETTE.GoldGlow,
		Enum.Material.Neon,
		0.15
	)
	local plaque = makePart(
		standee,
		"ShowcasePlaque",
		Vector3.new(4.4, 5.7, 0.55),
		origin * CFrame.new(0, 6.3, 0),
		PALETTE.Obsidian,
		Enum.Material.SmoothPlastic
	)

	local gui = Instance.new("SurfaceGui")
	gui.Name = "ShowcaseGui"
	gui.Face = Enum.NormalId.Front
	gui.SizingMode = Enum.SurfaceGuiSizingMode.PixelsPerStud
	gui.PixelsPerStud = 40
	gui.Parent = plaque

	local bg = Instance.new("Frame")
	bg.Size = UDim2.fromScale(1, 1)
	bg.BackgroundColor3 = Color3.fromRGB(16, 20, 32)
	bg.BorderSizePixel = 0
	bg.Parent = gui

	local stroke = Instance.new("UIStroke")
	stroke.Color = PALETTE.GoldGlow
	stroke.Thickness = 4
	stroke.Parent = bg

	local header = Instance.new("TextLabel")
	header.Size = UDim2.fromScale(0.9, 0.16)
	header.Position = UDim2.fromScale(0.05, 0.06)
	header.BackgroundTransparency = 1
	header.Font = Enum.Font.GothamBold
	header.Text = "FEATURED LEGEND"
	header.TextColor3 = PALETTE.GoldGlow
	header.TextScaled = true
	header.Parent = bg

	local nameLbl = Instance.new("TextLabel")
	nameLbl.Size = UDim2.fromScale(0.88, 0.44)
	nameLbl.Position = UDim2.fromScale(0.06, 0.28)
	nameLbl.BackgroundTransparency = 1
	nameLbl.Font = Enum.Font.GothamBlack
	nameLbl.Text = "Seventh Hokage"
	nameLbl.TextColor3 = Color3.fromRGB(255, 255, 255)
	nameLbl.TextWrapped = true
	nameLbl.TextScaled = true
	nameLbl.Parent = bg

	local rarityLbl = Instance.new("TextLabel")
	rarityLbl.Size = UDim2.fromScale(0.76, 0.14)
	rarityLbl.Position = UDim2.fromScale(0.12, 0.78)
	rarityLbl.BackgroundColor3 = Color3.fromRGB(34, 28, 18)
	rarityLbl.Font = Enum.Font.GothamBold
	rarityLbl.Text = "NEXUS / MYTHIC"
	rarityLbl.TextColor3 = PALETTE.GoldGlow
	rarityLbl.TextScaled = true
	rarityLbl.Parent = bg

	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0, 8)
	corner.Parent = rarityLbl

	local legends = {
		{ name = "Seventh Hokage", rarity = "MYTHIC", color = Color3.fromRGB(255, 178, 68) },
		{ name = "Autonomous Master", rarity = "UNIVERSAL", color = Color3.fromRGB(168, 224, 255) },
		{ name = "Warrior of Liberation", rarity = "NEXUS", color = Color3.fromRGB(255, 214, 102) },
		{ name = "Honored One", rarity = "SECRET", color = Color3.fromRGB(204, 128, 255) },
		{ name = "True Bankai", rarity = "LEGENDARY", color = Color3.fromRGB(255, 118, 92) },
	}

	task.spawn(function()
		local idx = 1
		while plaque.Parent and standee.Parent do
			local entry = legends[idx]
			nameLbl.Text = entry.name
			rarityLbl.Text = entry.rarity
			rarityLbl.TextColor3 = entry.color
			stroke.Color = entry.color
			trimFrame.Color = entry.color
			idx = (idx % #legends) + 1
			task.wait(4)
		end
	end)
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
		"RollAltarCrystal",
		Vector3.new(2.3, 3.1, 2.3),
		CFrame.new(center + Vector3.new(0, 6.4, 0)) * CFrame.Angles(math.rad(35), math.rad(45), math.rad(20)),
		PALETTE.GoldGlow,
		Enum.Material.Neon,
		0.08
	)
	crystal.CanCollide = false

	local coreAttachment = Instance.new("Attachment")
	coreAttachment.Name = "AltarSparkAttachment"
	coreAttachment.Parent = crystal

	local sparks = Instance.new("ParticleEmitter")
	sparks.Name = "AltarCoreSparks"
	sparks.Color = ColorSequence.new(PALETTE.GoldGlow, Color3.fromRGB(255, 246, 204))
	sparks.Rate = 14
	sparks.Lifetime = NumberRange.new(1.4, 2.4)
	sparks.Speed = NumberRange.new(1.4, 3.0)
	sparks.SpreadAngle = Vector2.new(35, 35)
	sparks.Size = NumberSequence.new({
		NumberSequenceKeypoint.new(0, 0.16),
		NumberSequenceKeypoint.new(0.6, 0.28),
		NumberSequenceKeypoint.new(1, 0),
	})
	sparks.Transparency = NumberSequence.new({
		NumberSequenceKeypoint.new(0, 0.15),
		NumberSequenceKeypoint.new(0.7, 0.35),
		NumberSequenceKeypoint.new(1, 1),
	})
	sparks.LightEmission = 0.85
	sparks.EmissionDirection = Enum.NormalId.Top
	sparks.Parent = coreAttachment

	local ringA = makePart(
		altar,
		"AltarOrbitRingA",
		Vector3.new(0.18, 7.6, 7.6),
		CFrame.new(center + Vector3.new(0, 6.4, 0)) * CFrame.Angles(math.rad(28), 0, math.rad(18)),
		PALETTE.GoldGlow,
		Enum.Material.Neon,
		0.62
	)
	ringA.Shape = Enum.PartType.Cylinder
	ringA.CanCollide = false
	ringA:SetAttribute("AltarOrbitRing", true)
	CollectionService:AddTag(ringA, "AltarOrbitRing")

	local ringB = makePart(
		altar,
		"AltarOrbitRingB",
		Vector3.new(0.18, 8.8, 8.8),
		CFrame.new(center + Vector3.new(0, 6.4, 0)) * CFrame.Angles(math.rad(-26), math.rad(45), math.rad(-22)),
		PALETTE.CrystalBlue,
		Enum.Material.Neon,
		0.66
	)
	ringB.Shape = Enum.PartType.Cylinder
	ringB.CanCollide = false
	ringB:SetAttribute("AltarOrbitRing", true)
	CollectionService:AddTag(ringB, "AltarOrbitRing")

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

	-- Twin towering crimson & gold Torii columns + flanking chakra stone lanterns
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

		-- Flanking chakra stone lanterns on the Torii platform
		local lanternX = if xSide < 0 then -10.2 else 10.2
		makePart(
			torii,
			"ChakraLanternPost_" .. tag,
			Vector3.new(1.1, 2.6, 1.1),
			CFrame.new(center + Vector3.new(lanternX, 2.3, 2.6)),
			PALETTE.Stone,
			Enum.Material.Slate
		)
		makePart(
			torii,
			"ChakraLanternCore_" .. tag,
			Vector3.new(0.85, 0.85, 0.85),
			CFrame.new(center + Vector3.new(lanternX, 4.0, 2.6)),
			PALETTE.PortalCyan,
			Enum.Material.Neon
		)
		makePart(
			torii,
			"ChakraLanternCap_" .. tag,
			Vector3.new(1.5, 0.45, 1.5),
			CFrame.new(center + Vector3.new(lanternX, 4.65, 2.6)),
			PALETTE.StoneDark,
			Enum.Material.Slate
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

	attachSign(veil, "World 1: Hidden Leaf", "Torii Battle Gate", PALETTE.GoldGlow, Vector3.new(0, 8.0, 0))
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
			primary = Color3.fromRGB(240, 246, 250),
			secondary = Color3.fromRGB(48, 176, 152),
			accent = Color3.fromRGB(255, 212, 82),
			style = "capsule",
		},
		{
			prop = "World3",
			name = "World3PirateArch",
			title = "World 3: Grand Line",
			subtitle = "Nautical Arch",
			pos = Vector3.new(15, 0.25, -33),
			primary = PALETTE.Navy,
			secondary = PALETTE.Brass,
			accent = Color3.fromRGB(64, 196, 255),
			style = "pirate",
		},
		{
			prop = "World4",
			name = "World4JujutsuTorii",
			title = "World 4: Tokyo Jujutsu",
			subtitle = "Cursed Torii",
			pos = Vector3.new(-14, 0.25, 34),
			primary = PALETTE.Obsidian,
			secondary = Color3.fromRGB(132, 34, 96),
			accent = Color3.fromRGB(178, 102, 255),
			style = "torii",
		},
		{
			prop = "World5",
			name = "World5SenkaimonGate",
			title = "World 5: Soul Society",
			subtitle = "Senkaimon Gate",
			pos = Vector3.new(14, 0.25, 34),
			primary = Color3.fromRGB(244, 242, 236),
			secondary = PALETTE.Gold,
			accent = Color3.fromRGB(255, 226, 148),
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
			-- Emerald Namekian twin spires flanking the sleek teal/white Capsule dome arch
			for _, xSpire in ipairs({ -5.5, 5.5 }) do
				makePart(
					model,
					"NamekSpire",
					Vector3.new(0.85, 6.8, 0.85),
					origin * CFrame.new(xSpire, 4.0, 0),
					Color3.fromRGB(52, 178, 108),
					Enum.Material.SmoothPlastic
				)
				makeBall(
					model,
					"NamekOrb",
					(origin * CFrame.new(xSpire, 7.9, 0)).Position,
					1.1,
					spec.accent,
					Enum.Material.Neon
				)
			end
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
			-- Deep navy & brass anchor accents + rope-gold trim
			for _, xSide in ipairs({ -3.8, 3.8 }) do
				makePart(
					model,
					"BrassAnchorRing",
					Vector3.new(1.75, 0.55, 1.75),
					origin * CFrame.new(xSide, 3.2, 0),
					spec.secondary,
					Enum.Material.Metal
				)
				makePart(
					model,
					"RopeGoldBand",
					Vector3.new(1.6, 0.4, 1.6),
					origin * CFrame.new(xSide, 6.2, 0),
					PALETTE.Gold,
					Enum.Material.Metal
				)
			end
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
			-- Dark obsidian & crimson-violet cursed torii + floating violet seal pillars
			for _, xSeal in ipairs({ -5.4, 5.4 }) do
				makePart(
					model,
					"CursedSealPillar",
					Vector3.new(0.75, 4.4, 0.75),
					origin * CFrame.new(xSeal, 4.6, 1.1),
					spec.accent,
					Enum.Material.Neon,
					0.2
				)
			end
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
			-- Luminous white-marble & brushed-gold Senkaimon gateway
			makePart(
				model,
				"Lintel",
				Vector3.new(10.8, 1.5, 1.9),
				origin * CFrame.new(0, 9.8, 0),
				spec.primary,
				Enum.Material.Marble
			)
			makePart(
				model,
				"GoldLintelBand",
				Vector3.new(11.0, 0.35, 2.05),
				origin * CFrame.new(0, 10.6, 0),
				spec.secondary,
				Enum.Material.Metal
			)
			makePart(
				model,
				"Crest",
				Vector3.new(3.0, 1.4, 2.0),
				origin * CFrame.new(0, 11.4, 0),
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
			body = Color3.fromRGB(238, 232, 218),
			accent = Color3.fromRGB(255, 204, 72),
			shape = "obelisk",
		},
		{
			prop = "Raid",
			name = "RaidBreachGate",
			title = "Raid Breach",
			subtitle = "Co-op Boss Raids",
			pos = Vector3.new(22, 0.35, -14),
			body = Color3.fromRGB(48, 28, 38),
			accent = Color3.fromRGB(255, 76, 92),
			shape = "gate",
		},
		{
			prop = "Ranked",
			name = "RankedArenaPlinth",
			title = "Ranked Arena",
			subtitle = "Async PvP Ladder",
			pos = Vector3.new(-22, 0.35, 22),
			body = Color3.fromRGB(234, 236, 244),
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
			-- Multi-tier golden Tower spire obelisk
			makePart(
				model,
				"ObeliskTier1",
				Vector3.new(3.6, 2.2, 3.6),
				origin * CFrame.new(0, 1.9, 0),
				sh.body,
				Enum.Material.Marble
			)
			makePart(
				model,
				"ObeliskTierGold1",
				Vector3.new(3.8, 0.3, 3.8),
				origin * CFrame.new(0, 3.1, 0),
				PALETTE.Gold,
				Enum.Material.Metal
			)
			anchor = makePart(
				model,
				"ObeliskShaft",
				Vector3.new(2.1, 6.8, 2.1),
				origin * CFrame.new(0, 6.6, 0),
				sh.body,
				Enum.Material.Marble
			)
			makePart(
				model,
				"ObeliskTip",
				Vector3.new(1.5, 1.5, 1.5),
				origin * CFrame.new(0, 10.8, 0) * CFrame.Angles(math.rad(45), math.rad(45), 0),
				sh.accent,
				Enum.Material.Neon
			)
		elseif sh.shape == "gate" then
			-- Crimson dimensional rift arch
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
			makePart(
				model,
				"RiftArchCrossbar",
				Vector3.new(5.6, 1.0, 1.4),
				origin * CFrame.new(0, 7.6, 0),
				sh.body,
				Enum.Material.Slate
			)
			anchor = makePart(
				model,
				"BreachCore",
				Vector3.new(3.2, 5.4, 0.5),
				origin * CFrame.new(0, 4.1, 0),
				sh.accent,
				Enum.Material.Neon,
				0.18
			)
		elseif sh.shape == "plinth" then
			-- Marble champion plinth with dual crossed banners
			makeUprightCylinder(
				model,
				"Column",
				sh.pos + Vector3.new(0, 2.8, 0),
				4.0,
				2.8,
				sh.body,
				Enum.Material.Marble
			)
			makePart(
				model,
				"BannerLeft",
				Vector3.new(1.1, 5.2, 0.18),
				origin * CFrame.new(-1.4, 4.4, -1.2) * CFrame.Angles(0, 0, math.rad(16)),
				PALETTE.Navy,
				Enum.Material.SmoothPlastic
			)
			makePart(
				model,
				"BannerRight",
				Vector3.new(1.1, 5.2, 0.18),
				origin * CFrame.new(1.4, 4.4, -1.2) * CFrame.Angles(0, 0, math.rad(-16)),
				PALETTE.Gold,
				Enum.Material.Metal
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
			-- Celestial lotus shrine with hovering lotus petals
			makeUprightCylinder(
				model,
				"AltarColumn",
				sh.pos + Vector3.new(0, 2.6, 0),
				3.6,
				2.8,
				sh.body,
				Enum.Material.Marble
			)
			for petalIdx = 1, 6 do
				local angle = math.rad((petalIdx - 1) * 60)
				local petalOffset = Vector3.new(math.cos(angle) * 2.5, 4.8, math.sin(angle) * 2.5)
				local petal = makePart(
					model,
					"LotusPetal_" .. tostring(petalIdx),
					Vector3.new(1.0, 1.8, 0.3),
					CFrame.lookAt(sh.pos + petalOffset, sh.pos + Vector3.new(0, 6.2, 0))
						* CFrame.Angles(math.rad(-25), 0, 0),
					PALETTE.LotusPink,
					Enum.Material.Neon,
					0.15
				)
				petal.CanCollide = false
			end
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
	buildShowcaseStandee(hub)
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
