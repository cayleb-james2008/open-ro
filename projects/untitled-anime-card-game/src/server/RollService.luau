--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = require(Shared:WaitForChild("Config"))
local ConfigFolder = Shared:WaitForChild("Config")
local Economy = require(ConfigFolder:WaitForChild("Economy"))
local Worlds = require(ConfigFolder:WaitForChild("Worlds"))
local LuckMath = require(Shared:WaitForChild("LuckMath"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local RollService = {}

local function countKeys(tbl: { [any]: any }?): number
	if type(tbl) ~= "table" then
		return 0
	end
	local n = 0
	for _ in pairs(tbl) do
		n += 1
	end
	return n
end

function RollService.getLuckInputs(
	profile: DataService.PlayerProfile,
	_friendCount: number?,
	player: Player?
): LuckMath.LuckInputs
	local now = os.time()
	local potionBonus = 0
	if (profile.luckPotionExpiresAt or 0) > now then
		potionBonus = profile.luckPotionBonus or 0
	end

	local totalCards = math.max(1, #Config.Cards.List)
	local indexBonus = math.min(0.5, (countKeys(profile.discoveredCards) / totalCards) * 0.5)
	local rebirthBonus = (profile.rebirths or 0) * 0.1

	local charmBonus = 0
	local charmsCfg = (Config :: any).Charms
	if charmsCfg and type(profile.equippedCharms) == "table" then
		for _, charmId in ipairs(profile.equippedCharms) do
			local cDef = if charmsCfg.get then charmsCfg.get(charmId) else charmsCfg.ById and charmsCfg.ById[charmId]
			if cDef and type(cDef.luckBonus) == "number" then
				charmBonus += cDef.luckBonus
			end
		end
	end

	local skillTreeBonus = 0
	local treeCfg = (Config :: any).SkillTree
	if treeCfg and type(profile.skillNodes) == "table" then
		for nodeId, rank in pairs(profile.skillNodes) do
			local nDef = if treeCfg.get then treeCfg.get(nodeId) else treeCfg.ById and treeCfg.ById[nodeId]
			if nDef and type(nDef.luckBonus) == "number" then
				skillTreeBonus += nDef.luckBonus * (rank or 0)
			end
		end
	end

	local weatherBonus = 0
	local okW, weatherMod = pcall(function()
		return require(script.Parent:FindFirstChild("WeatherService") :: any)
	end)
	if okW and type(weatherMod) == "table" and type(weatherMod.getLuckBonus) == "function" then
		weatherBonus = weatherMod.getLuckBonus()
	end

	local passBonus = 0
	local passes = profile.ownedPasses or {}
	if passes.vip_pass then
		passBonus += 0.25
	end
	if passes.double_luck then
		passBonus += 1.0
	end
	if passes.ultra_luck then
		passBonus += 1.5
	end

	local premiumBonus = if player and player.MembershipType == Enum.MembershipType.Premium then 0.15 else 0
	local groupBonus = if player and player:GetAttribute("InGroup") == true then 1.0 else 0

	return {
		luckUpgradeLevel = profile.luckUpgradeLevel or 0,
		potionBonus = potionBonus,
		streakBonus = math.min(0.25, (profile.loginStreak or profile.dailyStreak or 0) * 0.05),
		indexBonus = indexBonus,
		weatherBonus = weatherBonus,
		skillTreeBonus = skillTreeBonus,
		charmBonus = charmBonus,
		rebirthBonus = rebirthBonus,
		passBonus = passBonus,
		premiumBonus = premiumBonus,
		groupBonus = groupBonus,
		eventMult = 1 + rebirthBonus,
	}
end

function RollService.getRollCost(worldId: number): number
	local worldDef = Worlds.get(worldId)
	if worldDef and type((worldDef :: any).rollCostYen) == "number" then
		return (worldDef :: any).rollCostYen
	end
	local eco = Economy :: any
	if type(eco.BaseRollCostYen) == "number" then
		return eco.BaseRollCostYen
	end
	return 50
end

function RollService.selectBanner(
	profile: DataService.PlayerProfile,
	worldId: number?,
	bannerId: string?
): (boolean, string?)
	local targetWorld = math.max(1, math.floor(worldId or profile.selectedWorld or profile.currentWorld or 1))
	if not profile.unlockedWorlds[targetWorld] then
		return false, "World is locked."
	end
	profile.selectedWorld = targetWorld
	profile.currentWorld = targetWorld
	profile.selectedBannerId = if type(bannerId) == "string" and bannerId ~= ""
		then bannerId
		else ("world_" .. tostring(targetWorld))
	return true, nil
end

function RollService.rollSingle(
	profile: DataService.PlayerProfile,
	worldId: number,
	useTicket: boolean?,
	friendCount: number?,
	bannerId: string?,
	player: Player?
): (boolean, string?, { [string]: any }?)
	local targetWorld = math.max(1, math.floor(worldId or profile.selectedWorld or profile.currentWorld or 1))
	if type(bannerId) == "string" and bannerId ~= "" then
		local bannersCfg = (Config :: any).Banners
		if bannersCfg and type(bannersCfg.get) == "function" then
			local bDef = bannersCfg.get(bannerId)
			if bDef and type(bDef.worldId) == "number" then
				targetWorld = bDef.worldId
			end
		end
	end
	if not profile.unlockedWorlds[targetWorld] then
		return false, "World is locked.", nil
	end

	local spentYen = 0
	local spentTicket = false
	if useTicket then
		if (profile.tickets or 0) < 1 then
			return false, "Not enough Summon Tickets.", nil
		end
		profile.tickets -= 1
		spentTicket = true
	else
		local cost = RollService.getRollCost(targetWorld)
		if (profile.yen or 0) < cost then
			return false, string.format("Need %d Yen to roll.", cost), nil
		end
		profile.yen -= cost
		spentYen = cost
	end

	local luckInputs = RollService.getLuckInputs(profile, friendCount, player)
	local outcome = LuckMath.performRoll(targetWorld, luckInputs, profile.pity, profile.traitPity)
	profile.pity = outcome.nextPity
	profile.traitPity = outcome.nextTraitPity
	profile.rollCount = (profile.rollCount or 0) + 1

	local cardDef = outcome.card
	local cardName = (cardDef :: any).name or cardDef.displayName or cardDef.id
	local isNew = profile.discoveredCards[cardDef.id] ~= true
	local instance = DataService.addCard(profile, cardDef.id, outcome.mutation, outcome.trait)
	profile.discoveredCards[cardDef.id] = true
	local stats =
		LuckMath.computeCardStats(cardDef, instance.level, instance.mutation, instance.trait, instance.ascension)

	return true,
		nil,
		{
			uid = instance.uid,
			id = cardDef.id,
			cardId = cardDef.id,
			name = cardName,
			displayName = cardName,
			rarity = outcome.rarity or cardDef.rarity,
			role = cardDef.role,
			showName = cardDef.showName,
			mutation = outcome.mutation,
			trait = outcome.trait,
			oddsN = outcome.oddsN,
			isNew = isNew,
			hp = stats.hp,
			atk = stats.atk,
			spd = stats.spd,
			power = stats.power,
			artKey = "cards/" .. cardDef.id,
			isPity = outcome.isPity,
			pity = profile.pity,
			spentYen = spentYen,
			spentTicket = spentTicket,
			instance = instance,
		}
end

function RollService.rollBatch(
	profile: DataService.PlayerProfile,
	worldId: number,
	count: number,
	useTicket: boolean?,
	friendCount: number?,
	bannerId: string?,
	player: Player?
): (boolean, string?, { { [string]: any } }?)
	local n = math.clamp(math.floor(count or 1), 1, 10)
	local results: { { [string]: any } } = {}
	for _ = 1, n do
		local ok, err, res = RollService.rollSingle(profile, worldId, useTicket, friendCount, bannerId, player)
		if not ok then
			if #results == 0 then
				return false, err, nil
			end
			break
		end
		if res then
			table.insert(results, res)
		end
	end
	return true, nil, results
end

function RollService.traitReroll(
	profile: DataService.PlayerProfile,
	instanceId: string
): (boolean, string?, { [string]: any }?)
	local card = profile.cards[instanceId]
	if not card then
		return false, "Card not found.", nil
	end
	if (profile.traitCrystals or 0) < 1 then
		return false, "Need 1 Trait Crystal to reroll.", nil
	end
	profile.traitCrystals -= 1
	local newTrait, nextPity = LuckMath.rollTrait(profile.traitPity or 0)
	profile.traitPity = nextPity
	card.trait = newTrait
	return true, nil, { uid = card.uid, trait = newTrait, traitPity = nextPity }
end

return RollService
