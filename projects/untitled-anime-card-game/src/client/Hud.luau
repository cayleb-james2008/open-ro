--!strict

local Workspace = game:GetService("Workspace")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type HudCallbacks = {
	onRoll: () -> (),
	onToggleAuto: () -> (),
	onToggleFast: () -> (),
	onOpenDeck: () -> (),
	onOpenBattle: () -> (),
	onOpenSettings: () -> (),
}

export type HudState = {
	yen: number,
	gems: number,
	tickets: number,
	luckMultiplier: number,
	luckBreakdown: string?,
	luckSummary: string?,
	legendaryPity: number,
	legendaryPityMax: number,
	mythicPity: number,
	mythicPityMax: number,
	rollCostYen: number,
	autoEnabled: boolean,
	fastEnabled: boolean,
	nextGoal: string,
	deckCount: number,
	deckMax: number,
	rollStatusText: string?,
}

export type HudController = {
	update: (state: HudState) -> (),
	setVisible: (visible: boolean) -> (),
	destroy: () -> (),
}

local Hud = {}

function Hud.create(parent: Instance, callbacks: HudCallbacks): HudController
	local root = Instance.new("Frame")
	root.Name = "HudRoot"
	root.Size = UDim2.fromScale(1, 1)
	root.BackgroundTransparency = 1
	root.ZIndex = 5
	root.Parent = parent

	local hudScale = Instance.new("UIScale")
	hudScale.Name = "ResponsiveScale"
	hudScale.Scale = 1
	hudScale.Parent = root

	local viewportConn: RBXScriptConnection? = nil
	local cameraConn: RBXScriptConnection? = nil

	local layoutUpdater: ((viewportX: number) -> ())? = nil

	local function applyViewportScale()
		local cam = Workspace.CurrentCamera
		if not cam then
			return
		end
		local viewport = cam.ViewportSize
		if viewport.X <= 0 then
			return
		end
		-- Keep UIScale at 1.0 across mobile and desktop so touch targets never shrink below 44px
		hudScale.Scale = 1
		root.Size = UDim2.fromScale(1, 1)
		if layoutUpdater then
			layoutUpdater(viewport.X)
		end
	end

	local function bindCamera(cam: Camera?)
		if viewportConn then
			viewportConn:Disconnect()
			viewportConn = nil
		end
		if cam then
			viewportConn = cam:GetPropertyChangedSignal("ViewportSize"):Connect(applyViewportScale)
			applyViewportScale()
		end
	end

	cameraConn = Workspace:GetPropertyChangedSignal("CurrentCamera"):Connect(function()
		bindCamera(Workspace.CurrentCamera)
	end)
	bindCamera(Workspace.CurrentCamera)

	root.Destroying:Connect(function()
		if viewportConn then
			viewportConn:Disconnect()
			viewportConn = nil
		end
		if cameraConn then
			cameraConn:Disconnect()
			cameraConn = nil
		end
	end)

	-- Top-left resource stack (Currencies + Luck chip + Next Goal banner)
	local topLeft = Instance.new("Frame")
	topLeft.Name = "TopLeftStack"
	topLeft.Size = UDim2.fromOffset(330, 128)
	topLeft.Position = UDim2.fromOffset(12, 10)
	topLeft.BackgroundTransparency = 1
	topLeft.ZIndex = 6
	topLeft.Parent = root

	local topLeftLayout = Instance.new("UIListLayout")
	topLeftLayout.SortOrder = Enum.SortOrder.LayoutOrder
	topLeftLayout.Padding = UDim.new(0, 6)
	topLeftLayout.Parent = topLeft

	local currencyRow = Instance.new("Frame")
	currencyRow.Name = "CurrencyRow"
	currencyRow.LayoutOrder = 1
	currencyRow.Size = UDim2.new(1, 0, 0, 38)
	currencyRow.BackgroundTransparency = 1
	currencyRow.ZIndex = 6
	currencyRow.Parent = topLeft

	local currencyLayout = Instance.new("UIListLayout")
	currencyLayout.FillDirection = Enum.FillDirection.Horizontal
	currencyLayout.SortOrder = Enum.SortOrder.LayoutOrder
	currencyLayout.Padding = UDim.new(0, 6)
	currencyLayout.Parent = currencyRow

	local themedPills: { Frame } = {}
	local themedTexts: { TextLabel } = {}

	local function makePill(order: number, name: string, tag: string, accent: Color3, width: number): (Frame, TextLabel)
		local palette = Theme.getPalette()
		local pill = Instance.new("Frame")
		pill.Name = name
		pill.LayoutOrder = order
		pill.Size = UDim2.fromOffset(width, 36)
		pill.BackgroundColor3 = palette.bgPanel
		pill.BorderSizePixel = 0
		pill.ZIndex = 6
		pill.Parent = currencyRow
		Components.addCorner(pill, 10)
		Components.addStroke(pill, palette.border, 1.5, 0.15)

		local dot = Instance.new("Frame")
		dot.Name = "Dot"
		dot.Size = UDim2.fromOffset(10, 10)
		dot.Position = UDim2.new(0, 10, 0.5, -5)
		dot.BackgroundColor3 = accent
		dot.BorderSizePixel = 0
		dot.ZIndex = 7
		dot.Parent = pill
		Components.addCorner(dot, 5)

		local lbl = Components.makeLabel(
			pill,
			"Value",
			tag .. " 0",
			UDim2.new(1, -28, 1, 0),
			UDim2.fromOffset(24, 0),
			Theme.FONT_MONO,
			13,
			palette.textPrimary,
			Enum.TextXAlignment.Left
		)
		lbl.ZIndex = 7

		table.insert(themedPills, pill)
		table.insert(themedTexts, lbl)
		return pill, lbl
	end

	local yenPill, yenLabel = makePill(1, "YenPill", "YEN", Theme.getPalette().accentGold, 114)
	local gemsPill, gemsLabel = makePill(2, "GemsPill", "GEMS", Theme.getPalette().accentBlue, 100)
	local ticketsPill, ticketsLabel = makePill(3, "TicketsPill", "TIX", Theme.getPalette().accentPurple, 88)

	local luckChip = Instance.new("Frame")
	luckChip.Name = "LuckChip"
	luckChip.LayoutOrder = 2
	luckChip.Size = UDim2.new(1, 0, 0, 32)
	luckChip.BackgroundColor3 = Theme.getPalette().bgPanel
	luckChip.BorderSizePixel = 0
	luckChip.ZIndex = 6
	luckChip.Parent = topLeft
	Components.addCorner(luckChip, 8)
	Components.addStroke(luckChip, Theme.getPalette().accentGreen, 1.5, 0.35)
	table.insert(themedPills, luckChip)

	local luckLabel = Components.makeLabel(
		luckChip,
		"LuckText",
		"Luck 1.00x (Base 1.0x)",
		UDim2.new(1, -16, 1, 0),
		UDim2.fromOffset(10, 0),
		Theme.FONT_BODY,
		11,
		Theme.getPalette().accentGreen,
		Enum.TextXAlignment.Left
	)
	luckLabel.ZIndex = 7

	local goalChip = Instance.new("Frame")
	goalChip.Name = "GoalChip"
	goalChip.LayoutOrder = 3
	goalChip.Size = UDim2.new(1, 0, 0, 34)
	goalChip.BackgroundColor3 = Theme.getPalette().bgPanel
	goalChip.BorderSizePixel = 0
	goalChip.ZIndex = 6
	goalChip.Parent = topLeft
	Components.addCorner(goalChip, 8)
	Components.addStroke(goalChip, Theme.getPalette().border, 1.5, 0.2)
	table.insert(themedPills, goalChip)

	local goalLabel = Components.makeLabel(
		goalChip,
		"GoalText",
		"Next: Summon your first card!",
		UDim2.new(1, -16, 1, 0),
		UDim2.fromOffset(10, 0),
		Theme.FONT_DISPLAY,
		12,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	goalLabel.ZIndex = 7
	table.insert(themedTexts, goalLabel)

	-- Top-right navigation cluster (>=44px touch targets)
	local topRight = Instance.new("Frame")
	topRight.Name = "TopRightNav"
	topRight.Size = UDim2.fromOffset(330, 52)
	topRight.Position = UDim2.new(1, -12, 0, 10)
	topRight.AnchorPoint = Vector2.new(1, 0)
	topRight.BackgroundTransparency = 1
	topRight.ZIndex = 6
	topRight.Parent = root

	local navLayout = Instance.new("UIListLayout")
	navLayout.FillDirection = Enum.FillDirection.Horizontal
	navLayout.HorizontalAlignment = Enum.HorizontalAlignment.Right
	navLayout.SortOrder = Enum.SortOrder.LayoutOrder
	navLayout.Padding = UDim.new(0, 8)
	navLayout.Parent = topRight

	local deckBtn = Components.makeButton(
		topRight,
		"DeckButton",
		"Deck (0/5)",
		UDim2.fromOffset(108, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgPanel,
		Theme.getPalette().textPrimary
	)
	deckBtn.LayoutOrder = 1
	deckBtn.TextSize = 13
	deckBtn.ZIndex = 7

	local battleBtn = Components.makeButton(
		topRight,
		"BattleButton",
		"Battle W1",
		UDim2.fromOffset(108, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	battleBtn.LayoutOrder = 2
	battleBtn.TextSize = 13
	battleBtn.ZIndex = 7

	local settingsBtn = Components.makeButton(
		topRight,
		"SettingsButton",
		"Settings",
		UDim2.fromOffset(88, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgPanel,
		Theme.getPalette().textPrimary
	)
	settingsBtn.LayoutOrder = 3
	settingsBtn.TextSize = 13
	settingsBtn.ZIndex = 7

	-- Bottom-center Roll & Pity dock
	local rollDock = Components.makePanel(root, "RollDock", UDim2.fromOffset(396, 108), UDim2.new(0.5, 0, 1, -14), 14)
	rollDock.AnchorPoint = Vector2.new(0.5, 1)
	rollDock.ZIndex = 6

	local dockConstraint = Instance.new("UISizeConstraint")
	dockConstraint.MinSize = Vector2.new(300, 100)
	dockConstraint.MaxSize = Vector2.new(430, 114)
	dockConstraint.Parent = rollDock

	local pityLabel = Components.makeLabel(
		rollDock,
		"PityLabel",
		"Pity: Legendary 0/50  |  Mythic 0/250",
		UDim2.new(1, -20, 0, 22),
		UDim2.fromOffset(10, 6),
		Theme.FONT_MONO,
		11,
		Theme.getPalette().textSecondary,
		Enum.TextXAlignment.Center
	)
	pityLabel.ZIndex = 7

	local statusLabel = Components.makeLabel(
		rollDock,
		"StatusLabel",
		"",
		UDim2.new(1, -20, 0, 16),
		UDim2.fromOffset(10, 26),
		Theme.FONT_BODY,
		11,
		Theme.getPalette().accentGold,
		Enum.TextXAlignment.Center
	)
	statusLabel.ZIndex = 7

	local btnRow = Instance.new("Frame")
	btnRow.Name = "ButtonRow"
	btnRow.Size = UDim2.new(1, -20, 0, 50)
	btnRow.Position = UDim2.fromOffset(10, 48)
	btnRow.BackgroundTransparency = 1
	btnRow.ZIndex = 7
	btnRow.Parent = rollDock

	local autoBtn = Components.makeButton(
		btnRow,
		"AutoButton",
		"AUTO: OFF",
		UDim2.new(0.26, 0, 0, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	autoBtn.TextSize = 12
	autoBtn.ZIndex = 8

	local rollBtn = Components.makeButton(
		btnRow,
		"RollButton",
		"ROLL (FREE)",
		UDim2.new(0.44, 0, 0, 46),
		UDim2.fromScale(0.28, 0),
		Theme.getPalette().accentGold,
		Theme.getPalette().textOnAccent
	)
	rollBtn.TextSize = 15
	rollBtn.ZIndex = 8

	local fastBtn = Components.makeButton(
		btnRow,
		"FastButton",
		"FAST: OFF",
		UDim2.new(0.26, 0, 0, 46),
		UDim2.fromScale(0.74, 0),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	fastBtn.TextSize = 12
	fastBtn.ZIndex = 8

	layoutUpdater = function(viewportX: number)
		local isMobile = viewportX < 700
		if isMobile then
			local usableW = math.max(viewportX - 16, 300)
			topLeft.Position = UDim2.fromOffset(8, 8)
			topLeft.Size = UDim2.fromOffset(usableW, 100)
			topLeftLayout.Padding = UDim.new(0, 4)

			currencyRow.Size = UDim2.new(1, 0, 0, 34)
			local pillGap = 6
			local pillW = math.floor((usableW - pillGap * 2) / 3)
			yenPill.Size = UDim2.fromOffset(pillW, 34)
			gemsPill.Size = UDim2.fromOffset(pillW, 34)
			ticketsPill.Size = UDim2.fromOffset(usableW - pillW * 2 - pillGap * 2, 34)

			luckChip.Size = UDim2.new(1, 0, 0, 28)
			goalChip.Size = UDim2.new(1, 0, 0, 28)

			-- Row 3: compact >=44px nav buttons cleanly below topLeft
			topRight.AnchorPoint = Vector2.new(0, 0)
			topRight.Position = UDim2.fromOffset(8, 112)
			topRight.Size = UDim2.fromOffset(usableW, 46)
			navLayout.HorizontalAlignment = Enum.HorizontalAlignment.Left
			navLayout.Padding = UDim.new(0, 6)
			local btnGap = 6
			local btnW = math.floor((usableW - btnGap * 2) / 3)
			deckBtn.Size = UDim2.fromOffset(btnW, 46)
			battleBtn.Size = UDim2.fromOffset(btnW, 46)
			settingsBtn.Size = UDim2.fromOffset(usableW - btnW * 2 - btnGap * 2, 46)

			rollDock.Size = UDim2.fromOffset(math.min(usableW, 400), 104)
			rollDock.Position = UDim2.new(0.5, 0, 1, -10)
			dockConstraint.MinSize = Vector2.new(280, 100)
		else
			topLeft.Position = UDim2.fromOffset(12, 10)
			topLeft.Size = UDim2.fromOffset(330, 128)
			topLeftLayout.Padding = UDim.new(0, 6)

			currencyRow.Size = UDim2.new(1, 0, 0, 38)
			yenPill.Size = UDim2.fromOffset(114, 36)
			gemsPill.Size = UDim2.fromOffset(100, 36)
			ticketsPill.Size = UDim2.fromOffset(88, 36)

			luckChip.Size = UDim2.new(1, 0, 0, 32)
			goalChip.Size = UDim2.new(1, 0, 0, 34)

			topRight.AnchorPoint = Vector2.new(1, 0)
			topRight.Position = UDim2.new(1, -12, 0, 10)
			topRight.Size = UDim2.fromOffset(330, 52)
			navLayout.HorizontalAlignment = Enum.HorizontalAlignment.Right
			navLayout.Padding = UDim.new(0, 8)
			deckBtn.Size = UDim2.fromOffset(108, 46)
			battleBtn.Size = UDim2.fromOffset(108, 46)
			settingsBtn.Size = UDim2.fromOffset(88, 46)

			rollDock.Size = UDim2.fromOffset(396, 108)
			rollDock.Position = UDim2.new(0.5, 0, 1, -14)
			dockConstraint.MinSize = Vector2.new(300, 100)
		end
	end
	applyViewportScale()

	local lastState: HudState = {
		yen = 0,
		gems = 0,
		tickets = 0,
		luckMultiplier = 1,
		luckBreakdown = "Base 1.0x",
		legendaryPity = 0,
		legendaryPityMax = 50,
		mythicPity = 0,
		mythicPityMax = 250,
		rollCostYen = 0,
		autoEnabled = false,
		fastEnabled = false,
		nextGoal = "Summon your first card!",
		deckCount = 0,
		deckMax = 5,
		rollStatusText = "",
	}

	local function applyTheme()
		local palette = Theme.getPalette()
		for _, pill in themedPills do
			pill.BackgroundColor3 = palette.bgPanel
			local st = pill:FindFirstChildOfClass("UIStroke")
			if st and pill ~= luckChip then
				st.Color = palette.border
			end
		end
		for _, lbl in themedTexts do
			lbl.TextColor3 = palette.textPrimary
		end
		luckLabel.TextColor3 = palette.accentGreen

		deckBtn.BackgroundColor3 = palette.bgPanel
		deckBtn.TextColor3 = palette.textPrimary
		settingsBtn.BackgroundColor3 = palette.bgPanel
		settingsBtn.TextColor3 = palette.textPrimary

		rollDock.BackgroundColor3 = palette.bgPanel
		local dockStroke = rollDock:FindFirstChildOfClass("UIStroke")
		if dockStroke then
			dockStroke.Color = palette.border
		end
		pityLabel.TextColor3 = palette.textSecondary
		statusLabel.TextColor3 = palette.accentGold

		rollBtn.BackgroundColor3 = palette.accentGold
		rollBtn.TextColor3 = palette.textOnAccent

		autoBtn.BackgroundColor3 = if lastState.autoEnabled then palette.accentGreen else palette.bgMuted
		autoBtn.TextColor3 = if lastState.autoEnabled then palette.textOnAccent else palette.textPrimary

		fastBtn.BackgroundColor3 = if lastState.fastEnabled then palette.accentPurple else palette.bgMuted
		fastBtn.TextColor3 = if lastState.fastEnabled then Color3.fromRGB(255, 255, 255) else palette.textPrimary
	end

	local function renderState(state: HudState)
		lastState = state
		yenLabel.Text = "YEN " .. Components.formatNumber(state.yen)
		gemsLabel.Text = "GEMS " .. Components.formatNumber(state.gems)
		ticketsLabel.Text = "TIX " .. Components.formatNumber(state.tickets)

		local summary = state.luckSummary or state.luckBreakdown or "Base 1.0x"
		if summary ~= "" then
			luckLabel.Text = string.format("Luck %.2fx (%s)", state.luckMultiplier, summary)
		else
			luckLabel.Text = string.format("Luck %.2fx", state.luckMultiplier)
		end
		goalLabel.Text = "Next: " .. state.nextGoal
		deckBtn.Text = string.format("Deck (%d/%d)", state.deckCount, state.deckMax)

		pityLabel.Text = string.format(
			"Pity: Legendary %d/%d  |  Mythic %d/%d",
			state.legendaryPity,
			state.legendaryPityMax,
			state.mythicPity,
			state.mythicPityMax
		)
		statusLabel.Text = state.rollStatusText or ""

		if state.rollCostYen <= 0 then
			rollBtn.Text = "ROLL (FREE)"
		else
			rollBtn.Text = "ROLL (" .. Components.formatNumber(state.rollCostYen) .. " Y)"
		end

		autoBtn.Text = if state.autoEnabled then "AUTO: ON" else "AUTO: OFF"
		fastBtn.Text = if state.fastEnabled then "FAST: ON" else "FAST: OFF"

		applyTheme()
	end

	rollBtn.MouseButton1Click:Connect(callbacks.onRoll)
	autoBtn.MouseButton1Click:Connect(callbacks.onToggleAuto)
	fastBtn.MouseButton1Click:Connect(callbacks.onToggleFast)
	deckBtn.MouseButton1Click:Connect(callbacks.onOpenDeck)
	battleBtn.MouseButton1Click:Connect(callbacks.onOpenBattle)
	settingsBtn.MouseButton1Click:Connect(callbacks.onOpenSettings)

	local unbindTheme = Theme.onChanged(function()
		applyTheme()
	end)

	renderState(lastState)

	return {
		update = function(state: HudState)
			renderState(state)
		end,
		setVisible = function(visible: boolean)
			root.Visible = visible
		end,
		destroy = function()
			unbindTheme()
			root:Destroy()
		end,
	}
end

return Hud
