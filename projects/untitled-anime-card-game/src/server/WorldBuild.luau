--!strict
-- Sunlit Nexus Plaza 3D Hub builder matching AssetIds.meshes stud dimensions.

local Lighting = game:GetService("Lighting")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Workspace = game:GetService("Workspace")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local AssetIds = require(Shared:WaitForChild("AssetIds"))

local WorldBuild = {}

local HUB_NAME = "NexusPlazaHub"

local function makePart(
	parent: Instance,
	name: string,
	size: Vector3,
	cframe: CFrame,
	color: Color3,
	material: Enum.Material,
	shape: Enum.PartType?,
	canCollide: boolean?,
	transparency: number?
): Part
	local part = Instance.new("Part")
	part.Name = name
	part.Size = size
	part.CFrame = cframe
	part.Color = color
	part.Material = material
	part.Shape = shape or Enum.PartType.Block
	part.Anchored = true
	part.CanCollide = if canCollide ~= nil then canCollide else true
	part.Transparency = transparency or 0
	part.TopSurface = Enum.SurfaceType.Smooth
	part.BottomSurface = Enum.SurfaceType.Smooth
	part.Parent = parent
	return part
end

local function addSurfaceLabel(parent: BasePart, face: Enum.NormalId, title: string, subtitle: string, accent: Color3)
	local gui = Instance.new("SurfaceGui")
	gui.Name = "SignGui"
	gui.Face = face
	gui.SizingMode = Enum.SurfaceGuiSizingMode.PixelsPerStud
	gui.PixelsPerStud = 36
	gui.Parent = parent

	local bg = Instance.new("Frame")
	bg.Size = UDim2.fromScale(1, 1)
	bg.BackgroundColor3 = Color3.fromRGB(16, 23, 42)
	bg.BorderSizePixel = 0
	bg.Parent = gui

	local stroke = Instance.new("UIStroke")
	stroke.Color = accent
	stroke.Thickness = 3
	stroke.Parent = bg

	local titleLabel = Instance.new("TextLabel")
	titleLabel.Size = UDim2.new(1, -12, 0.56, 0)
	titleLabel.Position = UDim2.fromOffset(6, 4)
	titleLabel.BackgroundTransparency = 1
	titleLabel.Font = Enum.Font.GothamBold
	titleLabel.Text = title
	titleLabel.TextColor3 = Color3.fromRGB(255, 250, 235)
	titleLabel.TextScaled = true
	titleLabel.Parent = bg

	local subLabel = Instance.new("TextLabel")
	subLabel.Size = UDim2.new(1, -12, 0.36, 0)
	subLabel.Position = UDim2.new(0, 6, 0.6, 0)
	subLabel.BackgroundTransparency = 1
	subLabel.Font = Enum.Font.GothamMedium
	subLabel.Text = subtitle
	subLabel.TextColor3 = accent
	subLabel.TextScaled = true
	subLabel.Parent = bg
end

local function addPrompt(
	anchor: BasePart,
	actionName: string,
	actionText: string,
	objectText: string,
	distance: number
): ProximityPrompt
	local prompt = Instance.new("ProximityPrompt")
	prompt.Name = actionName .. "Prompt"
	prompt.ActionText = actionText
	prompt.ObjectText = objectText
	prompt.HoldDuration = 0
	prompt.MaxActivationDistance = distance
	prompt.RequiresLineOfSight = false
	prompt:SetAttribute("PropAction", actionName)
	prompt.Parent = anchor
	return prompt
end

