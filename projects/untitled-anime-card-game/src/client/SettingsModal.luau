--!strict

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type SettingsState = {
	theme: Theme.ThemeMode,
	reducedMotion: boolean,
	reducedFlash: boolean,
	skipBelowRarity: string,
}

export type SettingsModalController = {
	open: () -> (),
	close: () -> (),
	isOpen: () -> boolean,
	getState: () -> SettingsState,
	setState: (nextState: SettingsState) -> (),
	destroy: () -> (),
}

local SKIP_OPTIONS = { "None", "Common", "Uncommon", "Rare", "Epic" }

local SettingsModal = {}

function SettingsModal.create(
	parent: Instance,
	initialState: SettingsState?,
	onChange: ((SettingsState) -> ())?
): SettingsModalController
	local state: SettingsState = {
		theme = if initialState and initialState.theme == "Light" then "Light" else "Dark",
		reducedMotion = if initialState then initialState.reducedMotion == true else false,
		reducedFlash = if initialState then initialState.reducedFlash == true else false,
		skipBelowRarity = if initialState and initialState.skipBelowRarity ~= ""
			then initialState.skipBelowRarity
			else "None",
	}

	Theme.setMode(state.theme)

	local overlay = Instance.new("TextButton")
	overlay.Name = "SettingsOverlay"
	overlay.Size = UDim2.fromScale(1, 1)
	overlay.BackgroundColor3 = Color3.fromRGB(4, 8, 18)
	overlay.BackgroundTransparency = 0.42
	overlay.BorderSizePixel = 0
	overlay.Text = ""
	overlay.AutoButtonColor = false
	overlay.Visible = false
	overlay.ZIndex = 40
	overlay.Parent = parent

	local panel =
		Components.makePanel(overlay, "SettingsPanel", UDim2.fromScale(0.92, 0.82), UDim2.fromScale(0.5, 0.5), 14)
	panel.AnchorPoint = Vector2.new(0.5, 0.5)
	panel.ZIndex = 41

	local panelMax = Instance.new("UISizeConstraint")
	panelMax.MinSize = Vector2.new(300, 340)
	panelMax.MaxSize = Vector2.new(520, 480)
	panelMax.Parent = panel

	-- Intercept taps inside the panel so they do not close the backdrop
	local panelSink = Instance.new("TextButton")
	panelSink.Name = "InputSink"
	panelSink.Size = UDim2.fromScale(1, 1)
	panelSink.BackgroundTransparency = 1
	panelSink.Text = ""
	panelSink.AutoButtonColor = false
	panelSink.ZIndex = 41
	panelSink.Parent = panel

	local header = Instance.new("Frame")
	header.Name = "Header"
	header.Size = UDim2.new(1, 0, 0, 56)
	header.BackgroundTransparency = 1
	header.ZIndex = 42
	header.Parent = panel

	local titleLbl = Components.makeLabel(
		header,
		"Title",
		"Settings",
		UDim2.new(1, -68, 1, 0),
		UDim2.fromOffset(18, 0),
		Theme.FONT_DISPLAY,
		20,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	titleLbl.ZIndex = 42

	local closeBtn = Components.makeButton(
		header,
		"CloseButton",
		"X",
		UDim2.fromOffset(44, 44),
		UDim2.new(1, -54, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	closeBtn.ZIndex = 43

	local divider = Instance.new("Frame")
	divider.Name = "Divider"
	divider.Size = UDim2.new(1, -28, 0, 1)
	divider.Position = UDim2.fromOffset(14, 56)
	divider.BackgroundColor3 = Theme.getPalette().border
	divider.BorderSizePixel = 0
	divider.ZIndex = 42
	divider.Parent = panel

	local scroll = Instance.new("ScrollingFrame")
	scroll.Name = "Body"
	scroll.Size = UDim2.new(1, -20, 1, -72)
	scroll.Position = UDim2.fromOffset(10, 64)
	scroll.BackgroundTransparency = 1
	scroll.BorderSizePixel = 0
	scroll.ScrollBarThickness = 4
	scroll.CanvasSize = UDim2.fromOffset(0, 0)
	scroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
	scroll.ZIndex = 42
	scroll.Parent = panel
	Components.addPadding(scroll, 6, 8, 14, 8)

	local listLayout = Instance.new("UIListLayout")
	listLayout.SortOrder = Enum.SortOrder.LayoutOrder
	listLayout.Padding = UDim.new(0, 12)
	listLayout.Parent = scroll

	local themedCards: { Frame } = {}
	local themedHeaders: { TextLabel } = {}
	local themedDescs: { TextLabel } = {}

	local function makeSection(order: number, title: string, desc: string, heightPx: number): Frame
		local palette = Theme.getPalette()
		local card = Instance.new("Frame")
		card.Name = "Section_" .. tostring(order)
		card.LayoutOrder = order
		card.Size = UDim2.new(1, 0, 0, heightPx)
		card.BackgroundColor3 = palette.bgCard
		card.BorderSizePixel = 0
		card.ZIndex = 42
		card.Parent = scroll
		Components.addCorner(card, 10)
		Components.addStroke(card, palette.border, 1, 0.3)

		local tLbl = Components.makeLabel(
			card,
			"SectionTitle",
			title,
			UDim2.new(1, -24, 0, 22),
			UDim2.fromOffset(12, 10),
			Theme.FONT_DISPLAY,
			15,
			palette.textPrimary,
			Enum.TextXAlignment.Left
		)
		tLbl.ZIndex = 43

		local dLbl = Components.makeLabel(
			card,
			"SectionDesc",
			desc,
			UDim2.new(1, -24, 0, 18),
			UDim2.fromOffset(12, 32),
			Theme.FONT_BODY,
			12,
			palette.textSecondary,
			Enum.TextXAlignment.Left
		)
		dLbl.ZIndex = 43

		table.insert(themedCards, card)
		table.insert(themedHeaders, tLbl)
		table.insert(themedDescs, dLbl)
		return card
	end

	-- 1. Light / Dark Theme Switch (Settings ONLY)
	local themeSection = makeSection(1, "Appearance", "Choose Dark or Light mode across all menus.", 108)
	local darkBtn = Components.makeButton(
		themeSection,
		"ThemeDarkButton",
		"Dark",
		UDim2.new(0.46, 0, 0, 44),
		UDim2.fromOffset(12, 54),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	darkBtn.ZIndex = 43

	local lightBtn = Components.makeButton(
		themeSection,
		"ThemeLightButton",
		"Light",
		UDim2.new(0.46, 0, 0, 44),
		UDim2.new(0.54, -12, 0, 54),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	lightBtn.ZIndex = 43

	-- 2. Visual Comfort Toggles
	local comfortSection =
		makeSection(2, "Visual Comfort", "Tone down camera shake and bright flashes during rare pulls.", 118)
	local motionBtn = Components.makeButton(
		comfortSection,
		"ReducedMotionButton",
		"Reduced Motion: Off",
		UDim2.new(0.46, 0, 0, 44),
		UDim2.fromOffset(12, 60),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	motionBtn.ZIndex = 43
	motionBtn.TextSize = 13

	local flashBtn = Components.makeButton(
		comfortSection,
		"ReducedFlashButton",
		"Reduced Flash: Off",
		UDim2.new(0.46, 0, 0, 44),
		UDim2.new(0.54, -12, 0, 60),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	flashBtn.ZIndex = 43
	flashBtn.TextSize = 13

	-- 3. Auto-Skip Reveal Below Rarity
	local skipSection =
		makeSection(3, "Skip Reveal Animation", "Skip buildup animations for cards below your chosen rarity.", 120)
	local skipRow = Instance.new("Frame")
	skipRow.Name = "SkipRow"
	skipRow.Size = UDim2.new(1, -24, 0, 48)
	skipRow.Position = UDim2.fromOffset(12, 58)
	skipRow.BackgroundTransparency = 1
	skipRow.ZIndex = 43
	skipRow.Parent = skipSection

	local skipGrid = Instance.new("UIGridLayout")
	skipGrid.CellSize = UDim2.new(0.184, 0, 0, 44)
	skipGrid.CellPadding = UDim2.fromOffset(6, 6)
	skipGrid.SortOrder = Enum.SortOrder.LayoutOrder
	skipGrid.Parent = skipRow

	local skipButtons: { [string]: TextButton } = {}
	for idx, rarityOpt in SKIP_OPTIONS do
		local btn = Components.makeButton(
			skipRow,
			"Skip_" .. rarityOpt,
			rarityOpt,
			UDim2.fromOffset(64, 44),
			UDim2.fromOffset(0, 0),
			Theme.getPalette().bgMuted,
			Theme.getPalette().textPrimary
		)
		btn.LayoutOrder = idx
		btn.TextSize = 11
		btn.ZIndex = 43
		skipButtons[rarityOpt] = btn
	end

	local function refreshVisuals()
		local palette = Theme.getPalette()
		panel.BackgroundColor3 = palette.bgPanel
		local pStroke = panel:FindFirstChildOfClass("UIStroke")
		if pStroke then
			pStroke.Color = palette.border
		end
		titleLbl.TextColor3 = palette.textPrimary
		closeBtn.BackgroundColor3 = palette.bgMuted
		closeBtn.TextColor3 = palette.textPrimary
		divider.BackgroundColor3 = palette.border

		for _, card in themedCards do
			card.BackgroundColor3 = palette.bgCard
			local cStroke = card:FindFirstChildOfClass("UIStroke")
			if cStroke then
				cStroke.Color = palette.border
			end
		end
		for _, h in themedHeaders do
			h.TextColor3 = palette.textPrimary
		end
		for _, d in themedDescs do
			d.TextColor3 = palette.textSecondary
		end

		local isDark = state.theme == "Dark"
		darkBtn.BackgroundColor3 = if isDark then palette.accentGold else palette.bgMuted
		darkBtn.TextColor3 = if isDark then palette.textOnAccent else palette.textPrimary
		lightBtn.BackgroundColor3 = if not isDark then palette.accentGold else palette.bgMuted
		lightBtn.TextColor3 = if not isDark then palette.textOnAccent else palette.textPrimary

		motionBtn.Text = "Reduced Motion: " .. if state.reducedMotion then "On" else "Off"
		motionBtn.BackgroundColor3 = if state.reducedMotion then palette.accentGreen else palette.bgMuted
		motionBtn.TextColor3 = if state.reducedMotion then palette.textOnAccent else palette.textPrimary

		flashBtn.Text = "Reduced Flash: " .. if state.reducedFlash then "On" else "Off"
		flashBtn.BackgroundColor3 = if state.reducedFlash then palette.accentGreen else palette.bgMuted
		flashBtn.TextColor3 = if state.reducedFlash then palette.textOnAccent else palette.textPrimary

		for optName, btn in skipButtons do
			local selected = state.skipBelowRarity == optName
			btn.BackgroundColor3 = if selected then palette.accentBlue else palette.bgMuted
			btn.TextColor3 = if selected then palette.textOnAccent else palette.textPrimary
		end
	end

	local function emitChange()
		refreshVisuals()
		if onChange then
			onChange(table.clone(state))
		end
	end

	darkBtn.MouseButton1Click:Connect(function()
		state.theme = "Dark"
		Theme.setMode("Dark")
		emitChange()
	end)

	lightBtn.MouseButton1Click:Connect(function()
		state.theme = "Light"
		Theme.setMode("Light")
		emitChange()
	end)

	motionBtn.MouseButton1Click:Connect(function()
		state.reducedMotion = not state.reducedMotion
		emitChange()
	end)

	flashBtn.MouseButton1Click:Connect(function()
		state.reducedFlash = not state.reducedFlash
		emitChange()
	end)

	for optName, btn in skipButtons do
		btn.MouseButton1Click:Connect(function()
			state.skipBelowRarity = optName
			emitChange()
		end)
	end

	closeBtn.MouseButton1Click:Connect(function()
		overlay.Visible = false
	end)

	overlay.MouseButton1Click:Connect(function()
		overlay.Visible = false
	end)

	local unbindTheme = Theme.onChanged(function()
		refreshVisuals()
	end)

	refreshVisuals()

	return {
		open = function()
			refreshVisuals()
			overlay.Visible = true
		end,
		close = function()
			overlay.Visible = false
		end,
		isOpen = function(): boolean
			return overlay.Visible
		end,
		getState = function(): SettingsState
			return table.clone(state)
		end,
		setState = function(nextState: SettingsState)
			state.theme = if nextState.theme == "Light" then "Light" else "Dark"
			state.reducedMotion = nextState.reducedMotion == true
			state.reducedFlash = nextState.reducedFlash == true
			state.skipBelowRarity = nextState.skipBelowRarity or "None"
			Theme.setMode(state.theme)
			refreshVisuals()
		end,
		destroy = function()
			unbindTheme()
			overlay:Destroy()
		end,
	}
end

return SettingsModal
