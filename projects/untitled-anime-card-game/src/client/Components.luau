--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local AssetIds = require(Shared:WaitForChild("AssetIds"))
local Theme = require(script.Parent:WaitForChild("Theme"))

local Components = {}

local SUFFIXES = { "", "K", "M", "B", "T", "Qa", "Qi", "Sx" }

function Components.formatNumber(value: number): string
	if value ~= value then
		return "0"
	end
	local absVal = math.abs(value)
	local sign = if value < 0 then "-" else ""
	if absVal < 1000 then
		return sign .. tostring(math.floor(absVal + 0.5))
	end
	local tier = 1
	local scaled = absVal
	while scaled >= 1000 and tier < #SUFFIXES do
		scaled /= 1000
		tier += 1
	end
	local formatted: string
	if scaled >= 100 then
		formatted = string.format("%.0f", scaled)
	elseif scaled >= 10 then
		formatted = string.format("%.1f", scaled)
	else
		formatted = string.format("%.2f", scaled)
	end
	formatted = string.gsub(formatted, "%.?0+$", "")
	return sign .. formatted .. SUFFIXES[tier]
end

function Components.formatOdds(n: number): string
	if n <= 1 then
		return "1-in-1"
	end
	return "1-in-" .. Components.formatNumber(n)
end

function Components.addCorner(parent: GuiObject, radiusPx: number): UICorner
	local corner = Instance.new("UICorner")
	corner.CornerRadius = UDim.new(0, radiusPx)
	corner.Parent = parent
	return corner
end

function Components.addStroke(parent: GuiObject, color: Color3, thickness: number, transparency: number?): UIStroke
	local stroke = Instance.new("UIStroke")
	stroke.Color = color
	stroke.Thickness = thickness
	stroke.Transparency = transparency or 0
	stroke.ApplyStrokeMode = Enum.ApplyStrokeMode.Border
	stroke.Parent = parent
	return stroke
end

function Components.addPadding(parent: GuiObject, top: number, right: number, bottom: number, left: number): UIPadding
	local pad = Instance.new("UIPadding")
	pad.PaddingTop = UDim.new(0, top)
	pad.PaddingRight = UDim.new(0, right)
	pad.PaddingBottom = UDim.new(0, bottom)
	pad.PaddingLeft = UDim.new(0, left)
	pad.Parent = parent
	return pad
end

function Components.makePanel(parent: Instance, name: string, size: UDim2, pos: UDim2, radius: number?): Frame
	local palette = Theme.getPalette()
	local frame = Instance.new("Frame")
	frame.Name = name
	frame.Size = size
	frame.Position = pos
	frame.BackgroundColor3 = palette.bgPanel
	frame.BorderSizePixel = 0
	frame.Parent = parent
	Components.addCorner(frame, radius or 12)
	Components.addStroke(frame, palette.border, 1.5, 0.15)

	local sliceId = AssetIds.get("art/ui/ui_panel.png")
	if sliceId ~= "" then
		local bg = Instance.new("ImageLabel")
		bg.Name = "PanelSlice"
		bg.Size = UDim2.fromScale(1, 1)
		bg.BackgroundTransparency = 1
		bg.Image = sliceId
		bg.ImageTransparency = 0.85
		bg.ScaleType = Enum.ScaleType.Slice
		bg.SliceCenter = Rect.new(20, 20, 108, 108)
		bg.ZIndex = frame.ZIndex
		bg.Parent = frame
	end

	return frame
end

