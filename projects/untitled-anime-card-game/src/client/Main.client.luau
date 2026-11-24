--!strict

local Lighting = game:GetService("Lighting")
local Players = game:GetService("Players")
local ProximityPromptService = game:GetService("ProximityPromptService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local SoundService = game:GetService("SoundService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Net = require(Shared:WaitForChild("Net"))
local Config = Shared:WaitForChild("Config")
local Banners = require(Config:WaitForChild("Banners"))
local Worlds = require(Config:WaitForChild("Worlds"))

local ClientFolder = script.Parent
local Theme = require(ClientFolder:WaitForChild("Theme"))
local Hud = require(ClientFolder:WaitForChild("Hud"))
local RevealModal = require(ClientFolder:WaitForChild("RevealModal"))
local DeckModal = require(ClientFolder:WaitForChild("DeckModal"))
local BattleModal = require(ClientFolder:WaitForChild("BattleModal"))
local SettingsModal = require(ClientFolder:WaitForChild("SettingsModal"))
local EndgameModals = require(ClientFolder:WaitForChild("EndgameModals"))
local MetaModals = require(ClientFolder:WaitForChild("MetaModals"))

local localPlayer = Players.LocalPlayer
local playerGui = localPlayer:WaitForChild("PlayerGui")

local screenGui = Instance.new("ScreenGui")
screenGui.Name = "UACGMainGui"
screenGui.ResetOnSpawn = false
screenGui.IgnoreGuiInset = false
pcall(function()
	(screenGui :: any).ScreenInsets = (Enum :: any).ScreenInsets.CoreUISafeInsets
end)
screenGui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling
screenGui.DisplayOrder = 5
screenGui.Parent = playerGui

local requestRemote, syncRemote, pushRemote = Net.waitForRemotes(ReplicatedStorage, 15)

local currentState: { [string]: any }? = nil
local currentSettings: SettingsModal.SettingsState = {
	theme = "Dark",
	reducedMotion = false,
	reducedFlash = false,
	skipBelowRarity = "None",
}
local autoEnabled = false
local fastEnabled = false
local rollBusy = false
local autoLoopToken = 0
local statusMessage = ""
local selectedWorldId = 1
local currentBannerName = "Standard Summon"
local currentWeatherText = "Clear Skies (1.0x)"

local hudController: Hud.HudController
local revealController: RevealModal.RevealModalController
local deckController: DeckModal.DeckModalController
local battleController: BattleModal.BattleModalController
local settingsController: SettingsModal.SettingsModalController
local endgameController: any
local metaController: any

local function playSfx(soundId: string?)
	if not soundId or soundId == "" then
		return
	end
	pcall(function()
		local s = Instance.new("Sound")
		s.SoundId = soundId
		s.Volume = 0.35
		s.Parent = SoundService
		s:Play()
		task.delay(2, function()
			s:Destroy()
		end)
	end)
end

local function sendRequest(payload: Net.RequestPayload): Net.ResponsePayload
	if not requestRemote then
		return { ok = false, error = "Connecting to server..." }
	end
	local ok, result = pcall(function()
		return requestRemote:InvokeServer(payload)
	end)
	if not ok then
		return { ok = false, error = tostring(result) }
	end
	if type(result) ~= "table" then
		return { ok = false, error = "Invalid server response" }
	end
	return result :: Net.ResponsePayload
end

local function extractStringList(raw: any): { string }
	local out: { string } = {}
	if type(raw) == "table" then
		for _, uid in ipairs(raw :: { any }) do
			if type(uid) == "string" then
				table.insert(out, uid)
			end
		end
	end
	return out
end

local function extractActiveDeckUids(state: { [string]: any }): { string }
	return extractStringList(state.activeDeckUids or state.activeDeck or state.deckIds or state.deck)
end

local function extractDefenseDeckUids(state: { [string]: any }): { string }
	return extractStringList(state.defenseDeckUids or state.defenseDeck)
end

local function extractOwnedCards(state: { [string]: any }): { DeckModal.OwnedCard }
	local raw = state.cards or state.inventory or state.collection
	local out: { DeckModal.OwnedCard } = {}
	if type(raw) == "table" then
		for _, item in pairs(raw :: { [any]: any }) do
			if type(item) == "table" then
				local uid = tostring(item.uid or item.instanceId or item.id or "")
				if uid ~= "" then
					local atk = tonumber(item.atk) or 10
					local hp = tonumber(item.hp) or 100
					local spd = tonumber(item.spd) or 10
					local power = tonumber(item.power) or math.floor(hp * 0.25 + atk * 3 + spd)
					local resolvedId = tostring(item.id or item.cardId or uid)
					local resolvedName = tostring(item.displayName or item.name or item.cardId or "Unknown")
					local resolvedDupes = tonumber(item.dupesAvailable)
						or tonumber(item.dupeCount)
						or tonumber(item.dupes)
						or 0
					table.insert(out, {
						uid = uid,
						id = resolvedId,
						cardId = if item.cardId then tostring(item.cardId) else resolvedId,
						name = resolvedName,
						displayName = resolvedName,
						rarity = tostring(item.rarity or "Common"),
						mutation = if item.mutation then tostring(item.mutation) else nil,
						role = if item.role then tostring(item.role) else nil,
						faction = if item.faction then tostring(item.faction) else nil,
						showName = if item.showName then tostring(item.showName) else nil,
						artKey = if item.artKey then tostring(item.artKey) else ("cards/" .. resolvedId),
						level = tonumber(item.level) or 1,
						stars = tonumber(item.stars) or tonumber(item.ascension) or 0,
						hp = hp,
						atk = atk,
						spd = spd,
						power = power,
						locked = item.locked == true,
						dupesAvailable = resolvedDupes,
						dupeCount = resolvedDupes,
						levelUpCostYen = tonumber(item.levelUpCostYen) or tonumber(item.levelUpCost) or 100,
						sellValueYen = tonumber(item.sellValueYen) or tonumber(item.sellValue) or 25,
					})
				end
			end
		end
	end
	return out
end

local function computeDeckPower(cards: { DeckModal.OwnedCard }, activeUids: { string }): number
	local activeSet: { [string]: boolean } = {}
	for _, uid in ipairs(activeUids) do
		activeSet[uid] = true
	end
	local total = 0
	for _, card in ipairs(cards) do
		if activeSet[card.uid] then
			total += card.power
		end
	end
	return total
end

local function buildDeckSnapshot(state: { [string]: any }): DeckModal.DeckSnapshot
	local cards = extractOwnedCards(state)
	local activeUids = extractActiveDeckUids(state)
	local defenseUids = extractDefenseDeckUids(state)
	local totalPower = tonumber(state.deckPower) or computeDeckPower(cards, activeUids)
	local synergy = if type(state.synergySummary) == "string" and state.synergySummary ~= ""
		then state.synergySummary
		else "Build your squad for faction bonuses"
	return {
		cards = cards,
		activeDeckUids = activeUids,
		defenseDeckUids = defenseUids,
		maxDeckSize = tonumber(state.maxDeckSize) or 5,
		synergySummary = synergy,
		bondsSummary = if type(state.bondsSummary) == "string" then state.bondsSummary else nil,
		totalPower = totalPower,
		yen = tonumber(state.yen) or 0,
	}
end

local function buildStagesForState(state: { [string]: any }, worldFilter: number?): { BattleModal.BattleStageInfo }
	local clearedMap: { [string]: boolean } = {}
	if type(state.clearedStages) == "table" then
		for k, v in pairs(state.clearedStages :: { [any]: any }) do
			if type(k) == "string" and v == true then
				clearedMap[k] = true
			elseif type(v) == "string" then
				clearedMap[v] = true
			end
		end
	end
	local stages: { BattleModal.BattleStageInfo } = {}
	local worldIds: { number } = if worldFilter then { worldFilter } else { 1, 2, 3, 4, 5 }
	local unlockedWorld = tonumber(state.unlockedWorld) or 1
	for _, wId in ipairs(worldIds) do
		local worldDef = Worlds.getWorld(wId)
		if worldDef and type(worldDef.stages) == "table" then
			local highestCleared = 0
			for idx, st in ipairs(worldDef.stages) do
				local stId = tostring(st.id or ("w" .. tostring(wId) .. "_act" .. tostring(idx)))
				local cleared = clearedMap[stId] == true
				if cleared then
					highestCleared = idx
				end
				local subtitle = string.format(
					"World %d • %s",
					wId,
					if st.isBoss then "World Boss" else ("Act " .. tostring(st.act or idx))
				)
				table.insert(stages, {
					id = stId,
					name = tostring(st.name or stId),
					subtitle = subtitle,
					recommendedPower = tonumber(st.recommendedPower) or (120 * wId),
					rewardSummary = string.format(
						"+%d Yen  +%d Gems",
						tonumber(st.yenReward) or 0,
						tonumber(st.gemsReward) or 0
					),
					isBoss = st.isBoss == true,
					unlocked = (wId <= unlockedWorld) and (idx <= highestCleared + 1),
				})
			end
		end
	end
	if #stages == 0 then
		local wId = worldFilter or 1
		for act = 1, 5 do
			local stId = string.format("w%d_act%d", wId, act)
			table.insert(stages, {
				stageId = stId,
				worldId = wId,
				actNumber = act,
				name = string.format("World %d - Act %d", wId, act),
				recommendedPower = 120 * wId * act,
				yenReward = 80 * wId * act,
				gemsReward = 5 * act,
				unlocked = act == 1 or clearedMap[string.format("w%d_act%d", wId, act - 1)] == true,
				cleared = clearedMap[stId] == true,
				isBoss = act == 5,
			})
		end
	end
	return stages
end

local function buildHudState(state: { [string]: any }): Hud.HudState
	local activeUids = extractActiveDeckUids(state)
	local pity = if type(state.pity) == "table" then state.pity else {}
	local luckText = if type(state.luckSummary) == "string" and state.luckSummary ~= ""
		then state.luckSummary
		else "Base 1.0x"
	return {
		yen = tonumber(state.yen) or 0,
		gems = tonumber(state.gems) or 0,
		tickets = tonumber(state.tickets) or 0,
		luckMultiplier = tonumber(state.luckMultiplier) or 1,
		luckBreakdown = luckText,
		luckSummary = luckText,
		legendaryPity = tonumber(state.legendaryPity or pity.legendary) or 0,
		legendaryPityMax = tonumber(state.legendaryPityMax or pity.legendaryMax) or 50,
		mythicPity = tonumber(state.mythicPity or pity.mythic) or 0,
		mythicPityMax = tonumber(state.mythicPityMax or pity.mythicMax) or 250,
		rollCostYen = tonumber(state.rollCostYen) or 0,
		autoEnabled = autoEnabled,
		fastEnabled = fastEnabled,
		nextGoal = tostring(state.nextGoal or "Summon cards and clear World 1 Act 1!"),
		deckCount = #activeUids,
		deckMax = tonumber(state.maxDeckSize) or 5,
		rollStatusText = statusMessage,
		selectedWorld = selectedWorldId,
		bannerName = currentBannerName,
		weatherText = currentWeatherText,
	}
end

local function applyWeatherInfo(wData: any)
	if type(wData) ~= "table" then
		return
	end
	local name = tostring(wData.name or wData.weather or "Clear Skies")
	local mult = tonumber(wData.luckMult or wData.multiplier) or 1
	currentWeatherText = string.format("%s (%.2gx)", name, mult)
	revealController.setContext(currentBannerName, currentWeatherText)
	pcall(function()
		if mult > 1.2 then
			Lighting.OutdoorAmbient = Color3.fromRGB(145, 160, 195)
		else
			Lighting.OutdoorAmbient = Color3.fromRGB(130, 136, 150)
		end
	end)
	if currentState then
		hudController.update(buildHudState(currentState))
	end
end

local function applyState(newState: { [string]: any })
	currentState = newState
	if type(newState.selectedBanner) == "string" and newState.selectedBanner ~= "" then
		currentBannerName = newState.selectedBanner
	end
	if type(newState.weather) == "table" then
		applyWeatherInfo(newState.weather)
	end
	if type(newState.settings) == "table" then
		local s = newState.settings
		local rawTheme = if type(s.theme) == "string" then string.lower(s.theme) else ""
		local themeMode: Theme.ThemeMode = if rawTheme == "light" or s.darkMode == false then "Light" else "Dark"
		currentSettings = {
			theme = themeMode,
			reducedMotion = s.reducedMotion == true,
			reducedFlash = s.reducedFlash == true,
			skipBelowRarity = if type(s.skipBelowRarity) == "string" and s.skipBelowRarity ~= ""
				then s.skipBelowRarity
				else "None",
		}
		Theme.setMode(currentSettings.theme)
		settingsController.setState(currentSettings)
	end
	revealController.setContext(currentBannerName, currentWeatherText)
	hudController.update(buildHudState(newState))
	if deckController.isOpen() then
		deckController.refresh(buildDeckSnapshot(newState))
	end
	if endgameController and endgameController.updateState then
		endgameController.updateState(newState)
	end
	if metaController and metaController.updateState then
		metaController.updateState(newState)
	end
end

local function setStatus(msg: string)
	statusMessage = msg
	if currentState then
		hudController.update(buildHudState(currentState))
	end
end

local function performRoll(): boolean
	if rollBusy then
		return false
	end
	rollBusy = true
	local res = sendRequest({
		action = "Roll",
		bannerId = "world_" .. tostring(selectedWorldId),
		fastRoll = fastEnabled,
	})
	rollBusy = false
	if res.ok and type(res.data) == "table" then
		statusMessage = ""
		if res.data.state then
			applyState(res.data.state)
		end
		local results: { RevealModal.RollCardResult }? = nil
		if type(res.data.results) == "table" then
			results = res.data.results
		elseif type(res.data.roll) == "table" then
			results = { res.data.roll }
		elseif type(res.data.card) == "table" then
			results = { res.data.card }
		end
		if results and #results > 0 then
			revealController.play(results, autoEnabled or fastEnabled)
		end
		return true
	else
		setStatus(res.error or "Roll failed")
		return false
	end
end

settingsController = SettingsModal.create(
	screenGui,
	currentSettings,
	function(nextSettings: SettingsModal.SettingsState)
		currentSettings = nextSettings
		Theme.setMode(nextSettings.theme)
		if endgameController and endgameController.applyTheme then
			endgameController.applyTheme()
		end
		if metaController and metaController.applyTheme then
			metaController.applyTheme()
		end
		sendRequest({ action = "UpdateSettings", settings = nextSettings :: any })
		if currentState then
			hudController.update(buildHudState(currentState))
		end
	end
)

revealController = RevealModal.create(screenGui, function()
	return currentSettings
end)

local modalApi = {
	request = sendRequest,
	sendRequest = sendRequest,
	setStatus = setStatus,
	applyState = applyState,
	getState = function()
		return currentState
	end,
	playSfx = playSfx,
}

endgameController = EndgameModals.build(screenGui, modalApi, Theme)
metaController = MetaModals.build(screenGui, modalApi, Theme)

deckController = DeckModal.create(screenGui, {
	onEquipToggle = function(uid: string, shouldEquip: boolean)
		local currentIds = if currentState then extractActiveDeckUids(currentState) else {}
		local maxDeck = if currentState then (tonumber(currentState.maxDeckSize) or 5) else 5
		local nextIds: { string } = {}
		for _, id in ipairs(currentIds) do
			if id ~= uid then
				table.insert(nextIds, id)
			end
		end
		if shouldEquip then
			if #nextIds >= maxDeck then
				setStatus("Active deck is full")
				return
			end
			table.insert(nextIds, uid)
		end
		local res = sendRequest({ action = "SetDeck", deckIds = nextIds })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Unable to update deck")
		end
	end,
	onDefenseEquipToggle = function(uid: string, shouldEquip: boolean)
		local currentIds = if currentState then extractDefenseDeckUids(currentState) else {}
		local maxDeck = if currentState then (tonumber(currentState.maxDeckSize) or 5) else 5
		local nextIds: { string } = {}
		for _, id in ipairs(currentIds) do
			if id ~= uid then
				table.insert(nextIds, id)
			end
		end
		if shouldEquip then
			if #nextIds >= maxDeck then
				setStatus("Defense deck is full")
				return
			end
			table.insert(nextIds, uid)
		end
		local res = sendRequest({ action = "SaveDefenseDeck", defenseDeckIds = nextIds })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Unable to update defense deck")
		end
	end,
	onLevelUp = function(uid: string)
		local res = sendRequest({ action = "LevelUpCard", instanceId = uid })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Level up failed")
		end
	end,
	onAscend = function(uid: string)
		local res = sendRequest({ action = "AscendCard", instanceId = uid })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Ascend failed")
		end
	end,
	onToggleLock = function(uid: string)
		local res = sendRequest({ action = "ToggleLock", instanceId = uid })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Lock toggle failed")
		end
	end,
	onSell = function(uid: string)
		local res = sendRequest({ action = "SellCard", instanceId = uid })
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Sell failed")
		end
	end,
})

battleController = BattleModal.create(screenGui, {
	onStartBattle = function(stageId: string)
		local res = sendRequest({ action = "StartBattle", stageId = stageId })
		if res.ok and type(res.data) == "table" then
			if res.data.state then
				applyState(res.data.state)
			end
			battleController.playBattle(res.data.battle or res.data)
		else
			battleController.setStatus(res.error or "Battle failed")
		end
	end,
	getReducedMotion = function()
		return currentSettings.reducedMotion
	end,
})

local function closeAllModals()
	pcall(function()
		deckController.close()
	end)
	pcall(function()
		battleController.close()
	end)
	pcall(function()
		settingsController.close()
	end)
	pcall(function()
		endgameController.closeAll()
	end)
	pcall(function()
		metaController.closeAll()
	end)
	pcall(function()
		hudController.closeModesDrawer()
	end)
end

local function openBattleForWorld(worldId: number)
	closeAllModals()
	selectedWorldId = worldId
	if currentState then
		local snapshot = buildDeckSnapshot(currentState)
		battleController.open(buildStagesForState(currentState, worldId), snapshot.totalPower)
	end
end

hudController = Hud.create(screenGui, {
	onRoll = function()
		performRoll()
	end,
	onToggleAuto = function()
		autoEnabled = not autoEnabled
		autoLoopToken += 1
		local token = autoLoopToken
		if currentState then
			hudController.update(buildHudState(currentState))
		end
		if autoEnabled then
			task.spawn(function()
				while autoEnabled and token == autoLoopToken do
					local ok = performRoll()
					if not ok then
						autoEnabled = false
						if currentState then
							hudController.update(buildHudState(currentState))
						end
						break
					end
					task.wait(if fastEnabled then 0.35 else 1.2)
				end
			end)
		end
	end,
	onToggleFast = function()
		fastEnabled = not fastEnabled
		if currentState then
			hudController.update(buildHudState(currentState))
		end
	end,
	onOpenDeck = function()
		closeAllModals()
		if currentState then
			deckController.open(buildDeckSnapshot(currentState))
		end
	end,
	onOpenBattle = function()
		openBattleForWorld(selectedWorldId)
	end,
	onOpenSettings = function()
		closeAllModals()
		settingsController.open()
	end,
	onOpenShop = function()
		closeAllModals()
		metaController.openShop()
	end,
	onCycleWorld = function()
		selectedWorldId = (selectedWorldId % 5) + 1
		local bannerId = "banner_w" .. tostring(selectedWorldId)
		local bInfo = if type(Banners.get) == "function"
			then Banners.get(bannerId)
			elseif type(Banners.ById) == "table" then Banners.ById[bannerId]
			else nil
		currentBannerName = if type(bInfo) == "table" and bInfo.name
			then tostring(bInfo.name)
			else ("World " .. tostring(selectedWorldId) .. " Banner")
		sendRequest({ action = "SelectBanner", bannerId = bannerId })
		revealController.setContext(currentBannerName, currentWeatherText)
		if currentState then
			hudController.update(buildHudState(currentState))
		end
	end,
	onOpenTower = function()
		closeAllModals()
		endgameController.openTower()
	end,
	onOpenRaid = function()
		closeAllModals()
		endgameController.openRaid()
	end,
	onOpenWorldBoss = function()
		closeAllModals()
		endgameController.openWorldBoss()
	end,
	onOpenRanked = function()
		closeAllModals()
		endgameController.openRanked()
	end,
	onOpenRebirth = function()
		closeAllModals()
		endgameController.openRebirth()
	end,
	onOpenCharms = function()
		closeAllModals()
		endgameController.openCharms()
	end,
	onOpenQuests = function()
		closeAllModals()
		metaController.openQuests()
	end,
	onOpenIndex = function()
		closeAllModals()
		metaController.openIndex()
	end,
	onOpenLeaderboards = function()
		closeAllModals()
		metaController.openLeaderboards()
	end,
})

local function handlePropAction(prop: string)
	closeAllModals()
	if prop == "Roll" then
		performRoll()
	elseif prop == "Deck" and currentState then
		deckController.open(buildDeckSnapshot(currentState))
	elseif prop == "Battle" then
		openBattleForWorld(selectedWorldId)
	elseif prop == "Shop" then
		metaController.openShop()
	elseif prop == "Tower" then
		endgameController.openTower()
	elseif prop == "Raid" then
		endgameController.openRaid()
	elseif prop == "Ranked" then
		endgameController.openRanked()
	elseif prop == "Rebirth" then
		endgameController.openRebirth()
	elseif prop == "Charms" then
		endgameController.openCharms()
	elseif prop == "World2" then
		openBattleForWorld(2)
	elseif prop == "World3" then
		openBattleForWorld(3)
	elseif prop == "World4" then
		openBattleForWorld(4)
	elseif prop == "World5" then
		openBattleForWorld(5)
	end
end

ProximityPromptService.PromptTriggered:Connect(function(prompt: ProximityPrompt, player: Player)
	if player ~= localPlayer then
		return
	end
	local cur: Instance? = prompt
	while cur do
		local prop = cur:GetAttribute("OpenProp") or cur:GetAttribute("PropAction")
		if type(prop) == "string" and prop ~= "" then
			handlePropAction(prop)
			return
		end
		cur = cur.Parent
	end
end)

if syncRemote then
	syncRemote.OnClientEvent:Connect(function(payload: any)
		if type(payload) == "table" then
			applyState(payload)
		end
	end)
end

if pushRemote then
	pushRemote.OnClientEvent:Connect(function(eventData: any)
		if type(eventData) ~= "table" then
			return
		end
		if
			eventData.kind == "WeatherUpdate"
			or eventData.type == "WeatherUpdate"
			or eventData.event == "WeatherUpdate"
			or eventData.type == Net.PushEvents.WeatherUpdate
			or eventData.event == Net.PushEvents.WeatherUpdate
		then
			applyWeatherInfo(eventData.weather or eventData.data or eventData)
		elseif eventData.type == "OpenModal" then
			if type(eventData.modal) == "string" then
				handlePropAction(eventData.modal)
			end
		elseif eventData.type == "Toast" then
			setStatus(tostring(eventData.message or ""))
		end
	end)
end

task.spawn(function()
	local res = sendRequest({ action = "GetState" })
	if res.ok and type(res.data) == "table" then
		local state = res.data.state or res.data
		applyState(state)
	end
	local wRes = sendRequest({ action = "GetWeather" })
	if wRes.ok and type(wRes.data) == "table" then
		applyWeatherInfo(wRes.data)
	end
end)