local function setupLighting()
	Lighting.ClockTime = 14.5
	Lighting.Brightness = 2.4
	Lighting.GlobalShadows = true
	Lighting.Ambient = Color3.fromRGB(122, 138, 168)
	Lighting.OutdoorAmbient = Color3.fromRGB(145, 168, 200)
	Lighting.ColorShift_Top = Color3.fromRGB(255, 246, 224)
	Lighting.EnvironmentDiffuseScale = 0.75
	Lighting.EnvironmentSpecularScale = 0.8

	for _, child in ipairs(Lighting:GetChildren()) do
		if
			child:IsA("Sky")
			or child:IsA("Atmosphere")
			or child:IsA("BloomEffect")
			or child:IsA("ColorCorrectionEffect")
			or child:IsA("SunRaysEffect")
		then
			child:Destroy()
		end
	end

	local sky = Instance.new("Sky")
	sky.Name = "NexusCeruleanSky"
	sky.CelestialBodiesShown = true
	sky.SunAngularSize = 18
	sky.Parent = Lighting

	local atmosphere = Instance.new("Atmosphere")
	atmosphere.Name = "NexusAtmosphere"
	atmosphere.Density = 0.24
	atmosphere.Offset = 0.14
	atmosphere.Color = Color3.fromRGB(186, 224, 255)
	atmosphere.Decay = Color3.fromRGB(115, 172, 238)
	atmosphere.Glare = 0.22
	atmosphere.Haze = 1.1
	atmosphere.Parent = Lighting

	local bloom = Instance.new("BloomEffect")
	bloom.Intensity = 0.35
	bloom.Size = 20
	bloom.Threshold = 1.1
	bloom.Parent = Lighting

	local cc = Instance.new("ColorCorrectionEffect")
	cc.Saturation = 0.12
	cc.Contrast = 0.06
	cc.TintColor = Color3.fromRGB(255, 252, 246)
	cc.Parent = Lighting

	local sunRays = Instance.new("SunRaysEffect")
	sunRays.Intensity = 0.08
	sunRays.Spread = 0.6
	sunRays.Parent = Lighting
end

local function buildRollAltar(parent: Instance, origin: Vector3): ProximityPrompt
	local specSize = AssetIds.meshes["roll_altar"] and AssetIds.meshes["roll_altar"].size
		or Vector3.new(4.771, 2.650, 4.771)
	local model = Instance.new("Model")
	model.Name = "roll_altar"
	model.Parent = parent

	-- Upright Cylinder base plinth matching 4.771 stud diameter
	local baseHeight = 0.55
	local base = makePart(
		model,
		"AltarBase",
		Vector3.new(baseHeight, specSize.X, specSize.Z),
		CFrame.new(origin + Vector3.new(0, baseHeight * 0.5, 0)) * CFrame.Angles(0, 0, math.rad(90)),
		Color3.fromRGB(238, 240, 248),
		Enum.Material.Marble,
		Enum.PartType.Cylinder,
		true,
		0
	)
	model.PrimaryPart = base

	-- Glowing summoning ring (upright Cylinder)
	makePart(
		model,
		"SummonGlowRing",
		Vector3.new(0.14, specSize.X * 0.88, specSize.Z * 0.88),
		CFrame.new(origin + Vector3.new(0, baseHeight + 0.05, 0)) * CFrame.Angles(0, 0, math.rad(90)),
		Color3.fromRGB(76, 214, 255),
		Enum.Material.Neon,
		Enum.PartType.Cylinder,
		false,
		0.15
	)

	-- Gold tier pedestal
	makePart(
		model,
		"AltarMidColumn",
		Vector3.new(0.9, 2.4, 2.4),
		CFrame.new(origin + Vector3.new(0, 0.55 + 0.45, 0)) * CFrame.Angles(0, 0, math.rad(90)),
		Color3.fromRGB(240, 195, 78),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Cylinder,
		true,
		0
	)

	-- Floating core crystal at center anchor (y ~ 1.95, top ~ 2.65)
	local crystal = makePart(
		model,
		"SummonCrystal",
		Vector3.new(1.1, 1.35, 1.1),
		CFrame.new(origin + Vector3.new(0, specSize.Y - 0.68, 0)) * CFrame.Angles(math.rad(25), math.rad(45), 0),
		Color3.fromRGB(98, 232, 255),
		Enum.Material.Neon,
		Enum.PartType.Block,
		false,
		0.1
	)

	local light = Instance.new("PointLight")
	light.Color = Color3.fromRGB(98, 232, 255)
	light.Range = 18
	light.Brightness = 1.6
	light.Parent = crystal

	return addPrompt(base, "Roll", "Summon Card", "Roll Altar", 14)
