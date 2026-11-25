--!strict

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type OwnedCard = {
	uid: string,
	id: string,
	cardId: string?,
	name: string,
	displayName: string?,
	rarity: string,
	mutation: string?,
	role: string?,
	faction: string?,
	showName: string?,
	artKey: string?,
	level: number,
	stars: number,
	hp: number,
	atk: number,
	spd: number,
	power: number,
	locked: boolean?,
	dupesAvailable: number?,
	dupeCount: number?,
	levelUpCostYen: number?,
	sellValueYen: number?,
}

export type DeckSnapshot = {
	cards: { OwnedCard },
	activeDeckUids: { string },
	defenseDeckUids: { string }?,
	maxDeckSize: number,
	synergySummary: string,
	bondsSummary: string?,
	totalPower: number,
	yen: number,
}

export type DeckCallbacks = {
	onEquipToggle: (uid: string, shouldEquip: boolean) -> (),
	onDefenseEquipToggle: ((uid: string, shouldEquip: boolean) -> ())?,
	onLevelUp: (uid: string) -> (),
	onAscend: (uid: string) -> (),
	onToggleLock: (uid: string) -> (),
	onSell: (uid: string) -> (),
}

export type DeckModalController = {
	open: (snapshot: DeckSnapshot) -> (),
	close: () -> (),
	isOpen: () -> boolean,
	refresh: (snapshot: DeckSnapshot) -> (),
	destroy: () -> (),
}

local DeckModal = {}

