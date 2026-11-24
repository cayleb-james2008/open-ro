--!strict
local AnalyticsService = game:GetService("AnalyticsService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Net = require(Shared:WaitForChild("Net"))
local Config = Shared:WaitForChild("Config")
local Worlds = require(Config:WaitForChild("Worlds"))
local Banners = require(Config:WaitForChild("Banners"))

local ServerFolder = script.Parent
local DataService = require(ServerFolder:WaitForChild("DataService"))
local RollService = require(ServerFolder:WaitForChild("RollService"))
local DeckService = require(ServerFolder:WaitForChild("DeckService"))
local BattleService = require(ServerFolder:WaitForChild("BattleService"))
local WorldBuild = require(ServerFolder:WaitForChild("WorldBuild"))
local TowerService = require(ServerFolder:WaitForChild("TowerService"))
local RaidService = require(ServerFolder:WaitForChild("RaidService"))
local WorldBossService = require(ServerFolder:WaitForChild("WorldBossService"))
local RankedService = require(ServerFolder:WaitForChild("RankedService"))
local RebirthService = require(ServerFolder:WaitForChild("RebirthService"))
local CharmService = require(ServerFolder:WaitForChild("CharmService"))
local WeatherService = require(ServerFolder:WaitForChild("WeatherService"))
local RetentionService = require(ServerFolder:WaitForChild("RetentionService"))
local MonetizationService = require(ServerFolder:WaitForChild("MonetizationService"))

local requestRemote, syncRemote, pushRemote = Net.getOrCreateRemotes(ReplicatedStorage)

local function logFunnel(player: Player, stepName: string, stepIndex: number)
	pcall(function()
		(AnalyticsService :: any):LogOnboardingFunnelStepEvent(player, stepIndex, stepName)
	end)
end

local function logEconomy(
	player: Player,
	flowType: string,
	currency: string,
	amount: number,
	balance: number,
	sku: string
)
	pcall(function()
		local ft = if flowType == "Source"
			then Enum.AnalyticsEconomyFlowType.Source
			else Enum.AnalyticsEconomyFlowType.Sink
		local analytics: any = AnalyticsService
		analytics:LogEconomyEvent(player, ft, currency, math.floor(amount), math.floor(balance), "Gameplay", sku)
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
	local pAny = profile :: any
	local friendCount = math.max(0, #Players:GetPlayers() - 1)

	if action == "GetState" then
		return okWithState(player, profile)
	elseif action == "SelectBanner" then
		local bannerId = tostring(payload.bannerId or "")
		local banner = (Banners :: any).get and (Banners :: any).get(bannerId) or (Banners :: any)[bannerId]
		local worldId = if type(payload.worldId) == "number"
			then math.floor(payload.worldId)
			else (if banner and banner.worldId then banner.worldId else pAny.selectedWorld or profile.currentWorld)
		if not profile.unlockedWorlds[worldId] then
			return fail("World not unlocked yet")
		end
		pAny.selectedWorld = worldId
		profile.currentWorld = worldId
		if bannerId ~= "" then
			pAny.selectedBannerId = bannerId
		end
		return okWithState(player, profile)
	elseif action == "Roll" then
		local worldId = if type(payload.worldId) == "number"
			then payload.worldId
			else (pAny.selectedWorld or profile.currentWorld)
		local ok, err, result = (RollService :: any).rollSingle(
			profile,
			worldId,
			payload.useTicket == true,
			friendCount,
			player
		)
		if not ok or not result then
			return fail(err or "Roll failed")
		end
		RetentionService.trackProgress(pAny, "Roll", 1)
		if profile.rollCount == 1 then
			logFunnel(player, "FirstRoll", 2)
		end
		return okWithState(player, profile, { result = result, results = { result } })
	elseif action == "RollBatch" then
		local worldId = if type(payload.worldId) == "number"
			then payload.worldId
			else (pAny.selectedWorld or profile.currentWorld)
		local count = if type(payload.count) == "number" then payload.count else 5
		local ok, err, results = (RollService :: any).rollBatch(
			profile,
			worldId,
			count,
			payload.useTicket == true,
			friendCount,
			player
		)
		if not ok or not results then
			return fail(err or "Batch roll failed")
		end
		RetentionService.trackProgress(pAny, "Roll", #results)
		return okWithState(player, profile, { result = results[1], results = results })
	elseif action == "SetDeck" then
		local ok, err = (DeckService :: any).setDeck(profile, payload.deckIds or {})
		if not ok then
			return fail(err or "Could not update deck")
		end
		return okWithState(player, profile)
	elseif action == "SaveDefenseDeck" then
		local ok, err = DeckService.saveDefenseDeck(profile, payload.defenseDeckIds or payload.deckIds or {})
		if not ok then
			return fail(err or "Could not update defense deck")
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
		RetentionService.trackProgress(pAny, "LevelUp", 1)
		return okWithState(player, profile)
	elseif action == "AscendCard" then
		local ok, err = DeckService.ascendCard(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Ascension failed")
		end
		return okWithState(player, profile)
	elseif action == "RerollTrait" or action == "TraitReroll" then
		local ok, err, newTrait = (RollService :: any).rerollTrait(profile, tostring(payload.instanceId or ""))
		if not ok then
			return fail(err or "Trait reroll failed")
		end
		return okWithState(player, profile, { trait = newTrait })
	elseif action == "StartBattle" then
		local stageId = if type(payload.stageId) == "string" and payload.stageId ~= ""
			then payload.stageId
			else "w1_act1"
		local ok, battleData, err = BattleService.startBattle(player, stageId)
		if not ok or not battleData then
			return fail(err or "Battle failed")
		end
		RetentionService.trackProgress(pAny, "Battle", 1)
		syncPlayer(player, profile)
		return { ok = true, data = battleData }
	elseif action == "UnlockWorld" then
		local worldId = math.max(1, math.floor(tonumber(payload.worldId) or 1))
		local worldDef = Worlds.getWorld(worldId)
		if not worldDef then
			return fail("Unknown world")
		end
		if profile.unlockedWorlds[worldId] then
			profile.currentWorld = worldId
			pAny.selectedWorld = worldId
			return okWithState(player, profile)
		end
		local unlockCost = (worldDef :: any).unlockCostYen or 0
		if profile.yen < unlockCost then
			return fail(string.format("Need %d Yen to unlock", unlockCost))
		end
		profile.yen -= unlockCost
		profile.unlockedWorlds[worldId] = true
		profile.currentWorld = worldId
		pAny.selectedWorld = worldId
		logEconomy(player, "Sink", "Yen", unlockCost, profile.yen, "UnlockWorld_" .. tostring(worldId))
		return okWithState(player, profile)
	elseif action == "ChallengeTower" then
		local ok, res, err = (TowerService :: any).challengeFloor(player, profile, payload.floor)
		if not ok then
			return fail(err or "Tower challenge failed")
		end
		RetentionService.trackProgress(pAny, "Tower", 1)
		return okWithState(player, profile, res)
	elseif action == "GetTowerStatus" then
		return { ok = true, data = TowerService.getStatus(profile) }
	elseif action == "StartRaid" then
		local ok, res, err = (RaidService :: any).startRaid(
			player,
			profile,
			tostring(payload.raidId or "raid_skull_island")
		)
		if not ok then
			return fail(err or "Raid failed")
		end
		RetentionService.trackProgress(pAny, "Raid", 1)
		return okWithState(player, profile, res)
	elseif action == "GetRaidStatus" then
		return { ok = true, data = (RaidService :: any).getStatus(profile) }
	elseif action == "ChallengeWorldBoss" then
		local ok, res, err = (WorldBossService :: any).challenge(player, profile, payload.bossId)
		if not ok then
			return fail(err or "World boss challenge failed")
		end
		return okWithState(player, profile, res)
	elseif action == "GetWorldBossStatus" then
		return { ok = true, data = (WorldBossService :: any).getStatus(profile) }
	elseif action == "PlayRanked" then
		local ok, res, err = (RankedService :: any).playMatch(player, profile, payload.targetUserId)
		if not ok then
			return fail(err or "Ranked match failed")
		end
		RetentionService.trackProgress(pAny, "Ranked", 1)
		return okWithState(player, profile, res)
	elseif action == "ClaimRankedReward" then
		local ok, res, err = (RankedService :: any).claimReward(profile)
		if not ok then
			return fail(err or "Could not claim ranked reward")
		end
		return okWithState(player, profile, res)
	elseif action == "GetRankedStatus" then
		return { ok = true, data = (RankedService :: any).getStatus(profile) }
	elseif action == "PerformRebirth" or action == "Rebirth" then
		local ok, res, err = (RebirthService :: any).performRebirth(profile)
		if not ok then
			return fail(err or "Rebirth failed")
		end
		return okWithState(player, profile, res)
	elseif action == "BuySkillNode" then
		local ok, res, err = (RebirthService :: any).buySkillNode(profile, tostring(payload.nodeId or ""))
		if not ok then
			return fail(err or "Could not buy skill node")
		end
		return okWithState(player, profile, res)
	elseif action == "GetRebirthStatus" then
		return { ok = true, data = (RebirthService :: any).getStatus(profile) }
	elseif action == "CraftCharm" then
		local ok, res, err = CharmService.craftCharm(profile, tostring(payload.charmId or ""))
		if not ok then
			return fail(err or "Could not craft charm")
		end
		return okWithState(player, profile, res)
	elseif action == "EquipCharm" then
		local ids = table.clone(pAny.equippedCharms or {})
		local cid = tostring(payload.charmId or "")
		if cid ~= "" and not table.find(ids, cid) then
			table.insert(ids, cid)
		end
		local ok, res, err = CharmService.equipCharms(profile, ids)
		if not ok then
			return fail(err or "Could not equip charm")
		end
		return okWithState(player, profile, res)
	elseif action == "UnequipCharm" then
		local ids = {}
		local cid = tostring(payload.charmId or "")
		for _, existing in ipairs(pAny.equippedCharms or {}) do
			if existing ~= cid then
				table.insert(ids, existing)
			end
		end
		local ok, res, err = CharmService.equipCharms(profile, ids)
		if not ok then
			return fail(err or "Could not unequip charm")
		end
		return okWithState(player, profile, res)
	elseif action == "GetCharmStatus" then
		return { ok = true, data = CharmService.getStatus(profile) }
	elseif action == "ClaimQuest" then
		local ok, res, err = (RetentionService :: any).claimQuest(pAny, tostring(payload.questId or ""))
		if not ok then
			return fail(err or "Could not claim quest")
		end
		return okWithState(player, profile, res)
	elseif action == "ClaimLoginStreak" or action == "ClaimDaily" then
		local ok, res, err = (RetentionService :: any).claimLoginStreak(pAny)
		if not ok then
			return fail(err or "Could not claim daily streak")
		end
		return okWithState(player, profile, res)
	elseif action == "ClaimPlaytime" then
		local ok, res, err = (RetentionService :: any).claimPlaytime(pAny, tostring(payload.milestoneId or ""))
		if not ok then
			return fail(err or "Could not claim playtime reward")
		end
		return okWithState(player, profile, res)
	elseif action == "RedeemCode" then
		local ok, res, err = (RetentionService :: any).redeemCode(pAny, tostring(payload.code or ""))
		if not ok then
			return fail(err or "Could not redeem code")
		end
		return okWithState(player, profile, res)
	elseif action == "GetRetentionStatus" then
		return { ok = true, data = (RetentionService :: any).getStatus(pAny) }
	elseif action == "GetLeaderboards" then
		return { ok = true, data = (RetentionService :: any).getLeaderboards(tostring(payload.boardType or "rolls")) }
	elseif action == "GetWeather" then
		return { ok = true, data = (WeatherService :: any).getCurrentWeather() }
	elseif action == "PromptPurchase" then
		local ok, err, res = MonetizationService.promptPurchase(player, pAny, tostring(payload.productId or ""))
		if not ok then
			return fail(err or "Purchase failed")
		end
		return okWithState(player, profile, res)
	elseif action == "GetShopStatus" then
		return { ok = true, data = MonetizationService.getShopStatus(player, pAny) }
	elseif action == "UsePotion" then
		local potionId = tostring(payload.potionId or "Minor")
		local qty = profile.potions[potionId] or 0
		if qty < 1 then
			return fail("No potion available")
		end
		profile.potions[potionId] = qty - 1
		profile.luckPotionExpiresAt = os.time() + 300
		profile.luckPotionBonus = 1.0
		return okWithState(player, profile)
	elseif action == "UpdateSettings" then
		local s = payload.settings
		if type(s) == "table" then
			if type(s.theme) == "string" then
				local themeLower = string.lower(s.theme)
				if themeLower == "light" then
					profile.settings.theme = "Light"
				elseif themeLower == "dark" then
					profile.settings.theme = "Dark"
				end
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

MonetizationService.init(function(player)
	return DataService.getProfile(player) :: any
end, function(player, profile)
	syncPlayer(player, profile :: any)
end)

if type((WeatherService :: any).start) == "function" then
	(WeatherService :: any).start(function(weather)
		pushRemote:FireAllClients({
			kind = Net.PushEvents.WeatherUpdate,
			weather = weather,
		})
	end)
end

local built = WorldBuild.build()
local promptList: { ProximityPrompt } = if built and built.prompts then built.prompts else {}
if #promptList == 0 and built and built.hub then
	for _, desc in ipairs(built.hub:GetDescendants()) do
		if desc:IsA("ProximityPrompt") then
			table.insert(promptList, desc)
		end
	end
end
for _, prompt in ipairs(promptList) do
	prompt.Triggered:Connect(function(player: Player)
		local propAction = prompt:GetAttribute("PropAction")
		if type(propAction) == "string" then
			pushRemote:FireClient(player, {
				kind = "OpenProp",
				type = "OpenModal",
				propAction = propAction,
				modal = propAction,
			})
		end
	end)
end

DataService.onProfileReady(function(player: Player, profile: DataService.PlayerProfile)
	logFunnel(player, "JoinedHub", 1)
	syncPlayer(player, profile)
end)
DataService.init()