end

local function buildWorld1Portal(parent: Instance, origin: Vector3): ProximityPrompt
	local specSize = AssetIds.meshes["world1_portal_arch"] and AssetIds.meshes["world1_portal_arch"].size
		or Vector3.new(6.180, 4.668, 2.100)
	local model = Instance.new("Model")
	model.Name = "world1_portal_arch"
	model.Parent = parent

	local pillarW = 1.15
	local pillarH = specSize.Y - 0.9
	local halfSpan = (specSize.X - pillarW) * 0.5

	local baseStep = makePart(
		model,
		"PortalThreshold",
		Vector3.new(specSize.X, 0.4, specSize.Z),
		CFrame.new(origin + Vector3.new(0, 0.2, 0)),
		Color3.fromRGB(234, 238, 245),
		Enum.Material.Marble,
		Enum.PartType.Block,
		true,
		0
	)
	model.PrimaryPart = baseStep

	makePart(
		model,
		"LeftPillar",
		Vector3.new(pillarW, pillarH, specSize.Z * 0.72),
		CFrame.new(origin + Vector3.new(-halfSpan, 0.4 + pillarH * 0.5, 0)),
		Color3.fromRGB(48, 128, 94),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)

	makePart(
		model,
		"RightPillar",
		Vector3.new(pillarW, pillarH, specSize.Z * 0.72),
		CFrame.new(origin + Vector3.new(halfSpan, 0.4 + pillarH * 0.5, 0)),
		Color3.fromRGB(48, 128, 94),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)

	local lintel = makePart(
		model,
		"ArchLintel",
		Vector3.new(specSize.X, 0.9, specSize.Z * 0.85),
		CFrame.new(origin + Vector3.new(0, specSize.Y - 0.45, 0)),
		Color3.fromRGB(240, 192, 74),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)
	addSurfaceLabel(
		lintel,
		Enum.NormalId.Front,
		"WORLD 1: HIDDEN LEAF",
		"Act 1 & Ten-Tails Boss",
		Color3.fromRGB(95, 245, 175)
	)

	local portalMembrane = makePart(
		model,
		"PortalEnergy",
		Vector3.new(specSize.X - pillarW * 2, pillarH - 0.2, 0.22),
		CFrame.new(origin + Vector3.new(0, 0.4 + pillarH * 0.5, 0)),
		Color3.fromRGB(74, 242, 168),
		Enum.Material.Neon,
		Enum.PartType.Block,
		false,
		0.28
	)

	local light = Instance.new("PointLight")
	light.Color = Color3.fromRGB(74, 242, 168)
	light.Range = 20
	light.Brightness = 1.5
	light.Parent = portalMembrane

	return addPrompt(baseStep, "Battle", "Enter World 1", "Hidden Leaf Gate", 14)
end