function DeckModal.create(parent: Instance, callbacks: DeckCallbacks): DeckModalController
	local selectedUid: string? = nil
	local deckTab = "Active"
	local currentSnapshot: DeckSnapshot = {
		cards = {},
		activeDeckUids = {},
		defenseDeckUids = {},
		maxDeckSize = 5,
		synergySummary = "No synergy active",
		bondsSummary = "",
		totalPower = 0,
		yen = 0,
	}

	local overlay = Instance.new("TextButton")
	overlay.Name = "DeckOverlay"
	overlay.Size = UDim2.fromScale(1, 1)
	overlay.BackgroundColor3 = Color3.fromRGB(4, 8, 18)
	overlay.BackgroundTransparency = 0.4
	overlay.BorderSizePixel = 0
	overlay.Text = ""
	overlay.AutoButtonColor = false
	overlay.Visible = false
	overlay.ZIndex = 30
	overlay.Parent = parent

	local panel = Components.makePanel(overlay, "DeckPanel", UDim2.fromScale(0.94, 0.88), UDim2.fromScale(0.5, 0.5), 14)
	panel.AnchorPoint = Vector2.new(0.5, 0.5)
	panel.ZIndex = 31

	local panelBounds = Instance.new("UISizeConstraint")
	panelBounds.MinSize = Vector2.new(320, 380)
	panelBounds.MaxSize = Vector2.new(860, 620)
	panelBounds.Parent = panel

	local panelSink = Instance.new("TextButton")
	panelSink.Name = "InputSink"
	panelSink.Size = UDim2.fromScale(1, 1)
	panelSink.BackgroundTransparency = 1
	panelSink.Text = ""
	panelSink.AutoButtonColor = false
	panelSink.ZIndex = 31
	panelSink.Parent = panel

	local header = Instance.new("Frame")
	header.Name = "Header"
	header.Size = UDim2.new(1, 0, 0, 54)
	header.BackgroundTransparency = 1
	header.ZIndex = 32
	header.Parent = panel

	local titleLbl = Components.makeLabel(
		header,
		"Title",
		"Collection & Deck (0/5)",
		UDim2.new(1, -260, 1, 0),
		UDim2.fromOffset(16, 0),
		Theme.FONT_DISPLAY,
		17,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	titleLbl.ZIndex = 32

	local activeTabBtn = Components.makeButton(
		header,
		"ActiveTabBtn",
		"Active",
		UDim2.fromOffset(82, 44),
		UDim2.new(1, -234, 0.5, -22),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	activeTabBtn.TextSize = 12
	activeTabBtn.ZIndex = 33

	local defenseTabBtn = Components.makeButton(
		header,
		"DefenseTabBtn",
		"Defense",
		UDim2.fromOffset(82, 44),
		UDim2.new(1, -146, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	defenseTabBtn.TextSize = 12
	defenseTabBtn.ZIndex = 33

	local closeBtn = Components.makeButton(
		header,
		"CloseButton",
		"X",
		UDim2.fromOffset(44, 44),
		UDim2.new(1, -54, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	closeBtn.ZIndex = 33

	-- Active Deck strip + Synergy line
	local deckStrip = Instance.new("Frame")
	deckStrip.Name = "ActiveDeckStrip"
	deckStrip.Size = UDim2.new(1, -28, 0, 88)
	deckStrip.Position = UDim2.fromOffset(14, 56)
	deckStrip.BackgroundColor3 = Theme.getPalette().bgCard
	deckStrip.BorderSizePixel = 0
	deckStrip.ZIndex = 32
	deckStrip.Parent = panel
	Components.addCorner(deckStrip, 10)
	Components.addStroke(deckStrip, Theme.getPalette().border, 1, 0.25)

	local synergyLbl = Components.makeLabel(
		deckStrip,
		"SynergyLabel",
		"Power: 0  |  Synergy: None",
		UDim2.new(1, -20, 0, 22),
		UDim2.fromOffset(10, 6),
		Theme.FONT_BODY,
		12,
		Theme.getPalette().accentGold,
		Enum.TextXAlignment.Left
	)
	synergyLbl.ZIndex = 33

	local slotsRow = Instance.new("Frame")
	slotsRow.Name = "SlotsRow"
	slotsRow.Size = UDim2.new(1, -20, 0, 50)
	slotsRow.Position = UDim2.fromOffset(10, 32)
	slotsRow.BackgroundTransparency = 1
	slotsRow.ZIndex = 33
	slotsRow.Parent = deckStrip

	local slotsLayout = Instance.new("UIGridLayout")
	slotsLayout.CellSize = UDim2.new(0.188, 0, 0, 46)
	slotsLayout.CellPadding = UDim2.fromOffset(6, 4)
	slotsLayout.SortOrder = Enum.SortOrder.LayoutOrder
	slotsLayout.Parent = slotsRow

	-- Main split: scrollable collection + bottom/side inspector
	local collectionScroll = Instance.new("ScrollingFrame")
	collectionScroll.Name = "CollectionScroll"
	collectionScroll.Size = UDim2.new(1, -28, 1, -300)
	collectionScroll.Position = UDim2.fromOffset(14, 154)
	collectionScroll.BackgroundColor3 = Theme.getPalette().bgDeep
	collectionScroll.BorderSizePixel = 0
	collectionScroll.ScrollBarThickness = 5
	collectionScroll.CanvasSize = UDim2.fromOffset(0, 0)
	collectionScroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
	collectionScroll.ZIndex = 32
	collectionScroll.Parent = panel
	Components.addCorner(collectionScroll, 10)
	Components.addPadding(collectionScroll, 8, 8, 8, 8)

	local gridLayout = Instance.new("UIGridLayout")
	gridLayout.CellSize = UDim2.fromOffset(142, 186)
	gridLayout.CellPadding = UDim2.fromOffset(8, 8)
	gridLayout.SortOrder = Enum.SortOrder.LayoutOrder
	gridLayout.Parent = collectionScroll

	-- Inspector bar at bottom
	local inspector = Instance.new("Frame")
	inspector.Name = "Inspector"
	inspector.Size = UDim2.new(1, -28, 0, 128)
	inspector.Position = UDim2.new(0, 14, 1, -138)
	inspector.BackgroundColor3 = Theme.getPalette().bgCard
	inspector.BorderSizePixel = 0
	inspector.ZIndex = 32
	inspector.Parent = panel
	Components.addCorner(inspector, 10)
	Components.addStroke(inspector, Theme.getPalette().border, 1, 0.25)

	local inspTitle = Components.makeLabel(
		inspector,
		"InspectorTitle",
		"Select a card to equip, level up, or ascend",
		UDim2.new(1, -20, 0, 24),
		UDim2.fromOffset(10, 8),
		Theme.FONT_DISPLAY,
		14,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	inspTitle.ZIndex = 33

	local inspStats = Components.makeLabel(
		inspector,
		"InspectorStats",
		"",
		UDim2.new(1, -20, 0, 20),
		UDim2.fromOffset(10, 32),
		Theme.FONT_BODY,
		12,
		Theme.getPalette().textSecondary,
		Enum.TextXAlignment.Left
	)
	inspStats.ZIndex = 33

	local actionRow = Instance.new("Frame")
	actionRow.Name = "ActionRow"
	actionRow.Size = UDim2.new(1, -20, 0, 50)
	actionRow.Position = UDim2.fromOffset(10, 66)
	actionRow.BackgroundTransparency = 1
	actionRow.ZIndex = 33
	actionRow.Parent = inspector

	local actionLayout = Instance.new("UIGridLayout")
	actionLayout.CellSize = UDim2.new(0.188, 0, 0, 46)
	actionLayout.CellPadding = UDim2.fromOffset(6, 4)
	actionLayout.SortOrder = Enum.SortOrder.LayoutOrder
	actionLayout.Parent = actionRow

	local equipBtn = Components.makeButton(
		actionRow,
		"EquipButton",
		"Equip",
		UDim2.fromOffset(80, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	equipBtn.LayoutOrder = 1
	equipBtn.TextSize = 12
	equipBtn.ZIndex = 34

	local levelBtn = Components.makeButton(
		actionRow,
		"LevelUpButton",
		"Level Up",
		UDim2.fromOffset(80, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentGreen,
		Theme.getPalette().textOnAccent
	)
	levelBtn.LayoutOrder = 2
	levelBtn.TextSize = 11
	levelBtn.ZIndex = 34

	local ascendBtn = Components.makeButton(
		actionRow,
		"AscendButton",
		"Ascend",
		UDim2.fromOffset(80, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentGold,
		Theme.getPalette().textOnAccent
	)
	ascendBtn.LayoutOrder = 3
	ascendBtn.TextSize = 12
	ascendBtn.ZIndex = 34

	local lockBtn = Components.makeButton(
		actionRow,
		"LockButton",
		"Lock",
		UDim2.fromOffset(80, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	lockBtn.LayoutOrder = 4
	lockBtn.TextSize = 12
	lockBtn.ZIndex = 34

	local sellBtn = Components.makeButton(
		actionRow,
		"SellButton",
		"Sell",
		UDim2.fromOffset(80, 44),
		UDim2.fromOffset(0, 0),
		Theme.getPalette().accentRed,
		Color3.fromRGB(255, 255, 255)
	)
	sellBtn.LayoutOrder = 5
	sellBtn.TextSize = 12
	sellBtn.ZIndex = 34

	local function findCard(uid: string?): OwnedCard?
		if not uid then
			return nil
		end
		for _, card in currentSnapshot.cards do
			if card.uid == uid then
				return card
			end
		end
		return nil
	end

	local function isEquipped(uid: string): boolean
		return table.find(currentSnapshot.activeDeckUids, uid) ~= nil
	end

	local function updateInspector()
		local palette = Theme.getPalette()
		local card = findCard(selectedUid)
		if not card then
			inspTitle.Text = "Select a card from your collection"
			inspStats.Text = string.format(
				"Total Owned: %d  |  Yen: %s",
				#currentSnapshot.cards,
				Components.formatNumber(currentSnapshot.yen)
			)
			equipBtn.Active = false
			equipBtn.BackgroundColor3 = palette.bgMuted
			equipBtn.TextColor3 = palette.textMuted
			levelBtn.Active = false
			levelBtn.BackgroundColor3 = palette.bgMuted
			levelBtn.TextColor3 = palette.textMuted
			ascendBtn.Active = false
			ascendBtn.BackgroundColor3 = palette.bgMuted
			ascendBtn.TextColor3 = palette.textMuted
			lockBtn.Active = false
			lockBtn.BackgroundColor3 = palette.bgMuted
			lockBtn.TextColor3 = palette.textMuted
			sellBtn.Active = false
			sellBtn.BackgroundColor3 = palette.bgMuted
			sellBtn.TextColor3 = palette.textMuted
			return
		end

		local equipped = isEquipped(card.uid)
		local displayName = tostring(card.displayName or card.name or card.cardId or card.id or "Card")
		local mutText = if card.mutation and card.mutation ~= "Normal" then " [" .. card.mutation .. "]" else ""
		inspTitle.Text =
			string.format("%s (%s)%s — Lv.%d (%d*)", displayName, card.rarity, mutText, card.level, card.stars)
		inspStats.Text = string.format(
			"HP %s  ATK %s  SPD %s  |  Power %s",
			Components.formatNumber(card.hp),
			Components.formatNumber(card.atk),
			Components.formatNumber(card.spd),
			Components.formatNumber(card.power)
		)

		equipBtn.Active = true
		equipBtn.Text = if equipped then "Unequip" else "Equip"
		equipBtn.BackgroundColor3 = if equipped then palette.bgMuted else palette.accentBlue
		equipBtn.TextColor3 = if equipped then palette.textPrimary else Color3.fromRGB(255, 255, 255)

		local lvCost = card.levelUpCostYen or 100
		levelBtn.Active = true
		levelBtn.Text = "Lv+ (" .. Components.formatNumber(lvCost) .. ")"
		levelBtn.BackgroundColor3 = palette.accentGreen
		levelBtn.TextColor3 = palette.textOnAccent

		local dupes = card.dupesAvailable or card.dupeCount or 0
		ascendBtn.Active = dupes > 0
		ascendBtn.Text = if dupes > 0 then string.format("Ascend (%d)", dupes) else "Ascend (0)"
		ascendBtn.BackgroundColor3 = if dupes > 0 then palette.accentGold else palette.bgMuted
		ascendBtn.TextColor3 = if dupes > 0 then palette.textOnAccent else palette.textMuted

		lockBtn.Active = true
		lockBtn.Text = if card.locked then "Unlock" else "Lock"
		lockBtn.BackgroundColor3 = palette.bgMuted
		lockBtn.TextColor3 = palette.textPrimary

		local sellVal = card.sellValueYen or 25
		local canSell = (not equipped) and not card.locked
		sellBtn.Active = canSell
		sellBtn.Text = if equipped
			then "Equipped"
			elseif card.locked then "Locked"
			else "Sell +" .. Components.formatNumber(sellVal)
		sellBtn.BackgroundColor3 = if canSell then palette.accentRed else palette.bgMuted
		sellBtn.TextColor3 = if canSell then Color3.fromRGB(255, 255, 255) else palette.textMuted
	end

	local function renderAll()
		local palette = Theme.getPalette()
		panel.BackgroundColor3 = palette.bgPanel
		local pStroke = panel:FindFirstChildOfClass("UIStroke")
		if pStroke then
			pStroke.Color = palette.border
		end
		titleLbl.TextColor3 = palette.textPrimary
		titleLbl.Text =
			string.format("Collection & Deck (%d/%d)", #currentSnapshot.activeDeckUids, currentSnapshot.maxDeckSize)
		closeBtn.BackgroundColor3 = palette.bgMuted
		closeBtn.TextColor3 = palette.textPrimary

		deckStrip.BackgroundColor3 = palette.bgCard
		synergyLbl.TextColor3 = palette.accentGold
		synergyLbl.Text = string.format(
			"Deck Power: %s  |  %s",
			Components.formatNumber(currentSnapshot.totalPower),
			currentSnapshot.synergySummary
		)
		collectionScroll.BackgroundColor3 = palette.bgDeep
		inspector.BackgroundColor3 = palette.bgCard
		inspTitle.TextColor3 = palette.textPrimary
		inspStats.TextColor3 = palette.textSecondary

		for _, child in slotsRow:GetChildren() do
			if not child:IsA("UIGridLayout") then
				child:Destroy()
			end
		end

		for slotIdx = 1, currentSnapshot.maxDeckSize do
			local uid = currentSnapshot.activeDeckUids[slotIdx]
			local card = findCard(uid)
			local label = if card then card.name else "Empty Slot " .. tostring(slotIdx)
			local rStyle = if card then Theme.getRarity(card.rarity) else nil
			local bg = if rStyle then rStyle.bgTint else palette.bgMuted
			local fg = if rStyle then rStyle.glow else palette.textMuted

			local slotBtn = Components.makeButton(
				slotsRow,
				"Slot_" .. tostring(slotIdx),
				label,
				UDim2.fromOffset(80, 44),
				UDim2.fromOffset(0, 0),
				bg,
				fg
			)
			slotBtn.LayoutOrder = slotIdx
			slotBtn.TextSize = 11
			slotBtn.TextTruncate = Enum.TextTruncate.AtEnd
			slotBtn.ZIndex = 34
			if card then
				slotBtn.MouseButton1Click:Connect(function()
					selectedUid = card.uid
					updateInspector()
				end)
			end
		end

		for _, child in collectionScroll:GetChildren() do
			if not child:IsA("UIGridLayout") and not child:IsA("UIPadding") then
				child:Destroy()
			end
		end

		for idx, card in currentSnapshot.cards do
			local holder = Instance.new("TextButton")
			holder.Name = "Item_" .. card.uid
			holder.LayoutOrder = idx
			holder.Size = UDim2.fromOffset(142, 186)
			holder.BackgroundTransparency = 1
			holder.Text = ""
			holder.AutoButtonColor = false
			holder.ZIndex = 33
			holder.Parent = collectionScroll

			local view = Components.makeCardView(holder, card, UDim2.fromScale(1, 1), UDim2.fromOffset(0, 0))
			view.ZIndex = 33

			if isEquipped(card.uid) then
				local eqBadge = Instance.new("Frame")
				eqBadge.Name = "EquippedTag"
				eqBadge.Size = UDim2.fromOffset(60, 18)
				eqBadge.AnchorPoint = Vector2.new(1, 0)
				eqBadge.Position = UDim2.new(1, -6, 0, 6)
				eqBadge.BackgroundColor3 = Color3.fromRGB(20, 78, 48)
				eqBadge.BorderSizePixel = 0
				eqBadge.ZIndex = 35
				eqBadge.Parent = view
				Components.addCorner(eqBadge, 5)
				Components.addStroke(eqBadge, Color3.fromRGB(92, 224, 152), 1.2, 0)
				local eqLbl = Components.makeLabel(
					eqBadge,
					"Text",
					"EQUIPPED",
					UDim2.fromScale(1, 1),
					UDim2.fromOffset(0, 0),
					Theme.FONT_DISPLAY,
					9,
					Color3.fromRGB(142, 255, 192),
					Enum.TextXAlignment.Center
				)
				eqLbl.ZIndex = 36
			end

			if selectedUid == card.uid then
				Components.addStroke(holder, palette.accentGold, 3, 0)
			end

			holder.MouseButton1Click:Connect(function()
				selectedUid = card.uid
				renderAll()
			end)
		end

		updateInspector()
	end

	equipBtn.MouseButton1Click:Connect(function()
		if selectedUid then
			if deckTab == "Defense" and callbacks.onDefenseEquipToggle then
				callbacks.onDefenseEquipToggle(selectedUid, not isEquipped(selectedUid))
			else
				callbacks.onEquipToggle(selectedUid, not isEquipped(selectedUid))
			end
		end
	end)

	levelBtn.MouseButton1Click:Connect(function()
		if selectedUid then
			callbacks.onLevelUp(selectedUid)
		end
	end)

	ascendBtn.MouseButton1Click:Connect(function()
		if selectedUid then
			callbacks.onAscend(selectedUid)
		end
	end)

	lockBtn.MouseButton1Click:Connect(function()
		if selectedUid then
			callbacks.onToggleLock(selectedUid)
		end
	end)

	sellBtn.MouseButton1Click:Connect(function()
		local card = findCard(selectedUid)
		if card and not isEquipped(card.uid) and not card.locked then
			callbacks.onSell(card.uid)
		end
	end)

	local function updateTabVisuals()
		local p = Theme.getPalette()
		activeTabBtn.BackgroundColor3 = if deckTab == "Active" then p.accentBlue else p.bgMuted
		activeTabBtn.TextColor3 = if deckTab == "Active" then Color3.fromRGB(255, 255, 255) else p.textPrimary
		defenseTabBtn.BackgroundColor3 = if deckTab == "Defense" then p.accentPurple else p.bgMuted
		defenseTabBtn.TextColor3 = if deckTab == "Defense" then Color3.fromRGB(255, 255, 255) else p.textPrimary
	end

	activeTabBtn.MouseButton1Click:Connect(function()
		deckTab = "Active"
		updateTabVisuals()
		renderAll()
	end)

	defenseTabBtn.MouseButton1Click:Connect(function()
		deckTab = "Defense"
		updateTabVisuals()
		renderAll()
	end)

	closeBtn.MouseButton1Click:Connect(function()
		overlay.Visible = false
	end)

	overlay.MouseButton1Click:Connect(function()
		overlay.Visible = false
	end)

	local unbindTheme = Theme.onChanged(function()
		if overlay.Visible then
			renderAll()
		end
	end)

	return {
		open = function(snapshot: DeckSnapshot)
			currentSnapshot = snapshot
			if not findCard(selectedUid) and #snapshot.cards > 0 then
				selectedUid = snapshot.cards[1].uid
			end
			overlay.Visible = true
			renderAll()
		end,
		close = function()
			overlay.Visible = false
		end,
		isOpen = function(): boolean
			return overlay.Visible
		end,
		refresh = function(snapshot: DeckSnapshot)
			currentSnapshot = snapshot
			if not findCard(selectedUid) and #snapshot.cards > 0 then
				selectedUid = snapshot.cards[1].uid
			end
			if overlay.Visible then
				renderAll()
			end
		end,
		destroy = function()
			unbindTheme()
			overlay:Destroy()
		end,
	}
end

return DeckModal
