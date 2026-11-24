--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Economy = require(Config:WaitForChild("Economy"))
local Worlds = require(Config:WaitForChild("Worlds"))
local LuckMath = require(Shared:WaitForChild("LuckMath"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local RollService = {}

function RollService.getLuckInputs(profile: DataService.PlayerProfile, _friendCount: number?): LuckMath.LuckInputs
	local now = os.time()
	local potionBonus = 0
	if (profile.luckPotionExpiresAt or 0) > now then
		potionBonus = profile.luckPotionBonus or 0
	end
	local rebirthMult = 1 + (profile.rebirths or 0) * 0.1
	return {
		luckUpgradeLevel = profile.luckUpgradeLevel or 0,
		potionBonus = potionBonus,
		streakBonus = math.min(0.25, (profile.dailyStreak or 0) * 0.05),
		eventMult = rebirthMult,
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

function RollService.rollSingle(
	profile: DataService.PlayerProfile,
	worldId: number,
	useTicket: boolean?,
	friendCount: number?
): (boolean, string?, { [string]: any }?)
	local targetWorld = math.max(1, math.floor(worldId or profile.currentWorld or 1))
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

	local luckInputs = RollService.getLuckInputs(profile, friendCount)
	local outcome = LuckMath.performRoll(targetWorld, luckInputs, profile.pity, profile.traitPity)
	profile.pity = outcome.nextPity
	profile.traitPity = outcome.nextTraitPity
	profile.rollCount = (profile.rollCount or 0) + 1

	local instance = DataService.addCard(profile, outcome.card.id, outcome.mutation, outcome.trait)
	profile.discoveredCards[outcome.card.id] = true

	return true,
		nil,
		{
			instance = instance,
			cardId = outcome.card.id,
			displayName = outcome.card.displayName,
			rarity = outcome.rarity,
			mutation = outcome.mutation,
			trait = outcome.trait,
			oddsN = outcome.oddsN,
			isPity = outcome.isPity,
			pity = profile.pity,
			spentYen = spentYen,
			spentTicket = spentTicket,
		}
end

function RollService.rollBatch(
	profile: DataService.PlayerProfile,
	worldId: number,
	count: number,
	useTicket: boolean?,
	friendCount: number?
): (boolean, string?, { { [string]: any } }?)
	local n = math.clamp(math.floor(count or 1), 1, 10)
	local results: { { [string]: any } } = {}
	for _ = 1, n do
		local ok, err, res = RollService.rollSingle(profile, worldId, useTicket, friendCount)
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

return RollService
