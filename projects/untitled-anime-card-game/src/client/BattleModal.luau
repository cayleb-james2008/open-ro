--!strict

local TweenService = game:GetService("TweenService")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type BattleStageInfo = {
	id: string,
	name: string,
	subtitle: string,
	recommendedPower: number,
	rewardSummary: string,
	isBoss: boolean?,
	unlocked: boolean?,
}

export type BattleUnitInit = {
	id: string?,
	cardId: string?,
	name: string?,
	displayName: string?,
	rarity: string?,
	mutation: string?,
	role: string?,
	level: number?,
	stars: number?,
	maxHp: number,
	hp: number?,
	maxEnergy: number?,
	energy: number?,
	atk: number?,
	spd: number?,
	isBoss: boolean?,
}

export type BattleEvent = {
	turn: number?,
	round: number?,
	actorSide: string,
	actorIndex: number,
	actorName: string?,
	targetSide: string?,
	targetIndex: number?,
	targetName: string?,
	skillName: string?,
	action: string?,
	damage: number?,
	heal: number?,
	isCrit: boolean?,
	crit: boolean?,
	isDodge: boolean?,
	status: string?,
	actorHpAfter: number?,
	targetHpAfter: number?,
	targetHp: number?,
	actorEnergyAfter: number?,
	targetEnergyAfter: number?,
	defeated: boolean?,
	killed: boolean?,
}

export type BattleRewards = {
	yen: number?,
	gems: number?,
	xp: number?,
	tickets: number?,
	dropCardName: string?,
	firstClearBonus: boolean?,
}

export type BattleResult = {
	stageId: string,
	stageName: string?,
	victory: boolean?,
	won: boolean?,
	rounds: number?,
	mvpName: string?,
	allies: { BattleUnitInit }?,
	enemies: { BattleUnitInit }?,
	events: { BattleEvent }?,
	rewards: BattleRewards?,
	summaryText: string?,
}

export type BattleCallbacks = {
	onStartBattle: (stageId: string) -> (),
	getReducedMotion: () -> boolean,
	onSelectWorld: ((worldId: number) -> ())?,
	onChallengeWorldBoss: ((worldId: number) -> ())?,
}

export type BattleModalController = {
	open: (stages: { BattleStageInfo }, deckPower: number, worldId: number?) -> (),
	setWorld: (worldId: number) -> (),
	playBattle: (result: BattleResult) -> (),
	setStatus: (text: string) -> (),
	close: () -> (),
	isOpen: () -> boolean,
	destroy: () -> (),
}

type UnitWidget = {
	container: Frame,
	cardFrame: Frame,
	hpFill: Frame,
	hpText: TextLabel,
	energyFill: Frame,
	koOverlay: Frame,
	maxHp: number,
	maxEnergy: number,
	hp: number,
	energy: number,
	basePos: UDim2,
	side: string,
}

local BattleModal = {}