local function buildCardStandee(parent: Instance, origin: Vector3): ProximityPrompt
	local specSize = AssetIds.meshes["card_standee"] and AssetIds.meshes["card_standee"].size
		or Vector3.new(2.478, 2.606, 2.478)
	local model = Instance.new("Model")
	model.Name = "card_standee"
	model.Parent = parent

	local base = makePart(
		model,
		"StandeePlinth",
		Vector3.new(specSize.X, 0.5, specSize.Z),
		CFrame.new(origin + Vector3.new(0, 0.25, 0)),
		Color3.fromRGB(235, 238, 246),
		Enum.Material.Marble,
		Enum.PartType.Block,
		true,
		0
	)
	model.PrimaryPart = base

	local cardFrame = makePart(
		model,
		"ShowcaseCard",
		Vector3.new(1.55, 2.05, 0.14),
		CFrame.new(origin + Vector3.new(0, specSize.Y - 1.05, 0)) * CFrame.Angles(math.rad(-8), 0, 0),
		Color3.fromRGB(242, 198, 72),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)
	addSurfaceLabel(cardFrame, Enum.NormalId.Front, "DECK & CARDS", "Build Active Squad", Color3.fromRGB(255, 210, 88))

	makePart(
		model,
		"StandeeHalo",
		Vector3.new(0.1, specSize.X * 0.9, specSize.Z * 0.9),
		CFrame.new(origin + Vector3.new(0, 0.55, 0)) * CFrame.Angles(0, 0, math.rad(90)),
		Color3.fromRGB(255, 208, 78),
		Enum.Material.Neon,
		Enum.PartType.Cylinder,
		false,
		0.25
	)

	return addPrompt(base, "Deck", "Open Deck Builder", "Card Standee", 12)
end

local function buildShopKiosk(parent: Instance, origin: Vector3): ProximityPrompt
	local specSize = AssetIds.meshes["shop_kiosk"] and AssetIds.meshes["shop_kiosk"].size
		or Vector3.new(3.800, 3.183, 2.700)
	local model = Instance.new("Model")
	model.Name = "shop_kiosk"
	model.Parent = parent

	local counter = makePart(
		model,
		"KioskCounter",
		Vector3.new(specSize.X, 1.25, specSize.Z * 0.8),
		CFrame.new(origin + Vector3.new(0, 0.625, 0)),
		Color3.fromRGB(168, 114, 72),
		Enum.Material.WoodPlanks,
		Enum.PartType.Block,
		true,
		0
	)
	model.PrimaryPart = counter

	local postOffsetX = specSize.X * 0.44
	local postOffsetZ = specSize.Z * 0.34
	for _, sx in ipairs({ -1, 1 }) do
		for _, sz in ipairs({ -1, 1 }) do
			makePart(
				model,
				"CanopyPost",
				Vector3.new(0.24, specSize.Y - 1.5, 0.24),
				CFrame.new(origin + Vector3.new(sx * postOffsetX, 1.25 + (specSize.Y - 1.5) * 0.5, sz * postOffsetZ)),
				Color3.fromRGB(238, 240, 246),
				Enum.Material.SmoothPlastic,
				Enum.PartType.Block,
				true,
				0
			)
		end
	end

	local roof = makePart(
		model,
		"CanopyRoof",
		Vector3.new(specSize.X, 0.55, specSize.Z),
		CFrame.new(origin + Vector3.new(0, specSize.Y - 0.275, 0)),
		Color3.fromRGB(64, 134, 232),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)
	addSurfaceLabel(roof, Enum.NormalId.Front, "NEXUS KIOSK", "Settings & Upgrades", Color3.fromRGB(115, 200, 255))

	return addPrompt(counter, "Settings", "Open Settings", "Nexus Kiosk", 12)
end

local function buildTree(parent: Instance, pos: Vector3, scale: number)
	local trunkH = 5.5 * scale
	makePart(
		parent,
		"TreeTrunk",
		Vector3.new(trunkH, 1.2 * scale, 1.2 * scale),
		CFrame.new(pos + Vector3.new(0, trunkH * 0.5, 0)) * CFrame.Angles(0, 0, math.rad(90)),
		Color3.fromRGB(122, 84, 54),
		Enum.Material.Wood,
		Enum.PartType.Cylinder,
		true,
		0
	)
	makePart(
		parent,
		"TreeCanopy",
		Vector3.new(5.4 * scale, 4.6 * scale, 5.4 * scale),
		CFrame.new(pos + Vector3.new(0, trunkH + 1.6 * scale, 0)),
		Color3.fromRGB(72, 168, 96),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Ball,
		false,
		0
	)