function Components.makeButton(
	parent: Instance,
	name: string,
	text: string,
	size: UDim2,
	pos: UDim2,
	bgColor: Color3,
	textColor: Color3
): TextButton
	local btn = Instance.new("TextButton")
	btn.Name = name
	btn.Size = size
	btn.Position = pos
	btn.BackgroundColor3 = bgColor
	btn.BorderSizePixel = 0
	btn.Font = Theme.FONT_DISPLAY
	btn.Text = text
	btn.TextColor3 = textColor
	btn.TextScaled = false
	btn.TextSize = 15
	btn.AutoButtonColor = true
	btn.Parent = parent

	Components.addCorner(btn, 10)
	Components.addStroke(btn, Color3.fromRGB(255, 255, 255), 1, 0.72)

	local sizeConstraint = Instance.new("UISizeConstraint")
	sizeConstraint.MinSize = Vector2.new(Theme.MIN_TOUCH_PX, Theme.MIN_TOUCH_PX)
	sizeConstraint.Parent = btn

	local uiScale = Instance.new("UIScale")
	uiScale.Name = "HoverScale"
	uiScale.Scale = 1
	uiScale.Parent = btn

	local hoverColor = bgColor:Lerp(Color3.fromRGB(255, 255, 255), 0.12)
	local pressColor = bgColor:Lerp(Color3.fromRGB(0, 0, 0), 0.14)

	btn.MouseEnter:Connect(function()
		TweenService:Create(btn, TweenInfo.new(0.12, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			BackgroundColor3 = hoverColor,
		}):Play()
		TweenService:Create(uiScale, TweenInfo.new(0.12, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Scale = 1.03,
		}):Play()
	end)

	btn.MouseLeave:Connect(function()
		TweenService:Create(btn, TweenInfo.new(0.14, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			BackgroundColor3 = bgColor,
		}):Play()
		TweenService:Create(uiScale, TweenInfo.new(0.14, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Scale = 1,
		}):Play()
	end)

	btn.MouseButton1Down:Connect(function()
		TweenService:Create(btn, TweenInfo.new(0.06, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			BackgroundColor3 = pressColor,
		}):Play()
		TweenService:Create(uiScale, TweenInfo.new(0.06, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Scale = 0.96,
		}):Play()
	end)

	btn.MouseButton1Up:Connect(function()
		TweenService:Create(btn, TweenInfo.new(0.1, Enum.EasingStyle.Back, Enum.EasingDirection.Out), {
			BackgroundColor3 = hoverColor,
		}):Play()
		TweenService:Create(uiScale, TweenInfo.new(0.1, Enum.EasingStyle.Back, Enum.EasingDirection.Out), {
			Scale = 1,
		}):Play()
	end)

	return btn
end

function Components.makeLabel(
	parent: Instance,
	name: string,
	text: string,
	size: UDim2,
	pos: UDim2,
	font: Enum.Font?,
	textSize: number?,
	color: Color3?,
	xAlign: Enum.TextXAlignment?
): TextLabel
	local palette = Theme.getPalette()
	local label = Instance.new("TextLabel")
	label.Name = name
	label.Size = size
	label.Position = pos
	label.BackgroundTransparency = 1
	label.Font = font or Theme.FONT_BODY
	label.Text = text
	label.TextSize = textSize or 14
	label.TextColor3 = color or palette.textPrimary
	label.TextXAlignment = xAlign or Enum.TextXAlignment.Left
	label.TextYAlignment = Enum.TextYAlignment.Center
	label.TextTruncate = Enum.TextTruncate.AtEnd
	label.Parent = parent
	return label
end

function Components.makeRarityBadge(parent: Instance, rarityName: string, pos: UDim2): Frame
	local style = Theme.getRarity(rarityName)
	local badge = Instance.new("Frame")
	badge.Name = "RarityBadge"
	badge.Size = UDim2.fromOffset(92, 22)
	badge.Position = pos
	badge.BackgroundColor3 = style.bgTint
	badge.BorderSizePixel = 0
	badge.Parent = parent
	Components.addCorner(badge, 6)
	Components.addStroke(badge, style.color, 1.5, 0.1)

	local shapePip = Instance.new("Frame")
	shapePip.Name = "ShapePip"
	shapePip.Size = UDim2.fromOffset(10, 10)
	shapePip.Position = UDim2.new(0, 6, 0.5, -5)
	shapePip.BackgroundColor3 = style.color
	shapePip.BorderSizePixel = 0
	shapePip.Parent = badge
	if style.order <= 2 then
		Components.addCorner(shapePip, 5)
	elseif style.order <= 4 then
		shapePip.Rotation = 45
		Components.addCorner(shapePip, 2)
	elseif style.order <= 6 then
		shapePip.Rotation = 45
		Components.addStroke(shapePip, style.glow, 1, 0.2)
	else
		Components.addCorner(shapePip, 3)
		Components.addStroke(shapePip, style.glow, 1.5, 0)
	end

	local label = Components.makeLabel(
		badge,
		"Label",
		string.upper(style.name),
		UDim2.new(1, -22, 1, 0),
		UDim2.fromOffset(20, 0),
		Theme.FONT_DISPLAY,
		11,
		style.glow,
		Enum.TextXAlignment.Left
	)
	label.TextTruncate = Enum.TextTruncate.None

	return badge
end