function BattleModal.create(parent: Instance, callbacks: BattleCallbacks): BattleModalController
	local speedMult = 1
	local playbackToken = 0
	local skipRequested = false
	local currentStages: { BattleStageInfo } = {}
	local currentDeckPower = 0
	local lastStageId = "w1_act1"

	local overlay = Instance.new("TextButton")
	overlay.Name = "BattleOverlay"
	overlay.Size = UDim2.fromScale(1, 1)
	overlay.BackgroundColor3 = Color3.fromRGB(4, 8, 18)
	overlay.BackgroundTransparency = 0.35
	overlay.BorderSizePixel = 0
	overlay.Text = ""
	overlay.AutoButtonColor = false
	overlay.Visible = false
	overlay.ZIndex = 35
	overlay.Parent = parent

	local panel =
		Components.makePanel(overlay, "BattlePanel", UDim2.fromScale(0.95, 0.88), UDim2.fromScale(0.5, 0.5), 14)
	panel.AnchorPoint = Vector2.new(0.5, 0.5)
	panel.ZIndex = 36

	local panelBounds = Instance.new("UISizeConstraint")
	panelBounds.MinSize = Vector2.new(320, 380)
	panelBounds.MaxSize = Vector2.new(900, 620)
	panelBounds.Parent = panel

	local inputSink = Instance.new("TextButton")
	inputSink.Name = "InputSink"
	inputSink.Size = UDim2.fromScale(1, 1)
	inputSink.BackgroundTransparency = 1
	inputSink.Text = ""
	inputSink.AutoButtonColor = false
	inputSink.ZIndex = 36
	inputSink.Parent = panel

	-- Top Header
	local header = Instance.new("Frame")
	header.Name = "Header"
	header.Size = UDim2.new(1, 0, 0, 56)
	header.BackgroundTransparency = 1
	header.ZIndex = 37
	header.Parent = panel

	local titleLbl = Components.makeLabel(
		header,
		"Title",
		"World 1: Hidden Leaf Arena",
		UDim2.new(1, -240, 0, 28),
		UDim2.fromOffset(16, 6),
		Theme.FONT_DISPLAY,
		18,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Left
	)
	titleLbl.ZIndex = 38

	local subLbl = Components.makeLabel(
		header,
		"Subtitle",
		"Choose an Act or Boss stage to deploy your Active Deck",
		UDim2.new(1, -240, 0, 18),
		UDim2.fromOffset(16, 34),
		Theme.FONT_BODY,
		12,
		Theme.getPalette().textSecondary,
		Enum.TextXAlignment.Left
	)
	subLbl.ZIndex = 38

	local speedBtn = Components.makeButton(
		header,
		"SpeedButton",
		"1x",
		UDim2.fromOffset(56, 44),
		UDim2.new(1, -184, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	speedBtn.TextSize = 13
	speedBtn.Visible = false
	speedBtn.ZIndex = 39

	local skipBtn = Components.makeButton(
		header,
		"SkipButton",
		"Skip",
		UDim2.fromOffset(64, 44),
		UDim2.new(1, -120, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	skipBtn.TextSize = 13
	skipBtn.Visible = false
	skipBtn.ZIndex = 39

	local closeBtn = Components.makeButton(
		header,
		"CloseButton",
		"X",
		UDim2.fromOffset(44, 44),
		UDim2.new(1, -52, 0.5, -22),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	closeBtn.ZIndex = 39

	-- Stage Picker View
	local stageView = Instance.new("ScrollingFrame")
	stageView.Name = "StageView"
	stageView.Size = UDim2.new(1, -28, 1, -76)
	stageView.Position = UDim2.fromOffset(14, 64)
	stageView.BackgroundColor3 = Theme.getPalette().bgDeep
	stageView.BorderSizePixel = 0
	stageView.ScrollBarThickness = 5
	stageView.CanvasSize = UDim2.fromOffset(0, 0)
	stageView.AutomaticCanvasSize = Enum.AutomaticSize.Y
	stageView.ZIndex = 37
	stageView.Parent = panel
	Components.addCorner(stageView, 10)
	Components.addPadding(stageView, 12, 12, 12, 12)

	local stageLayout = Instance.new("UIListLayout")
	stageLayout.SortOrder = Enum.SortOrder.LayoutOrder
	stageLayout.Padding = UDim.new(0, 10)
	stageLayout.Parent = stageView

	-- Arena View
	local arenaView = Instance.new("Frame")
	arenaView.Name = "ArenaView"
	arenaView.Size = UDim2.new(1, -28, 1, -76)
	arenaView.Position = UDim2.fromOffset(14, 64)
	arenaView.BackgroundColor3 = Theme.getPalette().bgDeep
	arenaView.BorderSizePixel = 0
	arenaView.Visible = false
	arenaView.ClipsDescendants = true
	arenaView.ZIndex = 37
	arenaView.Parent = panel
	Components.addCorner(arenaView, 10)
	Components.addStroke(arenaView, Theme.getPalette().border, 1, 0.25)

	local turnBanner = Components.makeLabel(
		arenaView,
		"TurnBanner",
		"Turn 1",
		UDim2.new(1, -20, 0, 26),
		UDim2.fromOffset(10, 8),
		Theme.FONT_DISPLAY,
		14,
		Theme.getPalette().accentGold,
		Enum.TextXAlignment.Center
	)
	turnBanner.ZIndex = 38

	local actionBanner = Components.makeLabel(
		arenaView,
		"ActionBanner",
		"Preparing battle...",
		UDim2.new(1, -20, 0, 20),
		UDim2.fromOffset(10, 32),
		Theme.FONT_BODY,
		12,
		Theme.getPalette().textSecondary,
		Enum.TextXAlignment.Center
	)
	actionBanner.ZIndex = 38

	local lanesFrame = Instance.new("Frame")
	lanesFrame.Name = "Lanes"
	lanesFrame.Size = UDim2.new(1, -20, 1, -72)
	lanesFrame.Position = UDim2.fromOffset(10, 58)
	lanesFrame.BackgroundTransparency = 1
	lanesFrame.ZIndex = 38
	lanesFrame.Parent = arenaView

	local allyLane = Instance.new("Frame")
	allyLane.Name = "AllyLane"
	allyLane.Size = UDim2.fromScale(0.46, 1)
	allyLane.Position = UDim2.fromScale(0, 0)
	allyLane.BackgroundTransparency = 1
	allyLane.ZIndex = 38
	allyLane.Parent = lanesFrame

	local enemyLane = Instance.new("Frame")
	enemyLane.Name = "EnemyLane"
	enemyLane.Size = UDim2.fromScale(0.46, 1)
	enemyLane.Position = UDim2.fromScale(0.54, 0)
	enemyLane.BackgroundTransparency = 1
	enemyLane.ZIndex = 38
	enemyLane.Parent = lanesFrame

	local popupLayer = Instance.new("Frame")
	popupLayer.Name = "PopupLayer"
	popupLayer.Size = UDim2.fromScale(1, 1)
	popupLayer.BackgroundTransparency = 1
	popupLayer.ZIndex = 48
	popupLayer.Parent = arenaView

	-- Rewards Overlay inside Arena
	local rewardsCard =
		Components.makePanel(arenaView, "RewardsPopup", UDim2.fromOffset(360, 260), UDim2.fromScale(0.5, 0.5), 14)
	rewardsCard.AnchorPoint = Vector2.new(0.5, 0.5)
	rewardsCard.Visible = false
	rewardsCard.ZIndex = 50

	local rewardsTitle = Components.makeLabel(
		rewardsCard,
		"RewardsTitle",
		"VICTORY!",
		UDim2.new(1, -24, 0, 34),
		UDim2.fromOffset(12, 14),
		Theme.FONT_DISPLAY,
		24,
		Theme.getPalette().accentGold,
		Enum.TextXAlignment.Center
	)
	rewardsTitle.ZIndex = 51

	local rewardsSummary = Components.makeLabel(
		rewardsCard,
		"RewardsSummary",
		"",
		UDim2.new(1, -28, 0, 120),
		UDim2.fromOffset(14, 54),
		Theme.FONT_BODY,
		14,
		Theme.getPalette().textPrimary,
		Enum.TextXAlignment.Center
	)
	rewardsSummary.TextWrapped = true
	rewardsSummary.ZIndex = 51

	local replayBtn = Components.makeButton(
		rewardsCard,
		"ReplayButton",
		"Battle Again",
		UDim2.fromOffset(146, 46),
		UDim2.new(0.5, -154, 1, -60),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	replayBtn.TextSize = 14
	replayBtn.ZIndex = 52

	local stagesBackBtn = Components.makeButton(
		rewardsCard,
		"StagesButton",
		"Stages",
		UDim2.fromOffset(146, 46),
		UDim2.new(0.5, 8, 1, -60),
		Theme.getPalette().bgMuted,
		Theme.getPalette().textPrimary
	)
	stagesBackBtn.TextSize = 14
	stagesBackBtn.ZIndex = 52

	local function clearChildren(container: Instance, keepLayout: boolean)
		for _, child in container:GetChildren() do
			if not (keepLayout and child:IsA("UILayout")) then
				child:Destroy()
			end
		end
	end

	local function applyTheme()
		local palette = Theme.getPalette()
		panel.BackgroundColor3 = palette.bgPanel
		local pStroke = panel:FindFirstChildOfClass("UIStroke")
		if pStroke then
			pStroke.Color = palette.border
		end
		titleLbl.TextColor3 = palette.textPrimary
		subLbl.TextColor3 = palette.textSecondary
		stageView.BackgroundColor3 = palette.bgDeep
		arenaView.BackgroundColor3 = palette.bgDeep
		rewardsCard.BackgroundColor3 = palette.bgPanel
		rewardsSummary.TextColor3 = palette.textPrimary
		closeBtn.BackgroundColor3 = palette.bgMuted
		closeBtn.TextColor3 = palette.textPrimary
		skipBtn.BackgroundColor3 = palette.bgMuted
		skipBtn.TextColor3 = palette.textPrimary
		stagesBackBtn.BackgroundColor3 = palette.bgMuted
		stagesBackBtn.TextColor3 = palette.textPrimary
		speedBtn.BackgroundColor3 = if speedMult > 1 then palette.accentPurple else palette.bgMuted
		speedBtn.TextColor3 = if speedMult > 1 then Color3.fromRGB(255, 255, 255) else palette.textPrimary
	end

	local WORLD_TITLES: { [number]: string } = {
		[1] = "World 1: Hidden Leaf Arena",
		[2] = "World 2: West City & Namek",
		[3] = "World 3: Grand Line Seas",
		[4] = "World 4: Tokyo Jujutsu High",
		[5] = "World 5: Soul Society",
	}
	local function getWorldTitle(wId: number): string
		return WORLD_TITLES[wId] or ("World " .. tostring(wId))
	end

	local activeWorldId = 1
	local activeBondsText = "Active Bonds: Equip matching cards for Show & Cross-Card bonuses"
	local onSelectWorldCb: ((worldId: number) -> ())? = callbacks.onSelectWorld
	local onChallengeWorldBossCb: ((worldId: number) -> ())? = callbacks.onChallengeWorldBoss

	local function renderStageList()
		clearChildren(stageView, true)
		local palette = Theme.getPalette()

		local worldBar = Instance.new("Frame")
		worldBar.Name = "WorldTabsBar"
		worldBar.LayoutOrder = -3
		worldBar.Size = UDim2.new(1, 0, 0, 48)
		worldBar.BackgroundTransparency = 1
		worldBar.ZIndex = 38
		worldBar.Parent = stageView

		local worldPad = Instance.new("UIPadding")
		worldPad.PaddingRight = UDim.new(0, 10)
		worldPad.Parent = worldBar

		local worldLayout = Instance.new("UIListLayout")
		worldLayout.FillDirection = Enum.FillDirection.Horizontal
		worldLayout.Padding = UDim.new(0, 6)
		worldLayout.VerticalAlignment = Enum.VerticalAlignment.Center
		worldLayout.Parent = worldBar

		for wId = 1, 5 do
			local isSelected = wId == activeWorldId
			local wBtn = Components.makeButton(
				worldBar,
				"WorldTab_" .. tostring(wId),
				"W" .. tostring(wId),
				UDim2.fromOffset(46, 44),
				UDim2.fromOffset(0, 0),
				if isSelected then palette.accentBlue else palette.bgMuted,
				if isSelected then Color3.fromRGB(255, 255, 255) else palette.textPrimary
			)
			wBtn.LayoutOrder = wId
			wBtn.TextSize = 13
			wBtn.ZIndex = 39
			wBtn.Activated:Connect(function()
				activeWorldId = wId
				titleLbl.Text = getWorldTitle(activeWorldId)
				if onSelectWorldCb then
					onSelectWorldCb(wId)
				end
				renderStageList()
			end)
		end

		local wbBtn = Components.makeButton(
			worldBar,
			"WorldBossButton",
			"World Boss",
			UDim2.new(1, -260, 0, 44),
			UDim2.fromOffset(0, 0),
			palette.accentPurple,
			Color3.fromRGB(255, 255, 255)
		)
		wbBtn.LayoutOrder = 10
		wbBtn.TextSize = 13
		wbBtn.ZIndex = 39
		wbBtn.Activated:Connect(function()
			if onChallengeWorldBossCb then
				onChallengeWorldBossCb(activeWorldId)
			end
		end)

		local bondsRow = Instance.new("Frame")
		bondsRow.Name = "BondsSummaryRow"
		bondsRow.LayoutOrder = -2
		bondsRow.Size = UDim2.new(1, 0, 0, 36)
		bondsRow.BackgroundColor3 = palette.bgCard
		bondsRow.BorderSizePixel = 0
		bondsRow.ZIndex = 38
		bondsRow.Parent = stageView
		Components.addCorner(bondsRow, 8)
		Components.addStroke(bondsRow, palette.accentGold, 1, 0.45)

		local bondsLbl = Components.makeLabel(
			bondsRow,
			"BondsLabel",
			activeBondsText,
			UDim2.new(1, -16, 1, 0),
			UDim2.fromOffset(8, 0),
			Theme.FONT_BODY,
			12,
			palette.accentGold,
			Enum.TextXAlignment.Left
		)
		bondsLbl.TextTruncate = Enum.TextTruncate.AtEnd
		bondsLbl.ZIndex = 39

		for idx, stage in currentStages do
			local stageAny = stage :: any
			local stageId = tostring(stage.id or stageAny.stageId or ("stage_" .. tostring(idx)))
			local stageName = tostring(stage.name or stageId)
			local stageSubtitle = tostring(stage.subtitle or ("Act " .. tostring(stageAny.actNumber or idx)))
			local stageRewardSummary = tostring(
				stage.rewardSummary
					or string.format(
						"+%d Yen  +%d Gems",
						tonumber(stageAny.yenReward) or 0,
						tonumber(stageAny.gemsReward) or 0
					)
			)
			local row = Instance.new("Frame")
			row.Name = "Stage_" .. stageId
			row.LayoutOrder = idx
			row.Size = UDim2.new(1, 0, 0, 88)
			row.BackgroundColor3 = palette.bgCard
			row.BorderSizePixel = 0
			row.ZIndex = 38
			row.Parent = stageView
			Components.addCorner(row, 10)
			Components.addStroke(
				row,
				if stage.isBoss then palette.accentRed else palette.border,
				if stage.isBoss then 2 else 1.5,
				0.1
			)

			local nameText = stageName
			if stage.isBoss then
				local stripped = string.gsub(stageName, "^%s*%[?[Bb][Oo][Ss][Ss]%]?:?%s*", "")
				nameText = "BOSS: " .. stripped
			end
			local nameLbl = Components.makeLabel(
				row,
				"Name",
				nameText,
				UDim2.new(1, -152, 0, 26),
				UDim2.fromOffset(12, 8),
				Theme.FONT_DISPLAY,
				15,
				if stage.isBoss then palette.accentGold else palette.textPrimary,
				Enum.TextXAlignment.Left
			)
			nameLbl.ZIndex = 39

			local infoLbl = Components.makeLabel(
				row,
				"Info",
				string.format(
					"%s  |  Rec Power: %s  (Yours: %s)",
					stageSubtitle,
					Components.formatNumber(tonumber(stage.recommendedPower) or 0),
					Components.formatNumber(currentDeckPower)
				),
				UDim2.new(1, -152, 0, 20),
				UDim2.fromOffset(12, 34),
				Theme.FONT_BODY,
				12,
				palette.textSecondary,
				Enum.TextXAlignment.Left
			)
			infoLbl.ZIndex = 39

			local rewardLbl = Components.makeLabel(
				row,
				"Reward",
				"Rewards: " .. stageRewardSummary,
				UDim2.new(1, -152, 0, 18),
				UDim2.fromOffset(12, 58),
				Theme.FONT_MONO,
				11,
				palette.accentGreen,
				Enum.TextXAlignment.Left
			)
			rewardLbl.ZIndex = 39

			local unlocked = stage.unlocked ~= false
			local fightBtn = Components.makeButton(
				row,
				"FightButton",
				if unlocked then "Battle" else "Locked",
				UDim2.fromOffset(122, 48),
				UDim2.new(1, -134, 0.5, -24),
				if not unlocked then palette.bgMuted elseif stage.isBoss then palette.accentRed else palette.accentBlue,
				if unlocked then Color3.fromRGB(255, 255, 255) else palette.textSecondary
			)
			fightBtn.TextSize = 14
			fightBtn.ZIndex = 40
			if unlocked then
				fightBtn.MouseButton1Click:Connect(function()
					lastStageId = stageId
					subLbl.Text = "Starting " .. stageName .. "..."
					callbacks.onStartBattle(stageId)
				end)
			end
		end
	end

	local function buildUnitWidget(
		lane: Frame,
		unit: BattleUnitInit,
		index: number,
		total: number,
		side: string
	): UnitWidget
		local palette = Theme.getPalette()
		local slotHeight = if total <= 2 then 124 else math.clamp(math.floor(336 / math.max(total, 1)), 82, 108)
		local yStep = 1 / math.max(total, 1)
		local yCenter = (index - 0.5) * yStep

		local container = Instance.new("Frame")
		container.Name = side .. "_" .. tostring(index)
		container.Size = UDim2.new(0.94, 0, 0, slotHeight)
		container.Position = UDim2.fromScale(0.5, yCenter)
		container.AnchorPoint = Vector2.new(0.5, 0.5)
		container.BackgroundTransparency = 1
		container.ZIndex = 39
		container.Parent = lane

		local unitId = unit.cardId or unit.id or ("unit_" .. tostring(index))
		local unitName = unit.name or unit.displayName or unitId
		local unitMaxHp = math.max(unit.maxHp or unit.hp or 1, 1)
		local cardData = {
			uid = unitId .. "_" .. tostring(index),
			id = unitId,
			cardId = unitId,
			name = unitName,
			rarity = unit.rarity or "Common",
			mutation = unit.mutation or "Normal",
			role = unit.role or "Striker",
			level = unit.level or 1,
			stars = unit.stars or 0,
			hp = unitMaxHp,
			atk = unit.atk or 0,
			spd = unit.spd or 100,
		}
		local cardFrame = Components.makeCardView(container, cardData, UDim2.new(1, 0, 1, -24), UDim2.fromOffset(0, 0))
		cardFrame.ZIndex = 40

		local portraitBox = cardFrame:FindFirstChild("PortraitBox")
		if portraitBox and portraitBox:IsA("GuiObject") then
			portraitBox.Size = UDim2.new(1, -10, 0.44, 0)
		end
		local cardName = cardFrame:FindFirstChild("CardName")
		if cardName and cardName:IsA("GuiObject") then
			cardName.Position = UDim2.new(0, 6, 0.46, 2)
			cardName.Size = UDim2.new(1, -12, 0, 16)
		end
		local metaLbl = cardFrame:FindFirstChild("Meta")
		if metaLbl and metaLbl:IsA("GuiObject") then
			metaLbl.Position = UDim2.new(0, 6, 0.46, 20)
			metaLbl.Size = UDim2.new(1, -12, 0, 14)
		end
		local statsLbl = cardFrame:FindFirstChild("Stats")
		if statsLbl and statsLbl:IsA("GuiObject") then
			statsLbl.Position = UDim2.new(0, 6, 1, -16)
			statsLbl.Size = UDim2.new(1, -12, 0, 14)
		end

		local hpBack = Instance.new("Frame")
		hpBack.Name = "HpBar"
		hpBack.Size = UDim2.new(1, 0, 0, 12)
		hpBack.Position = UDim2.new(0, 0, 1, -22)
		hpBack.BackgroundColor3 = palette.bgMuted
		hpBack.BorderSizePixel = 0
		hpBack.ZIndex = 40
		hpBack.Parent = container
		Components.addCorner(hpBack, 4)

		local startHp = unit.hp or unitMaxHp
		local hpRatio = math.clamp(startHp / unitMaxHp, 0, 1)
		local hpTrail = Instance.new("Frame")
		hpTrail.Name = "Trail"
		hpTrail.Size = UDim2.fromScale(hpRatio, 1)
		hpTrail.BackgroundColor3 = Color3.fromRGB(255, 240, 180)
		hpTrail.BackgroundTransparency = 0.25
		hpTrail.BorderSizePixel = 0
		hpTrail.ZIndex = 40
		hpTrail.Parent = hpBack
		Components.addCorner(hpTrail, 4)

		local hpFill = Instance.new("Frame")
		hpFill.Name = "Fill"
		hpFill.Size = UDim2.fromScale(hpRatio, 1)
		hpFill.BackgroundColor3 = if side == "Ally" then palette.accentGreen else palette.accentRed
		hpFill.BorderSizePixel = 0
		hpFill.ZIndex = 41
		hpFill.Parent = hpBack
		Components.addCorner(hpFill, 4)

		local hpText = Components.makeLabel(
			hpBack,
			"HpText",
			Components.formatNumber(startHp) .. " / " .. Components.formatNumber(unitMaxHp),
			UDim2.fromScale(1, 1),
			UDim2.fromOffset(0, 0),
			Theme.FONT_MONO,
			9,
			Color3.fromRGB(255, 255, 255),
			Enum.TextXAlignment.Center
		)
		hpText.TextStrokeTransparency = 0.35
		hpText.ZIndex = 42

		local maxEnergy = unit.maxEnergy or 100
		local startEnergy = unit.energy or 0
		local energyBack = Instance.new("Frame")
		energyBack.Name = "EnergyBar"
		energyBack.Size = UDim2.new(1, 0, 0, 6)
		energyBack.Position = UDim2.new(0, 0, 1, -7)
		energyBack.BackgroundColor3 = palette.bgMuted
		energyBack.BorderSizePixel = 0
		energyBack.ZIndex = 40
		energyBack.Parent = container
		Components.addCorner(energyBack, 3)

		local energyFill = Instance.new("Frame")
		energyFill.Name = "Fill"
		energyFill.Size = UDim2.fromScale(math.clamp(startEnergy / math.max(maxEnergy, 1), 0, 1), 1)
		energyFill.BackgroundColor3 = palette.accentGold
		energyFill.BorderSizePixel = 0
		energyFill.ZIndex = 41
		energyFill.Parent = energyBack
		Components.addCorner(energyFill, 3)

		local koOverlay = Instance.new("Frame")
		koOverlay.Name = "KO"
		koOverlay.Size = UDim2.fromScale(1, 1)
		koOverlay.BackgroundColor3 = Color3.fromRGB(10, 12, 20)
		koOverlay.BackgroundTransparency = 0.45
		koOverlay.BorderSizePixel = 0
		koOverlay.Visible = startHp <= 0
		koOverlay.ZIndex = 44
		koOverlay.Parent = cardFrame
		Components.addCorner(koOverlay, 10)
		Components.makeLabel(
			koOverlay,
			"Label",
			"KO",
			UDim2.fromScale(1, 1),
			UDim2.fromOffset(0, 0),
			Theme.FONT_DISPLAY,
			18,
			palette.accentRed,
			Enum.TextXAlignment.Center
		).ZIndex =
			45

		return {
			container = container,
			cardFrame = cardFrame,
			hpFill = hpFill,
			hpTrail = hpTrail,
			hpText = hpText,
			energyFill = energyFill,
			koOverlay = koOverlay,
			maxHp = unitMaxHp,
			maxEnergy = math.max(maxEnergy, 1),
			hp = startHp,
			energy = startEnergy,
			basePos = container.Position,
			side = side,
		}
	end

	local function spawnSlashAndFlash(target: UnitWidget, isAllyActor: boolean, isCrit: boolean)
		local absArena = arenaView.AbsolutePosition
		local absTarget = target.container.AbsolutePosition
		local absSize = target.container.AbsoluteSize
		local cx = (absTarget.X - absArena.X) + absSize.X * 0.5
		local cy = (absTarget.Y - absArena.Y) + absSize.Y * 0.45

		local slash = Instance.new("Frame")
		slash.Name = "SlashStreak"
		slash.Size = UDim2.fromOffset(if isCrit then 110 else 80, if isCrit then 8 else 5)
		slash.Position = UDim2.fromOffset(cx, cy)
		slash.AnchorPoint = Vector2.new(0.5, 0.5)
		slash.Rotation = if isAllyActor then -32 else 32
		slash.BackgroundColor3 = if isCrit then Color3.fromRGB(255, 224, 92) else Color3.fromRGB(255, 255, 255)
		slash.BorderSizePixel = 0
		slash.ZIndex = 48
		slash.Parent = popupLayer
		Components.addCorner(slash, 4)

		local dur = 0.18 / speedMult
		TweenService:Create(slash, TweenInfo.new(dur, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Size = UDim2.fromOffset(if isCrit then 150 else 110, 2),
			BackgroundTransparency = 1,
		}):Play()
		task.delay(dur + 0.03, function()
			if slash.Parent then
				slash:Destroy()
			end
		end)

		if not callbacks.getReducedFlash() then
			local hitFlash = Instance.new("Frame")
			hitFlash.Name = "HitFlash"
			hitFlash.Size = UDim2.fromScale(1, 1)
			hitFlash.BackgroundColor3 = if isCrit then Color3.fromRGB(255, 232, 140) else Color3.fromRGB(255, 255, 255)
			hitFlash.BackgroundTransparency = 0.42
			hitFlash.BorderSizePixel = 0
			hitFlash.ZIndex = 46
			hitFlash.Parent = target.cardFrame
			Components.addCorner(hitFlash, 10)
			TweenService:Create(hitFlash, TweenInfo.new(dur, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
				BackgroundTransparency = 1,
			}):Play()
			task.delay(dur + 0.03, function()
				if hitFlash.Parent then
					hitFlash:Destroy()
				end
			end)
		end
	end

	local function spawnFloatingText(target: UnitWidget, text: string, color: Color3, big: boolean)
		local absArena = arenaView.AbsolutePosition
		local absTarget = target.container.AbsolutePosition
		local absSize = target.container.AbsoluteSize
		local relX = (absTarget.X - absArena.X) + absSize.X * 0.5
		local relY = (absTarget.Y - absArena.Y) + absSize.Y * 0.25

		local lbl = Components.makeLabel(
			popupLayer,
			"DamagePopup",
			text,
			UDim2.fromOffset(140, 28),
			UDim2.fromOffset(relX - 70, relY),
			Theme.FONT_DISPLAY,
			if big then 20 else 15,
			color,
			Enum.TextXAlignment.Center
		)
		lbl.TextStrokeTransparency = 0.2
		lbl.ZIndex = 49

		local dur = if callbacks.getReducedMotion() then 0.25 else (0.65 / speedMult)
		local tween = TweenService:Create(lbl, TweenInfo.new(dur, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
			Position = UDim2.fromOffset(relX - 70, relY - 38),
			TextTransparency = 1,
			TextStrokeTransparency = 1,
		})
		tween:Play()
		task.delay(dur + 0.05, function()
			if lbl.Parent then
				lbl:Destroy()
			end
		end)
	end

	local function updateUnitBars(
		w: UnitWidget,
		newHp: number?,
		newEnergy: number?,
		defeated: boolean?,
		instant: boolean?
	)
		local barDur = if instant or callbacks.getReducedMotion() then 0 else (0.18 / speedMult)
		if newHp ~= nil then
			w.hp = math.max(0, newHp)
			local ratio = math.clamp(w.hp / w.maxHp, 0, 1)
			if barDur > 0 then
				TweenService:Create(w.hpFill, TweenInfo.new(barDur, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
					Size = UDim2.fromScale(ratio, 1),
				}):Play()
				local trail = (w :: any).hpTrail :: Frame?
				if trail then
					task.delay(0.08 / speedMult, function()
						if trail.Parent then
							TweenService:Create(
								trail,
								TweenInfo.new(barDur * 1.5, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
								{ Size = UDim2.fromScale(ratio, 1) }
							):Play()
						end
					end)
				end
			else
				w.hpFill.Size = UDim2.fromScale(ratio, 1)
				local trail = (w :: any).hpTrail :: Frame?
				if trail then
					trail.Size = UDim2.fromScale(ratio, 1)
				end
			end
			w.hpText.Text = Components.formatNumber(w.hp) .. " / " .. Components.formatNumber(w.maxHp)
			if w.hp <= 0 or defeated then
				w.koOverlay.Visible = true
			end
		elseif defeated then
			w.hp = 0
			w.hpFill.Size = UDim2.fromScale(0, 1)
			w.hpText.Text = "0 / " .. Components.formatNumber(w.maxHp)
			w.koOverlay.Visible = true
		end
		if newEnergy ~= nil then
			w.energy = math.clamp(newEnergy, 0, w.maxEnergy)
			local eRatio = w.energy / w.maxEnergy
			if barDur > 0 then
				TweenService
					:Create(w.energyFill, TweenInfo.new(barDur, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
						Size = UDim2.fromScale(eRatio, 1),
					})
					:Play()
			else
				w.energyFill.Size = UDim2.fromScale(eRatio, 1)
			end
		end
	end

	local function showRewards(result: BattleResult)
		local palette = Theme.getPalette()
		local isVictory = if result.victory ~= nil then result.victory else (result.won == true)
		rewardsTitle.Text = if isVictory then "VICTORY!" else "DEFEAT"
		rewardsTitle.TextColor3 = if isVictory then palette.accentGold else palette.accentRed

		local lines: { string } = {}
		if result.summaryText and result.summaryText ~= "" then
			table.insert(lines, result.summaryText)
		end
		if result.mvpName and result.mvpName ~= "" then
			table.insert(lines, "MVP: " .. result.mvpName)
		end
		local rw = result.rewards
		if rw then
			local parts: { string } = {}
			if (rw.yen or 0) > 0 then
				table.insert(parts, "+" .. Components.formatNumber(rw.yen or 0) .. " Yen")
			end
			if (rw.gems or 0) > 0 then
				table.insert(parts, "+" .. Components.formatNumber(rw.gems or 0) .. " Gems")
			end
			if (rw.xp or 0) > 0 then
				table.insert(parts, "+" .. Components.formatNumber(rw.xp or 0) .. " XP")
			end
			if (rw.tickets or 0) > 0 then
				table.insert(parts, "+" .. Components.formatNumber(rw.tickets or 0) .. " Tickets")
			end
			if #parts > 0 then
				table.insert(lines, table.concat(parts, "   "))
			end
			if rw.firstClearBonus then
				table.insert(lines, "First Clear Bonus Awarded!")
			end
			if rw.dropCardName and rw.dropCardName ~= "" then
				table.insert(lines, "Boss Drop: " .. rw.dropCardName)
			end
		elseif not isVictory then
			table.insert(lines, "Roll or level up your cards in Deck to grow stronger!")
		end

		rewardsSummary.Text = table.concat(lines, "\n")
		rewardsCard.Visible = true
		skipBtn.Visible = false
		speedBtn.Visible = false
	end

	local function playBattle(result: BattleResult)
		playbackToken += 1
		local token = playbackToken
		skipRequested = false
		lastStageId = result.stageId or lastStageId
		local isVictory = if result.victory ~= nil then result.victory else (result.won == true)
		local alliesList = result.allies or {}
		local enemiesList = result.enemies or {}
		local eventsList = result.events or {}

		overlay.Visible = true
		stageView.Visible = false
		arenaView.Visible = true
		rewardsCard.Visible = false
		speedBtn.Visible = true
		skipBtn.Visible = true
		applyTheme()

		local displayStageName: string? = if result.stageName
				and result.stageName ~= ""
				and result.stageName ~= result.stageId
			then result.stageName
			else nil
		if not displayStageName and result.stageId then
			for _, st in currentStages do
				if st.id == result.stageId and st.name ~= "" then
					displayStageName = st.name
					break
				end
			end
		end
		titleLbl.Text = displayStageName or "World Battle"
		subLbl.Text = "Auto-Battle Replay"

		clearChildren(allyLane, false)
		clearChildren(enemyLane, false)
		clearChildren(popupLayer, false)

		local allyWidgets: { UnitWidget } = {}
		local enemyWidgets: { UnitWidget } = {}

		for i, u in alliesList do
			allyWidgets[i] = buildUnitWidget(allyLane, u, i, #alliesList, "Ally")
		end
		for i, u in enemiesList do
			enemyWidgets[i] = buildUnitWidget(enemyLane, u, i, #enemiesList, "Enemy")
		end

		local function getWidget(side: string?, idx: number?): UnitWidget?
			if not side or not idx then
				return nil
			end
			local norm = string.lower(side)
			if norm == "ally" then
				return allyWidgets[idx]
			elseif norm == "enemy" then
				return enemyWidgets[idx]
			end
			return nil
		end

		task.spawn(function()
			local palette = Theme.getPalette()
			for _, ev in eventsList do
				if token ~= playbackToken or not overlay.Visible then
					return
				end

				local isAllyActor = string.lower(tostring(ev.actorSide or "")) == "ally"
				local actor = getWidget(ev.actorSide, ev.actorIndex)
				local target = getWidget(ev.targetSide, ev.targetIndex)
				local targetHpAfter = if ev.targetHpAfter ~= nil then ev.targetHpAfter else ev.targetHp
				local isDefeated = ev.defeated == true or ev.killed == true
				local isCrit = ev.isCrit == true or ev.crit == true

				if skipRequested then
					if actor then
						updateUnitBars(actor, ev.actorHpAfter, ev.actorEnergyAfter, false, true)
					end
					if target then
						updateUnitBars(target, targetHpAfter, ev.targetEnergyAfter, isDefeated, true)
					end
					continue
				end

				local turnNum = ev.turn or ev.round or 1
				turnBanner.Text = "Turn " .. tostring(turnNum)
				local skillText = ev.skillName or ev.action or "Attack"
				local actorUnit = if isAllyActor
					then alliesList[ev.actorIndex or 1]
					else enemiesList[ev.actorIndex or 1]
				local unitName = if actorUnit then (actorUnit.name or actorUnit.displayName) else nil
				local actorLabel = if ev.actorName and ev.actorName ~= ""
					then ev.actorName
					elseif unitName and unitName ~= "" then unitName
					else ((if isAllyActor then "Ally #" else "Enemy #") .. tostring(ev.actorIndex or 1))
				actionBanner.Text = string.format("%s uses %s", actorLabel, skillText)

				local playSfx = (callbacks :: any).playSfx :: ((cue: string) -> ())?
				local lungeDir = if isAllyActor then 1 else -1
				local lungeDur = 0.11 / speedMult
				local hitstopDur = 0.06 / speedMult
				local recoilDur = 0.13 / speedMult

				if actor and not callbacks.getReducedMotion() then
					local lungePos = UDim2.new(
						actor.basePos.X.Scale + lungeDir * 0.24,
						actor.basePos.X.Offset,
						actor.basePos.Y.Scale,
						actor.basePos.Y.Offset
					)
					local t1 = TweenService:Create(
						actor.container,
						TweenInfo.new(lungeDur, Enum.EasingStyle.Back, Enum.EasingDirection.Out),
						{ Position = lungePos }
					)
					t1:Play()
					task.wait(lungeDur)
				end

				if token ~= playbackToken then
					return
				end

				if actor then
					updateUnitBars(actor, ev.actorHpAfter, ev.actorEnergyAfter, false, false)
				end
				if target then
					updateUnitBars(target, targetHpAfter, ev.targetEnergyAfter, isDefeated, false)
					if ev.isDodge then
						spawnFloatingText(target, "DODGE", palette.accentBlue, false)
					elseif (ev.damage or 0) > 0 then
						local prefix = if isCrit then "CRIT! -" else "-"
						local col = if isCrit then palette.accentGold else Color3.fromRGB(255, 196, 86)
						spawnFloatingText(target, prefix .. Components.formatNumber(ev.damage or 0), col, isCrit)
						spawnSlashAndFlash(target, isAllyActor, isCrit)
						if playSfx then
							playSfx(if isCrit then "battle_crit" else "ui_click")
						end
						if not callbacks.getReducedMotion() then
							local hitOffset = UDim2.fromOffset(if isAllyActor then 7 else -7, 0)
							target.cardFrame.Position = hitOffset
							TweenService:Create(
								target.cardFrame,
								TweenInfo.new(0.12 / speedMult, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
								{ Position = UDim2.fromOffset(0, 0) }
							):Play()
							if isCrit then
								arenaView.Position = UDim2.fromOffset(12, 56)
								TweenService
									:Create(
										arenaView,
										TweenInfo.new(0.12 / speedMult, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
										{ Position = UDim2.fromOffset(12, 60) }
									)
									:Play()
							end
						end
					elseif (ev.heal or 0) > 0 then
						spawnFloatingText(
							target,
							"+" .. Components.formatNumber(ev.heal or 0) .. " HP",
							palette.accentGreen,
							false
						)
					end
					local evAny = ev :: any
					if (evAny.shield or 0) > 0 then
						spawnFloatingText(
							target,
							"SHIELD +" .. Components.formatNumber(evAny.shield),
							Color3.fromRGB(72, 224, 240),
							false
						)
					elseif evAny.statusApplied and evAny.statusApplied ~= "" then
						local stUpper = string.upper(tostring(evAny.statusApplied))
						local stCol = if stUpper == "BURN" then palette.accentRed else palette.accentPurple
						spawnFloatingText(target, stUpper, stCol, false)
					end
				end

				if not callbacks.getReducedMotion() then
					task.wait(hitstopDur)
				end

				if actor and not callbacks.getReducedMotion() then
					local t2 = TweenService:Create(
						actor.container,
						TweenInfo.new(recoilDur, Enum.EasingStyle.Quad, Enum.EasingDirection.InOut),
						{ Position = actor.basePos }
					)
					t2:Play()
					task.wait(recoilDur)
				elseif not skipRequested then
					task.wait(0.08 / speedMult)
				end
			end

			if token ~= playbackToken then
				return
			end
			turnBanner.Text = if isVictory then "Stage Cleared" else "Stage Failed"
			actionBanner.Text = if isVictory then "Victory!" else "Try upgrading your deck."
			showRewards(result)
		end)
	end

	speedBtn.MouseButton1Click:Connect(function()
		speedMult = if speedMult == 1 then 2 elseif speedMult == 2 then 4 else 1
		speedBtn.Text = tostring(speedMult) .. "x"
		applyTheme()
	end)

	skipBtn.MouseButton1Click:Connect(function()
		skipRequested = true
	end)

	replayBtn.MouseButton1Click:Connect(function()
		rewardsCard.Visible = false
		callbacks.onStartBattle(lastStageId)
	end)

	stagesBackBtn.MouseButton1Click:Connect(function()
		playbackToken += 1
		arenaView.Visible = false
		stageView.Visible = true
		speedBtn.Visible = false
		skipBtn.Visible = false
		titleLbl.Text = getWorldTitle(activeWorldId)
		subLbl.Text = "Choose an Act or Boss stage to deploy your Active Deck"
	end)

	local function closeModal()
		playbackToken += 1
		overlay.Visible = false
	end

	closeBtn.MouseButton1Click:Connect(closeModal)
	overlay.MouseButton1Click:Connect(closeModal)

	local unbindTheme = Theme.onChanged(function()
		applyTheme()
		if stageView.Visible then
			renderStageList()
		end
	end)

	return {
		open = function(stages: { BattleStageInfo }, deckPower: number, worldId: number?)
			playbackToken += 1
			if worldId ~= nil then
				activeWorldId = worldId
			end
			currentStages = stages
			currentDeckPower = deckPower
			stageView.Visible = true
			arenaView.Visible = false
			rewardsCard.Visible = false
			speedBtn.Visible = false
			skipBtn.Visible = false
			titleLbl.Text = getWorldTitle(activeWorldId)
			subLbl.Text = "Choose an Act or Boss stage to deploy your Active Deck"
			applyTheme()
			renderStageList()
			overlay.Visible = true
		end,
		setWorld = function(worldId: number)
			activeWorldId = worldId
			titleLbl.Text = getWorldTitle(activeWorldId)
			if stageView.Visible then
				renderStageList()
			end
		end,
		playBattle = playBattle,
		setStatus = function(text: string)
			subLbl.Text = text
		end,
		close = closeModal,
		isOpen = function(): boolean
			return overlay.Visible
		end,
		destroy = function()
			playbackToken += 1
			unbindTheme()
			overlay:Destroy()
		end,
	}
end

return BattleModal