end

function WorldBuild.build(): { hub: Folder, prompts: { ProximityPrompt } }
	setupLighting()

	local oldBaseplate = Workspace:FindFirstChild("Baseplate")
	if oldBaseplate and oldBaseplate:IsA("BasePart") then
		oldBaseplate:Destroy()
	end

	local existing = Workspace:FindFirstChild(HUB_NAME)
	if existing then
		existing:Destroy()
	end

	local hub = Instance.new("Folder")
	hub.Name = HUB_NAME
	hub.Parent = Workspace

	-- Emerald meadow ground & water surround (zero bare grey baseplate)
	makePart(
		hub,
		"WaterHorizon",
		Vector3.new(420, 1, 420),
		CFrame.new(0, -2.5, 0),
		Color3.fromRGB(58, 166, 224),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0.15
	)
	makePart(
		hub,
		"EmeraldGround",
		Vector3.new(220, 2, 220),
		CFrame.new(0, -1, 0),
		Color3.fromRGB(72, 158, 92),
		Enum.Material.Grass,
		Enum.PartType.Block,
		true,
		0
	)

	-- Sunlit marble & gold plaza terraces
	makePart(
		hub,
		"MainMarblePlaza",
		Vector3.new(76, 1, 76),
		CFrame.new(0, 0.5, 0),
		Color3.fromRGB(240, 243, 250),
		Enum.Material.Marble,
		Enum.PartType.Block,
		true,
		0
	)
	makePart(
		hub,
		"PlazaGoldBorder",
		Vector3.new(78, 0.4, 78),
		CFrame.new(0, 0.15, 0),
		Color3.fromRGB(236, 190, 74),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)
	-- Raised north portal dais
	makePart(
		hub,
		"PortalDais",
		Vector3.new(26, 1, 14),
		CFrame.new(0, 1.5, -21),
		Color3.fromRGB(232, 238, 248),
		Enum.Material.Marble,
		Enum.PartType.Block,
		true,
		0
	)
	makePart(
		hub,
		"PortalSteps",
		Vector3.new(14, 0.5, 4),
		CFrame.new(0, 1.25, -12.5),
		Color3.fromRGB(236, 190, 74),
		Enum.Material.SmoothPlastic,
		Enum.PartType.Block,
		true,
		0
	)

	-- SpawnLocation facing -Z so player immediately sees Roll Altar & World 1 Portal
	local spawn = Instance.new("SpawnLocation")
	spawn.Name = "NexusSpawn"
	spawn.Size = Vector3.new(8, 1, 8)
	spawn.CFrame = CFrame.new(0, 1.1, 22)
	spawn.Anchored = true
	spawn.CanCollide = true
	spawn.Color = Color3.fromRGB(76, 152, 235)
	spawn.Material = Enum.Material.SmoothPlastic
	spawn.Neutral = true
	spawn.Duration = 0
	spawn.Parent = hub

	local prompts: { ProximityPrompt } = {}
	table.insert(prompts, buildRollAltar(hub, Vector3.new(0, 1, 2)))
	table.insert(prompts, buildWorld1Portal(hub, Vector3.new(0, 2, -21)))
	table.insert(prompts, buildCardStandee(hub, Vector3.new(-15, 1, 5)))
	table.insert(prompts, buildShopKiosk(hub, Vector3.new(15, 1, 5)))

	-- Stylized perimeter trees
	local treeSpots = {
		Vector3.new(-44, 0, -32),
		Vector3.new(44, 0, -32),
		Vector3.new(-46, 0, 18),
		Vector3.new(46, 0, 18),
		Vector3.new(-28, 0, -44),
		Vector3.new(28, 0, -44),
	}
	for idx, pos in ipairs(treeSpots) do
		buildTree(hub, pos, 0.9 + (idx % 3) * 0.18)
	end

	return {
		hub = hub,
		prompts = prompts,
	}
end

return WorldBuild
