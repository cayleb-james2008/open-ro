--!strict

local Workspace = game:GetService("Workspace")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

local Hud = {}

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
	selectedWorld: number?,
	bannerName: string?,
	weatherText: string?,
}

export type HudCallbacks = {
	onRoll: () -> (),
	onToggleAuto: () -> (),
	onToggleFast: () -> (),
	onOpenDeck: () -> (),
	onOpenBattle: () -> (),
	onOpenSettings: () -> (),
	onOpenShop: (() -> ())?,
	onCycleWorld: (() -> ())?,
	onOpenTower: (() -> ())?,
	onOpenRaid: (() -> ())?,
	onOpenWorldBoss: (() -> ())?,
	onOpenRanked: (() -> ())?,
	onOpenRebirth: (() -> ())?,
	onOpenCharms: (() -> ())?,
	onOpenQuests: (() -> ())?,
	onOpenIndex: (() -> ())?,
	onOpenLeaderboards: (() -> ())?,
}

export type HudController = {
	update: (state: HudState) -> (),
	setVisible: (visible: boolean) -> (),
	destroy: () -> (),
}

function Hud.create(parent: Instance, callbacks: HudCallbacks): HudController
	local root = Instance.new("Frame")
	root.Name = "HudRoot"
	root.Size = UDim2.fromScale(1, 1)
	root.BackgroundTransparency = 1
	root.ZIndex = 5
	root.Parent = parent

	local uiScale = Instance.new("UIScale")
	uiScale.Scale = 1
	uiScale.Parent = root

	local layoutUpdater: ((viewportX: number) -> ())?

	local function applyViewportScale()
		local cam = Workspace.CurrentCamera
		if not cam then
			return
		end
		local vx = cam.ViewportSize.X
		local vy = cam.ViewportSize.Y
		if vx < 420 then
			uiScale.Scale = 1
		elseif vx < 700 or vy < 540 then
			uiScale.Scale = 0.9
		else
			uiScale.Scale = 1
		end
		if layoutUpdater then
			layoutUpdater(vx)
		end
	end
	local cam = Workspace.CurrentCamera
	if cam then
		cam:GetPropertyChangedSignal("ViewportSize"):Connect(applyViewportScale)
	end

	-- Top-left resource stack
	local topLeft = Instance.new("Frame")
	topLeft.Name = "TopLeft"
	topLeft.Size = UDim2.fromOffset(340, 174)
	topLeft.Position = UDim2.fromOffset(12, 10)
	topLeft.BackgroundTransparency = 1
	topLeft.ZIndex = 6
	topLeft.Parent = root

	local topLeftLayout = Instance.new("UIListLayout")
	topLeftLayout.FillDirection = Enum.FillDirection.Vertical
	topLeftLayout.Padding = UDim.new(0, 6)
	topLeftLayout.SortOrder = Enum.SortOrder.LayoutOrder
	topLeftLayout.Parent = topLeft

	local currencyRow = Instance.new("Frame")
	currencyRow.Name = "CurrencyRow"
	currencyRow.Size = UDim2.new(1, 0, 0, 36)
	currencyRow.BackgroundTransparency = 1
	currencyRow.LayoutOrder = 1
	currencyRow.ZIndex = 6
	currencyRow.Parent = topLeft

	local currencyLayout = Instance.new("UIListLayout")
	currencyLayout.FillDirection = Enum.FillDirection.Horizontal
	currencyLayout.Padding = UDim.new(0, 6)
	currencyLayout.SortOrder = Enum.SortOrder.LayoutOrder
	currencyLayout.Parent = currencyRow

	local themedPills: { Frame } = {}
	local themedTexts: { TextLabel } = {}

	local function makeCurrencyPill(name: string, order: number, width: number, textColor: Color3): (Frame, TextLabel)
		local pill = Components.makePanel(currencyRow, name, UDim2.fromOffset(width, 36), UDim2.fromOffset(0, 0), 10)
		pill.LayoutOrder = order
		pill.ZIndex = 7
		local lbl = Components.makeLabel(
			pill,
			"Value",
			"0",
			UDim2.new(1, -12, 1, 0),
			UDim2.fromOffset(6, 0),
			Theme.FONT_NUM,
			12,
			textColor,
			Enum.TextXAlignment.Center
		)
		lbl.ZIndex = 8
		table.insert(themedPills, pill)
		return pill, lbl
	end

	local yenPill, yenLabel = makeCurrencyPill("YenPill", 1, 114, Theme.getPalette().accentGold)
	local gemsPill, gemsLabel = makeCurrencyPill("GemsPill", 2, 102, Theme.getPalette().accentBlue)
	local ticketsPill, ticketsLabel = makeCurrencyPill("TicketsPill", 3, 94, Theme.getPalette().accentPurple)

	-- Row 2: Luck + Weather chips
	local chipsRow = Instance.new("Frame")
	chipsRow.Name = "ChipsRow"
	chipsRow.Size = UDim2.new(1, 0, 0, 30)
	chipsRow.BackgroundTransparency = 1
	chipsRow.LayoutOrder = 2
	chipsRow.ZIndex = 6
	chipsRow.Parent = topLeft

	local chipsLayout = Instance.new("UIListLayout")
	chipsLayout.FillDirection = Enum.FillDirection.Horizontal
	chipsLayout.Padding = UDim.new(0, 6)
	chipsLayout.SortOrder = Enum.SortOrder.LayoutOrder
	chipsLayout.Parent = chipsRow

	local luckChip = Components.makePanel(chipsRow, "LuckChip", UDim2.new(0.56, -3, 1, 0), UDim2.fromOffset(0, 0), 10)
	luckChip.LayoutOrder = 1
	luckChip.ZIndex = 7
	local luckStroke = luckChip:FindFirstChildOfClass("UIStroke")
	if luckStroke then
		luckStroke.Color = Theme.getPalette().accentGreen
	end
	local luckLabel = Components.makeLabel(
		luckChip,
		"LuckLabel",
		"Luck 1.0x",
		UDim2.new(1, -12, 1, 0),
		UDim2.fromOffset(6, 0),
		Theme.FONT_BODY,
		11,
		Theme.getPalette().accentGreen,
		Enum.TextXAlignment.Left
	)
	luckLabel.ZIndex = 8
	table.insert(themedPills, luckChip)

	local weatherChip =
		Components.makePanel(chipsRow, "WeatherChip", UDim2.new(0.44, -3, 1, 0), UDim2.fromOffset(0, 0), 10)
	weatherChip.LayoutOrder = 2
	weatherChip.ZIndex = 7
	local weatherLabel = Components.makeLabel(
		weatherChip,
		"WeatherLabel",
		"Clear Skies",
		UDim2.new(1, -12, 1, 0),
		UDim2.fromOffset(6, 0),
		Theme.FONT_BODY,
		11,
		Theme.getPalette().accentBlue,
		Enum.TextXAlignment.Center
	)
	weatherLabel.ZIndex = 8
	table.insert(themedPills, weatherChip)

	-- Row 3: Next Goal
	local goalChip = Components.makePanel(topLeft, "GoalChip", UDim2.new(1, 0, 0, 30), UDim2.fromOffset(0, 0), 10)
	goalChip.LayoutOrder = 3
	goalChip.ZIndex = 7
	local goalLabel = Components.makeLabel(
		goalChip,
		"GoalLabel",
		"Next: Summon your first card!",
		UDim2.new(1, -14, 1, 0),
		UDim2.fromOffset(7, 0),
		Theme.FONT_BODY,
		11,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	goalLabel.ZIndex = 8
	table.insert(themedPills, goalChip)
	table.insert(themedTexts, goalLabel)

	-- Row 4: World / Banner selector (>=44px interactive button)
	local worldBannerBtn = Components.makeButton(
		topLeft,
		"WorldBannerButton",
		"World 1 • Hidden Leaf Banner (Tap to switch)",
		UDim2.new(1, 0, 0, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgPanel,
		Theme.getPalette().textPrimary
	)
	worldBannerBtn.LayoutOrder = 4
	worldBannerBtn.TextSize = 12
	worldBannerBtn.ZIndex = 7

	-- Top-right navigation buttons (>=44px height)
	local topRight = Instance.new("Frame")
	topRight.Name = "TopRight"
	topRight.Size = UDim2.fromOffset(540, 52)
	topRight.Position = UDim2.new(1, -12, 0, 10)
	topRight.AnchorPoint = Vector2.new(1, 0)
	topRight.BackgroundTransparency = 1
	topRight.ZIndex = 6
	topRight.Parent = root

	local navGrid = Instance.new("UIGridLayout")
	navGrid.CellSize = UDim2.fromOffset(102, 46)
	navGrid.CellPadding = UDim2.fromOffset(6, 6)
	navGrid.HorizontalAlignment = Enum.HorizontalAlignment.Right
	navGrid.SortOrder = Enum.SortOrder.LayoutOrder
	navGrid.Parent = topRight

	local deckBtn = Components.makeButton(
		topRight,
		"DeckButton",
		"Deck (0/5)",
		UDim2.fromOffset(102, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgPanel,
		Theme.getPalette().textPrimary
	)
	deckBtn.LayoutOrder = 1
	deckBtn.TextSize = 12
	deckBtn.ZIndex = 7

	local battleBtn = Components.makeButton(
		topRight,
		"BattleButton",
		"Battle",
		UDim2.fromOffset(102, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	battleBtn.LayoutOrder = 2
	battleBtn.TextSize = 13
	battleBtn.ZIndex = 7

	local modesBtn = Components.makeButton(
		topRight,
		"ModesButton",
		"Modes",
		UDim2.fromOffset(102, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentPurple,
		Color3.fromRGB(255, 255, 255)
	)
	modesBtn.LayoutOrder = 3
	modesBtn.TextSize = 13
	modesBtn.ZIndex = 7

	local shopBtn = Components.makeButton(
		topRight,
		"ShopButton",
		"Shop",
		UDim2.fromOffset(102, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentGold,
		Theme.getPalette().textOnAccent
	)
	shopBtn.LayoutOrder = 4
	shopBtn.TextSize = 13
	shopBtn.ZIndex = 7

	local settingsBtn = Components.makeButton(
		topRight,
		"SettingsButton",
		"Settings",
		UDim2.fromOffset(102, 46),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgPanel,
		Theme.getPalette().textPrimary
	)
	settingsBtn.LayoutOrder = 5
	settingsBtn.TextSize = 12
	settingsBtn.ZIndex = 7

	-- Modes Drawer popup
	local modesDrawer =
		Components.makePanel(root, "ModesDrawer", UDim2.fromOffset(340, 326), UDim2.fromScale(0.5, 0.5), 14)
	modesDrawer.AnchorPoint = Vector2.new(0.5, 0.5)
	modesDrawer.Visible = false
	modesDrawer.ZIndex = 24

	local drawerTitle = Components.makeLabel(
		modesDrawer,
		"Title",
		"All Game Modes & Features",
		UDim2.new(1, -60, 0, 28),
		UDim2.fromOffset(14, 10),
		Theme.FONT_HEADING,
		15,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	drawerTitle.ZIndex = 25

	local closeModesBtn = Components.makeButton(
		modesDrawer,
		"CloseModesBtn",
		"X",
		UDim2.fromOffset(44, 44),
		UDim2.new(1, -50, 0, 4),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	closeModesBtn.ZIndex = 25

	local modesGridHost = Instance.new("Frame")
	modesGridHost.Name = "ModesGridHost"
	modesGridHost.Size = UDim2.new(1, -24, 1, -60)
	modesGridHost.Position = UDim2.fromOffset(12, 52)
	modesGridHost.BackgroundTransparency = 1
	modesGridHost.ZIndex = 25
	modesGridHost.Parent = modesDrawer

	local modesGrid = Instance.new("UIGridLayout")
	modesGrid.CellSize = UDim2.fromOffset(100, 48)
	modesGrid.CellPadding = UDim2.fromOffset(8, 8)
	modesGrid.SortOrder = Enum.SortOrder.LayoutOrder
	modesGrid.Parent = modesGridHost

	local modeButtons: { TextButton } = {}
	local function addModeButton(name: string, label: string, order: number, cb: (() -> ())?)
		local btn = Components.makeButton(
			modesGridHost,
			name,
			label,
			UDim2.fromOffset(100, 48),
			UDim2.fromOffset(0, 0),
			Theme.getPalette().bgMuted,
			Theme.getPalette().textPrimary
		)
		btn.LayoutOrder = order
		btn.TextSize = 12
		btn.ZIndex = 26
		btn.MouseButton1Click:Connect(function()
			modesDrawer.Visible = false
			if cb then
				cb()
			end
		end)
		table.insert(modeButtons, btn)
	end

	addModeButton("TowerModeBtn", "Tower", 1, callbacks.onOpenTower)
	addModeButton("RaidModeBtn", "Raids", 2, callbacks.onOpenRaid)
	addModeButton("WorldBossModeBtn", "World Boss", 3, callbacks.onOpenWorldBoss)
	addModeButton("RankedModeBtn", "Ranked PvP", 4, callbacks.onOpenRanked)
	addModeButton("RebirthModeBtn", "Rebirth", 5, callbacks.onOpenRebirth)
	addModeButton("CharmsModeBtn", "Charms", 6, callbacks.onOpenCharms)
	addModeButton("QuestsModeBtn", "Quests & Codes", 7, callbacks.onOpenQuests)
	addModeButton("IndexModeBtn", "54-Card Index", 8, callbacks.onOpenIndex)
	addModeButton("LeadersModeBtn", "Leaderboards", 9, callbacks.onOpenLeaderboards)

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
			topLeft.Size = UDim2.fromOffset(usableW, 152)
			topLeftLayout.Padding = UDim.new(0, 4)

			currencyRow.Size = UDim2.new(1, 0, 0, 34)
			local pillGap = 6
			local pillW = math.floor((usableW - pillGap * 2) / 3)
			yenPill.Size = UDim2.fromOffset(pillW, 34)
			gemsPill.Size = UDim2.fromOffset(pillW, 34)
			ticketsPill.Size = UDim2.fromOffset(usableW - pillW * 2 - pillGap * 2, 34)

			chipsRow.Size = UDim2.new(1, 0, 0, 28)
			goalChip.Size = UDim2.new(1, 0, 0, 28)
			worldBannerBtn.Size = UDim2.new(1, 0, 0, 44)

			topRight.AnchorPoint = Vector2.new(0, 0)
			topRight.Position = UDim2.fromOffset(8, 166)
			topRight.Size = UDim2.fromOffset(usableW, 96)
			navGrid.HorizontalAlignment = Enum.HorizontalAlignment.Left
			local btnW = math.floor((usableW - 12) / 3)
			navGrid.CellSize = UDim2.fromOffset(btnW, 44)
			navGrid.CellPadding = UDim2.fromOffset(6, 6)

			modesDrawer.Size = UDim2.fromOffset(math.min(usableW, 350), 310)
			local modeCellW = math.floor((math.min(usableW, 350) - 40) / 3)
			modesGrid.CellSize = UDim2.fromOffset(modeCellW, 46)

			rollDock.Size = UDim2.fromOffset(math.min(usableW, 400), 104)
			rollDock.Position = UDim2.new(0.5, 0, 1, -10)
			dockConstraint.MinSize = Vector2.new(280, 100)
		else
			topLeft.Position = UDim2.fromOffset(12, 10)
			topLeft.Size = UDim2.fromOffset(340, 174)
			topLeftLayout.Padding = UDim.new(0, 6)

			currencyRow.Size = UDim2.new(1, 0, 0, 36)
			yenPill.Size = UDim2.fromOffset(114, 36)
			gemsPill.Size = UDim2.fromOffset(102, 36)
			ticketsPill.Size = UDim2.fromOffset(94, 36)

			chipsRow.Size = UDim2.new(1, 0, 0, 30)
			goalChip.Size = UDim2.new(1, 0, 0, 30)
			worldBannerBtn.Size = UDim2.new(1, 0, 0, 44)

			topRight.AnchorPoint = Vector2.new(1, 0)
			topRight.Position = UDim2.new(1, -12, 0, 10)
			topRight.Size = UDim2.fromOffset(550, 52)
			navGrid.HorizontalAlignment = Enum.HorizontalAlignment.Right
			navGrid.CellSize = UDim2.fromOffset(102, 46)
			navGrid.CellPadding = UDim2.fromOffset(6, 6)

			modesDrawer.Size = UDim2.fromOffset(348, 320)
			modesGrid.CellSize = UDim2.fromOffset(102, 48)

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
		selectedWorld = 1,
		bannerName = "Hidden Leaf",
		weatherText = "Clear Skies",
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
		weatherLabel.TextColor3 = palette.accentBlue

		deckBtn.BackgroundColor3 = palette.bgPanel
		deckBtn.TextColor3 = palette.textPrimary
		settingsBtn.BackgroundColor3 = palette.bgPanel
		settingsBtn.TextColor3 = palette.textPrimary
		worldBannerBtn.BackgroundColor3 = palette.bgPanel
		worldBannerBtn.TextColor3 = palette.textPrimary

		modesDrawer.BackgroundColor3 = palette.bgPanel
		drawerTitle.TextColor3 = palette.textPrimary
		closeModesBtn.BackgroundColor3 = palette.bgMuted
		closeModesBtn.TextColor3 = palette.textPrimary
		for _, mb in modeButtons do
			mb.BackgroundColor3 = palette.bgMuted
			mb.TextColor3 = palette.textPrimary
		end

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
		weatherLabel.Text = state.weatherText or "Clear Skies"
		goalLabel.Text = "Next: " .. state.nextGoal
		local wNum = state.selectedWorld or 1
		local bName = state.bannerName or ("World " .. tostring(wNum))
		worldBannerBtn.Text = string.format("W%d • %s Banner (Switch)", wNum, bName)
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
	shopBtn.MouseButton1Click:Connect(function()
		if callbacks.onOpenShop then
			callbacks.onOpenShop()
		end
	end)
	worldBannerBtn.MouseButton1Click:Connect(function()
		if callbacks.onCycleWorld then
			callbacks.onCycleWorld()
		end
	end)
	modesBtn.MouseButton1Click:Connect(function()
		modesDrawer.Visible = not modesDrawer.Visible
	end)
	closeModesBtn.MouseButton1Click:Connect(function()
		modesDrawer.Visible = false
	end)

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