function Components.makeCardView(parent: Instance, cardData: { [string]: any }, size: UDim2, pos: UDim2): Frame
	local palette = Theme.getPalette()
	local rarityName = tostring(cardData.rarity or "Common")
	local mutationName = tostring(cardData.mutation or "Normal")
	local rarity = Theme.getRarity(rarityName)
	local mutation = Theme.getMutation(mutationName)

	local frame = Instance.new("Frame")
	frame.Name = "Card_" .. tostring(cardData.uid or cardData.id or "Unit")
	frame.Size = size
	frame.Position = pos
	frame.BackgroundColor3 = palette.bgCard
	frame.BorderSizePixel = 0
	frame.ClipsDescendants = true
	frame.Parent = parent
	Components.addCorner(frame, 10)
	Components.addStroke(frame, rarity.color, 2, 0.05)

	local frameKey = "art/ui/frame_r" .. tostring(math.clamp(rarity.order, 1, 6)) .. ".png"
	local frameAsset = AssetIds.get(frameKey)
	if frameAsset ~= "" then
		local borderImg = Instance.new("ImageLabel")
		borderImg.Name = "RarityFrame"
		borderImg.Size = UDim2.fromScale(1, 1)
		borderImg.BackgroundTransparency = 1
		borderImg.Image = frameAsset
		borderImg.ScaleType = Enum.ScaleType.Slice
		borderImg.SliceCenter = Rect.new(28, 28, 228, 328)
		borderImg.ZIndex = 3
		borderImg.Parent = frame
	end

	local portraitBox = Instance.new("Frame")
	portraitBox.Name = "PortraitBox"
	portraitBox.Size = UDim2.new(1, -10, 0.56, 0)
	portraitBox.Position = UDim2.fromOffset(5, 5)
	portraitBox.BackgroundColor3 = rarity.bgTint
	portraitBox.BorderSizePixel = 0
	portraitBox.Parent = frame
	Components.addCorner(portraitBox, 7)

	local grad = Instance.new("UIGradient")
	grad.Color = ColorSequence.new({
		ColorSequenceKeypoint.new(0, rarity.bgTint),
		ColorSequenceKeypoint.new(1, rarity.color),
	})
	grad.Rotation = 135
	grad.Transparency = NumberSequence.new(0.25)
	grad.Parent = portraitBox

	local cardId = tostring(cardData.cardId or cardData.id or "")
	local portraitAsset = if cardData.artKey then AssetIds.get(tostring(cardData.artKey)) else ""
	if portraitAsset == "" and cardId ~= "" then
		portraitAsset = AssetIds.get("cards/" .. cardId)
	end
	if portraitAsset == "" and cardId ~= "" then
		portraitAsset = AssetIds.get(cardId)
	end
	if portraitAsset == "" and cardId ~= "" then
		portraitAsset = AssetIds.get("art/cards/" .. cardId .. "/art.png")
	end
	local portraitImage = Instance.new("ImageLabel")
	portraitImage.Name = "PortraitImage"
	portraitImage.Size = UDim2.fromScale(1, 1)
	portraitImage.BackgroundTransparency = 1
	portraitImage.ScaleType = Enum.ScaleType.Crop
	portraitImage.ZIndex = 2
	portraitImage.Parent = portraitBox
	Components.addCorner(portraitImage, 7)

	local hasLivePortrait = string.match(portraitAsset, "^rbxassetid://[1-9]%d*$") ~= nil
	if hasLivePortrait then
		portraitImage.Image = portraitAsset
		portraitImage.Visible = true
	else
		portraitImage.Visible = false
		local roleText = tostring(cardData.role or "Striker")

		local isCompact = size.Y.Offset > 0 and size.Y.Offset <= 140
		local crestSize = if isCompact then 30 else 58

		local crest = Instance.new("Frame")
		crest.Name = "RoleCrest"
		crest.Size = UDim2.fromOffset(crestSize, crestSize)
		crest.Position = if isCompact then UDim2.new(1, -24, 0.5, 0) else UDim2.fromScale(0.5, 0.44)
		crest.AnchorPoint = Vector2.new(0.5, 0.5)
		crest.BackgroundColor3 = rarity.bgTint:Lerp(rarity.color, 0.28)
		crest.BorderSizePixel = 0
		crest.ZIndex = 2
		crest.Parent = portraitBox
		Components.addCorner(crest, math.floor(crestSize / 2))
		Components.addStroke(crest, rarity.glow, 2, 0.1)

		local emblem = Components.makeLabel(
			crest,
			"Emblem",
			rarity.shortTag,
			UDim2.fromScale(1, 1),
			UDim2.fromOffset(0, 0),
			Theme.FONT_DISPLAY,
			if isCompact then 13 else 24,
			rarity.glow,
			Enum.TextXAlignment.Center
		)
		emblem.ZIndex = 3
		emblem.TextStrokeTransparency = 0.35

		local cornerTL = Instance.new("Frame")
		cornerTL.Name = "MetallicAccentTL"
		cornerTL.Size = UDim2.fromOffset(14, 3)
		cornerTL.Position = UDim2.fromOffset(6, 6)
		cornerTL.BackgroundColor3 = rarity.glow
		cornerTL.BorderSizePixel = 0
		cornerTL.ZIndex = 2
		cornerTL.Parent = portraitBox

		local cornerBR = Instance.new("Frame")
		cornerBR.Name = "MetallicAccentBR"
		cornerBR.Size = UDim2.fromOffset(14, 3)
		cornerBR.Position = UDim2.new(1, -20, 1, -9)
		cornerBR.BackgroundColor3 = rarity.glow
		cornerBR.BorderSizePixel = 0
		cornerBR.ZIndex = 2
		cornerBR.Parent = portraitBox

		local roleLbl = Components.makeLabel(
			portraitBox,
			"RoleTag",
			string.upper(roleText),
			UDim2.new(1, -8, 0, 18),
			UDim2.new(0, 4, 1, -22),
			Theme.FONT_DISPLAY,
			10,
			Color3.fromRGB(245, 248, 255),
			Enum.TextXAlignment.Center
		)
		roleLbl.ZIndex = 3
	end

	local badge = Components.makeRarityBadge(frame, rarityName, UDim2.fromOffset(6, 8))
	badge.ZIndex = 4

	if mutationName ~= "Normal" then
		local mutPill = Instance.new("Frame")
		mutPill.Name = "MutationPill"
		mutPill.Size = UDim2.fromOffset(66, 20)
		mutPill.Position = UDim2.new(1, -72, 0, 9)
		mutPill.BackgroundColor3 = mutation.secondary
		mutPill.BorderSizePixel = 0
		mutPill.ZIndex = 4
		mutPill.Parent = frame
		Components.addCorner(mutPill, 5)
		Components.addStroke(mutPill, mutation.color, 1.5, 0)
		Components.makeLabel(
			mutPill,
			"Text",
			mutation.tag,
			UDim2.fromScale(1, 1),
			UDim2.fromOffset(0, 0),
			Theme.FONT_DISPLAY,
			9,
			mutation.color,
			Enum.TextXAlignment.Center
		).ZIndex =
			4
	end

	local level = tonumber(cardData.level) or 1
	local stars = tonumber(cardData.stars) or 0
	local displayName = tostring(cardData.name or cardData.displayName or "Card")

	local nameLbl = Components.makeLabel(
		frame,
		"CardName",
		displayName,
		UDim2.new(1, -12, 0, 20),
		UDim2.new(0, 6, 0.58, 4),
		Theme.FONT_DISPLAY,
		13,
		palette.textPrimary,
		Enum.TextXAlignment.Left
	)
	nameLbl.ZIndex = 2

	local starText = if stars > 0 then string.rep("*", stars) .. " " else ""
	local metaLbl = Components.makeLabel(
		frame,
		"Meta",
		string.format("Lv.%d %s| %s", level, starText, tostring(cardData.role or "Striker")),
		UDim2.new(1, -12, 0, 16),
		UDim2.new(0, 6, 0.58, 24),
		Theme.FONT_BODY,
		11,
		palette.textSecondary,
		Enum.TextXAlignment.Left
	)
	metaLbl.ZIndex = 2

	local hp = tonumber(cardData.hp) or 0
	local atk = tonumber(cardData.atk) or 0
	local spd = tonumber(cardData.spd) or 0
	local statsText = string.format(
		"HP %s  ATK %s  SPD %s",
		Components.formatNumber(hp),
		Components.formatNumber(atk),
		Components.formatNumber(spd)
	)
	local statsLbl = Components.makeLabel(
		frame,
		"Stats",
		statsText,
		UDim2.new(1, -12, 0, 18),
		UDim2.new(0, 6, 1, -24),
		Theme.FONT_MONO,
		10,
		palette.accentGold,
		Enum.TextXAlignment.Left
	)
	statsLbl.ZIndex = 2

	return frame
end

return Components
