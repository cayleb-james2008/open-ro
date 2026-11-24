--!strict

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Net = require(Shared:WaitForChild("Net"))
local Config = Shared:WaitForChild("Config")
local Worlds = require(Config:WaitForChild("Worlds"))

local ClientFolder = script.Parent
local Theme = require(ClientFolder:WaitForChild("Theme"))
local Hud = require(ClientFolder:WaitForChild("Hud"))
local RevealModal = require(ClientFolder:WaitForChild("RevealModal"))
local DeckModal = require(ClientFolder:WaitForChild("DeckModal"))
local BattleModal = require(ClientFolder:WaitForChild("BattleModal"))
local SettingsModal = require(ClientFolder:WaitForChild("SettingsModal"))

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

local hudController: Hud.HudController
local revealController: RevealModal.RevealModalController
local deckController: DeckModal.DeckModalController
local battleController: BattleModal.BattleModalController
local settingsController: SettingsModal.SettingsModalController

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

local function extractActiveDeckUids(state: { [string]: any }): { string }
	local raw = state.activeDeckUids or state.activeDeck or state.deckIds or state.deck
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
					table.insert(out, {
						uid = uid,
						id = tostring(item.id or item.cardId or uid),
						name = tostring(item.name or item.cardId or "Unknown"),
						rarity = tostring(item.rarity or "Common"),
						mutation = if item.mutation then tostring(item.mutation) else nil,
						role = if item.role then tostring(item.role) else nil,
						faction = if item.faction then tostring(item.faction) else nil,
						level = tonumber(item.level) or 1,
						stars = tonumber(item.stars) or tonumber(item.ascension) or 0,
						hp = hp,
						atk = atk,
						spd = spd,
						power = power,
						locked = item.locked == true,
						dupesAvailable = tonumber(item.dupesAvailable) or tonumber(item.dupes) or 0,
						levelUpCostYen = tonumber(item.levelUpCostYen) or 100,
						sellValueYen = tonumber(item.sellValueYen) or 25,
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
	local totalPower = tonumber(state.deckPower) or computeDeckPower(cards, activeUids)
	local synergy = if type(state.synergySummary) == "string" and state.synergySummary ~= ""
		then state.synergySummary
		else "Build your squad for faction bonuses"
	return {
		cards = cards,
		activeDeckUids = activeUids,
		maxDeckSize = tonumber(state.maxDeckSize) or 5,
		synergySummary = synergy,
		totalPower = totalPower,
		yen = tonumber(state.yen) or 0,
	}
end

local function findWorldDef(worldId: number): any
	local worldsAny = Worlds :: any
	if type(worldsAny.get) == "function" then
		return worldsAny.get(worldId) or worldsAny.get(1)
	end
	if type(Worlds.List) == "table" then
		for _, w in ipairs(Worlds.List) do
			if w.id == worldId then
				return w
			end
		end
		return Worlds.List[1]
	end
	return nil
end

local function buildStagesForState(state: { [string]: any }): { BattleModal.BattleStageInfo }
	local stages: { BattleModal.BattleStageInfo } = {}
	local unlockedWorld = tonumber(state.unlockedWorld) or 1
	local clearedStages = if type(state.clearedStages) == "table" then state.clearedStages else {}
	local worldDef = findWorldDef(unlockedWorld)
	if worldDef and type(worldDef.stages) == "table" then
		for index, stageDef in ipairs(worldDef.stages) do
			local prevCleared = index == 1 or clearedStages[worldDef.stages[index - 1].id] == true
			local rewards = if type(stageDef.rewards) == "table" then stageDef.rewards else {}
			local yenReward = tonumber(stageDef.yenReward or rewards.yen) or 0
			local gemReward = tonumber(stageDef.gemReward or rewards.gems) or 0
			local rewardText = string.format("%s Yen • %s Gems", tostring(yenReward), tostring(gemReward))
			local subtitleText = stageDef.subtitle or (if stageDef.isBoss then "World Boss" else "Battle Stage")
			table.insert(stages, {
				id = stageDef.id,
				name = stageDef.name,
				subtitle = subtitleText,
				recommendedPower = stageDef.recommendedPower or 100,
				rewardSummary = rewardText,
				isBoss = stageDef.isBoss == true,
				unlocked = prevCleared,
			})
		end
	end
	return stages
end

local function formatLuckFactor(value: number): string
	if math.abs(value * 10 - math.round(value * 10)) < 0.001 then
		return string.format("%.1fx", value)
	end
	return string.format("%.2fx", value)
end

local function formatLuckBreakdown(raw: any): string
	if type(raw) == "string" and raw ~= "" then
		return raw
	end
	if type(raw) ~= "table" then
		return "Base 1.0x"
	end
	local tbl = raw :: { [any]: any }
	if #tbl > 0 then
		local parts: { string } = {}
		for _, entry in ipairs(tbl) do
			if type(entry) == "string" and entry ~= "" then
				table.insert(parts, entry)
			elseif type(entry) == "table" then
				local label = tostring(entry.label or entry.name or entry.source or "Bonus")
				local val = tonumber(entry.value or entry.multiplier or entry.bonus)
				if val then
					table.insert(parts, string.format("%s %s", label, formatLuckFactor(val)))
				else
					table.insert(parts, label)
				end
			end
		end
		if #parts > 0 then
			return table.concat(parts, " + ")
		end
	end
	local keys: { string } = {}
	for k, _ in pairs(tbl) do
		if type(k) == "string" and k ~= "total" then
			table.insert(keys, k)
		end
	end
	table.sort(keys)
	local parts: { string } = {}
	for _, k in ipairs(keys) do
		local v = tbl[k]
		local label = string.upper(string.sub(k, 1, 1)) .. string.sub(k, 2)
		if type(v) == "number" then
			if v ~= 0 then
				table.insert(parts, string.format("%s: %s", label, formatLuckFactor(v)))
			end
		elseif type(v) == "string" and v ~= "" then
			table.insert(parts, string.format("%s: %s", label, v))
		elseif type(v) == "boolean" and v then
			table.insert(parts, label)
		end
	end
	if #parts == 0 then
		return "Base 1.0x"
	end
	return table.concat(parts, " • ")
end

local function buildHudState(state: { [string]: any }): Hud.HudState
	local activeUids = extractActiveDeckUids(state)
	local pity = if type(state.pity) == "table" then state.pity else {}
	return {
		yen = tonumber(state.yen) or 0,
		gems = tonumber(state.gems) or 0,
		tickets = tonumber(state.tickets) or 0,
		luckMultiplier = tonumber(state.luckMultiplier) or 1,
		luckBreakdown = formatLuckBreakdown(state.luckBreakdown),
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
	}
end

local function applyState(newState: { [string]: any })
	currentState = newState
	if type(newState.settings) == "table" then
		local s = newState.settings
		local themeMode: Theme.ThemeMode = if s.theme == "Light" or s.darkMode == false then "Light" else "Dark"
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
	hudController.update(buildHudState(newState))
	if deckController.isOpen() then
		deckController.refresh(buildDeckSnapshot(newState))
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
		sendRequest({
			action = "UpdateSettings",
			settings = nextSettings :: any,
		})
		if currentState then
			hudController.update(buildHudState(currentState))
		end
	end
)

revealController = RevealModal.create(screenGui, function()
	return currentSettings
end)

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
		local res = sendRequest({
			action = "SetDeck",
			deckIds = nextIds,
		})
		if res.ok and type(res.data) == "table" and res.data.state then
			applyState(res.data.state)
		elseif not res.ok then
			setStatus(res.error or "Unable to update deck")
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
			local battleResult = res.data.battle or res.data.result
			if type(battleResult) == "table" then
				battleController.playBattle(battleResult :: BattleModal.BattleResult)
			end
		else
			battleController.setStatus(res.error or "Battle could not start")
		end
	end,
	getReducedMotion = function(): boolean
		return currentSettings.reducedMotion
	end,
})

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
				while autoEnabled and autoLoopToken == token do
					local ok = performRoll()
					if not ok then
						autoEnabled = false
						if currentState then
							hudController.update(buildHudState(currentState))
						end
						break
					end
					task.wait(if fastEnabled then 0.35 else 1.25)
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
		if currentState then
			deckController.open(buildDeckSnapshot(currentState))
		end
	end,
	onOpenBattle = function()
		if currentState then
			local snapshot = buildDeckSnapshot(currentState)
			battleController.open(buildStagesForState(currentState), snapshot.totalPower)
		end
	end,
	onOpenSettings = function()
		settingsController.open()
	end,
})

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
		if eventData.type == "OpenModal" then
			if eventData.modal == "Deck" and currentState then
				deckController.open(buildDeckSnapshot(currentState))
			elseif eventData.modal == "Battle" and currentState then
				local snapshot = buildDeckSnapshot(currentState)
				battleController.open(buildStagesForState(currentState), snapshot.totalPower)
			elseif eventData.modal == "Settings" then
				settingsController.open()
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
end)
