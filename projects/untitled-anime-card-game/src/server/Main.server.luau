--!strict
local AnalyticsService = game:GetService("AnalyticsService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Net = require(Shared:WaitForChild("Net"))
local Config = Shared:WaitForChild("Config")
local Economy = require(Config:WaitForChild("Economy"))
local Worlds = require(Config:WaitForChild("Worlds"))

local ServerFolder = script.Parent
local DataService = require(ServerFolder:WaitForChild("DataService"))
local RollService = require(ServerFolder:WaitForChild("RollService"))
local DeckService = require(ServerFolder:WaitForChild("DeckService"))
local BattleService = require(ServerFolder:WaitForChild("BattleService"))
local WorldBuild = require(ServerFolder:WaitForChild("WorldBuild"))

local requestRemote, syncRemote, pushRemote = Net.getOrCreateRemotes(ReplicatedStorage)

local PROMO_CODES: { [string]: { yen: number, gems: number, tickets: number } } = {
	RELEASE = { yen = 500, gems = 100, tickets = 2 },
	NEXUS = { yen = 300, gems = 75, tickets = 1 },
	WELCOME = { yen = 250, gems = 50, tickets = 1 },
}

local function logFunnel(player: Player, stepName: string, stepIndex: number)
	pcall(function()
		(AnalyticsService :: any):LogOnboardingFunnelStepEvent(player, stepIndex, stepName)
	end)
end

local function syncPlayer(player: Player, profile: DataService.PlayerProfile)
	local state = DataService.serializeForClient(profile)
	syncRemote:FireClient(player, state)
end

local function okWithState(
	player: Player,
	profile: DataService.PlayerProfile,
	extra: { [string]: any }?
): Net.ResponsePayload
	local state = DataService.serializeForClient(profile)
	syncRemote:FireClient(player, state)
	local data: { [string]: any } = { state = state }
	if extra then
		for k, v in pairs(extra) do
			data[k] = v
		end
	end
	return { ok = true, data = data }
end

local function fail(msg: string): Net.ResponsePayload
	return { ok = false, error = msg }
end

local function handleRequest(player: Player, rawPayload: any): Net.ResponsePayload
	if type(rawPayload) ~= "table" then
		return fail("Invalid request")
	end
	local payload = rawPayload :: Net.RequestPayload
	local action = payload.action
	if type(action) ~= "string" then
		return fail("Missing action")
	end

	local profile = DataService.getOrLoadProfile(player)
	local friendCount = math.max(0, #Players:GetPlayers() - 1)

	if action == "GetState" then
		return okWithState(player, profile)
	elseif action == "Roll" then
		local worldId = if type(payload.worldId) == "number" then payload.worldId else profile.currentWorld
		local ok, err, result = RollService.rollSingle(profile, worldId, payload.useTicket == true, friendCount)
		if not ok or not result then
			return fail(err or "Roll failed")
		end
		if profile.rollCount == 1 then
			logFunnel(player, "FirstRoll", 2)
		end
		return okWithState(player, profile, {
			result = result,
			results = { result },
		})
	elseif action == "RollBatch" then
		local worldId = if type(payload.worldId) == "number" then payload.worldId else profile.currentWorld
		local count = if type(payload.count) == "number" then payload.count else 5
		local ok, err, results = RollService.rollBatch(profile, worldId, count, payload.useTicket == true, friendCount)
		if not ok or not results then
			return fail(err or "Batch roll failed")
		end
		return okWithState(player, profile, {
			result = results[1],
			results = results,
		})
	elseif action == "SetDeck" then
		local ok, err = DeckService.setDeck(profile, payload.deckIds, payload.defenseDeckIds)
		if not ok then
			return fail(err or "Could not update deck")
		end
		return okWithState(player, profile)
	elseif action == "ToggleLock" then
		local ok, err = DeckService.toggleLock(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Could not toggle lock")
		end
		return okWithState(player, profile)
	elseif action == "SellCard" then
		local ok, err, yenGained = DeckService.sellCard(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Could not sell card")
		end
		return okWithState(player, profile, { yenGained = yenGained })
	elseif action == "QuickSell" then
		local ok, err, yenGained = DeckService.quickSell(profile, payload.instanceIds or {})
		if not ok then
			return fail(err or "Quick sell failed")
		end
		return okWithState(player, profile, { yenGained = yenGained })
	elseif action == "LevelUpCard" then
		local ok, err = DeckService.levelUpCard(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Level up failed")
		end
		return okWithState(player, profile)
	elseif action == "AscendCard" then
		local ok, err = DeckService.ascendCard(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Ascension failed")
		end
		return okWithState(player, profile)
	elseif action == "RerollTrait" then
		local ok, err = DeckService.rerollTrait(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Trait reroll failed")
		end
		return okWithState(player, profile)
	elseif action == "StartBattle" then
		local stageId = if type(payload.stageId) == "string" and payload.stageId ~= "" then payload.stageId else "w1_s1"
		local ok, battleData, err = BattleService.startBattle(player, stageId)
		if not ok or not battleData then
			return fail(err or "Battle failed")
		end
		syncPlayer(player, profile)
		return { ok = true, data = battleData }
	elseif action == "UnlockWorld" then
		local worldId = math.max(1, math.floor(tonumber(payload.worldId) or 1))
		local worldDef = Worlds.get(worldId)
		if not worldDef then
			return fail("Unknown world")
		end
		if profile.unlockedWorlds[worldId] then
			profile.currentWorld = worldId
			return okWithState(player, profile)
		end
		local unlockCost = (worldDef :: any).unlockCostYen or 0
		if profile.yen < unlockCost then
			return fail(string.format("Need %d Yen to unlock", unlockCost))
		end
		profile.yen -= unlockCost
		profile.unlockedWorlds[worldId] = true
		profile.currentWorld = worldId
		return okWithState(player, profile)
	elseif action == "RedeemCode" then
		local code = string.upper(tostring(payload.code or ""))
		local reward = PROMO_CODES[code]
		if not reward then
			return fail("Invalid code")
		end
		if profile.redeemedCodes[code] then
			return fail("Code already redeemed")
		end
		profile.redeemedCodes[code] = true
		profile.yen += reward.yen
		profile.gems += reward.gems
		profile.tickets += reward.tickets
		return okWithState(player, profile, { reward = reward })
	elseif action == "UsePotion" then
		local potionId = tostring(payload.potionId or "luck_potion_1")
		local qty = profile.potions[potionId] or 0
		if qty < 1 then
			return fail("No potion available")
		end
		profile.potions[potionId] = qty - 1
		profile.luckPotionExpiresAt = os.time() + 300
		profile.luckPotionBonus = 1.0
		return okWithState(player, profile)
	elseif action == "ClaimDaily" then
		local now = os.time()
		if now - (profile.lastDailyClaim or 0) < 20 * 3600 then
			return fail("Daily reward already claimed")
		end
		profile.lastDailyClaim = now
		profile.dailyStreak = (profile.dailyStreak or 0) + 1
		local yenReward = 250 + math.min(6, profile.dailyStreak) * 75
		local gemReward = 50 + math.min(6, profile.dailyStreak) * 15
		profile.yen += yenReward
		profile.gems += gemReward
		profile.tickets += 1
		return okWithState(player, profile, { yen = yenReward, gems = gemReward, tickets = 1 })
	elseif action == "Rebirth" then
		local eco = Economy :: any
		local reqYen = if type(eco.RebirthBaseYen) == "number" then eco.RebirthBaseYen else 10000
		if profile.yen < reqYen then
			return fail(string.format("Need %d Yen to Rebirth", reqYen))
		end
		profile.yen = 500
		profile.rebirths = (profile.rebirths or 0) + 1
		profile.tickets += 3
		return okWithState(player, profile)
	elseif action == "UpdateSettings" then
		local s = payload.settings
		if type(s) == "table" then
			if s.theme == "light" or s.theme == "Light" then
				profile.settings.theme = "light"
			elseif s.theme == "dark" or s.theme == "Dark" then
				profile.settings.theme = "dark"
			end
			if type(s.reducedMotion) == "boolean" then
				profile.settings.reducedMotion = s.reducedMotion
			end
			if type(s.reducedFlash) == "boolean" then
				profile.settings.reducedFlash = s.reducedFlash
			end
			if type(s.skipBelowRarity) == "string" then
				profile.settings.skipBelowRarity = s.skipBelowRarity
			end
		end
		return okWithState(player, profile)
	end

	return fail("Unsupported action")
end

requestRemote.OnServerInvoke = handleRequest

local hub = WorldBuild.build()
if hub then
	for _, desc in ipairs(hub:GetDescendants()) do
		if desc:IsA("ProximityPrompt") then
			desc.Triggered:Connect(function(player: Player)
				local propAction = desc:GetAttribute("PropAction")
				if type(propAction) == "string" then
					pushRemote:FireClient(player, {
						kind = "OpenProp",
						propAction = propAction,
					})
				end
			end)
		end
	end
end

DataService.start(function(player: Player, profile: DataService.PlayerProfile)
	logFunnel(player, "JoinedHub", 1)
	syncPlayer(player, profile)
end)
