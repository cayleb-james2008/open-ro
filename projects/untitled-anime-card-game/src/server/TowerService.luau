--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Cards = require(Config:WaitForChild("Cards"))
local Worlds = require(Config:WaitForChild("Worlds"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local TowerService = {}

TowerService.MAX_FLOOR = 100
TowerService.FLOOR_100_REWARD_CARD = "excl_temporal_attendant"

local BOSS_CARD_CYCLE = {
	"boss_ten_tails_vessel",
	"boss_golden_tyrant",
	"boss_strongest_creature",
	"boss_divine_general",
	"boss_almighty_king",
}

export type FloorConfig = {
	floor: number,
	isBossFloor: boolean,
	enemies: {
		[number]: {
			cardId: string,
			name: string,
			hp: number,
			atk: number,
			spd: number,
			skillName: string,
			skillMult: number,
		},
	},
	yenReward: number,
	gemReward: number,
	traitCrystalReward: number,
	dropCardId: string?,
	dropChance: number,
}

local function getCardSafe(cardId: string): any
	local cardsAny = Cards :: any
	if type(cardsAny.get) == "function" then
		return cardsAny.get(cardId)
	end
	if type(cardsAny.ById) == "table" then
		return cardsAny.ById[cardId]
	end
	return nil
end

function TowerService.getFloorConfig(floor: number): FloorConfig
	local f = math.clamp(math.floor(floor), 1, TowerService.MAX_FLOOR)
	local worldsAny = Worlds :: any
	if type(worldsAny.getTowerFloor) == "function" then
		local custom = worldsAny.getTowerFloor(f)
		if type(custom) == "table" and type(custom.enemies) == "table" and #custom.enemies > 0 then
			return custom :: FloorConfig
		end
	end

	local isBoss = (f % 10 == 0)
	local scale = math.pow(1.062, f - 1)
	local baseHp = math.floor(140 * scale * (if isBoss then 1.65 else 1))
	local baseAtk = math.floor(22 * math.pow(1.048, f - 1) * (if isBoss then 1.35 else 1))
	local baseSpd = math.floor(14 + f * 0.45)

	local enemies = {}
	if f == TowerService.MAX_FLOOR then
		local whisDef = getCardSafe(TowerService.FLOOR_100_REWARD_CARD)
		table.insert(enemies, {
			cardId = TowerService.FLOOR_100_REWARD_CARD,
			name = if whisDef then (whisDef.name or whisDef.displayName) else "Temporal Attendant",
			hp = math.floor(baseHp * 1.4),
			atk = math.floor(baseAtk * 1.25),
			spd = baseSpd + 15,
			skillName = "Temporal Rewind",
			skillMult = 1.65,
		})
	elseif isBoss then
		local bossIdx = ((math.floor(f / 10) - 1) % #BOSS_CARD_CYCLE) + 1
		local bossId = BOSS_CARD_CYCLE[bossIdx]
		local bossDef = getCardSafe(bossId)
		table.insert(enemies, {
			cardId = bossId,
			name = if bossDef then (bossDef.name or bossDef.displayName) else ("Arena Guardian F" .. tostring(f)),
			hp = baseHp,
			atk = baseAtk,
			spd = baseSpd + 6,
			skillName = "Arena Dominion",
			skillMult = 1.5,
		})
	else
		local count = if f >= 50 then 3 elseif f >= 20 then 2 else 1
		for i = 1, count do
			local worldPick = math.clamp(math.ceil(f / 20), 1, 5)
			local fallbackId = BOSS_CARD_CYCLE[worldPick]
			table.insert(enemies, {
				cardId = fallbackId,
				name = string.format("Tower Challenger F%d-%d", f, i),
				hp = math.floor(baseHp / (1 + (count - 1) * 0.35)),
				atk = math.floor(baseAtk / (1 + (count - 1) * 0.25)),
				spd = baseSpd + i,
				skillName = "Challenger Strike",
				skillMult = 1.25,
			})
		end
	end

	return {
		floor = f,
		isBossFloor = isBoss,
		enemies = enemies,
		yenReward = math.floor(120 + f * 35),
		gemReward = if isBoss then 25 + math.floor(f / 2) else 5 + math.floor(f / 10),
		traitCrystalReward = if isBoss then 1 else 0,
		dropCardId = if f == TowerService.MAX_FLOOR then TowerService.FLOOR_100_REWARD_CARD else nil,
		dropChance = if f == TowerService.MAX_FLOOR then 1.0 else 0,
	}
end

function TowerService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	local clearedFloor = math.clamp(math.floor(pAny.towerFloor or 0), 0, TowerService.MAX_FLOOR)
	local nextFloor = math.min(TowerService.MAX_FLOOR, clearedFloor + 1)
	return {
		clearedFloor = clearedFloor,
		nextFloor = nextFloor,
		maxFloor = TowerService.MAX_FLOOR,
		nextConfig = TowerService.getFloorConfig(nextFloor),
	}
end

function TowerService.challengeFloor(
	player: Player?,
	profile: DataService.PlayerProfile,
	requestedFloor: number?
): (boolean, { [string]: any }?, string?)
	local pAny = profile :: any
	local clearedFloor = math.clamp(math.floor(pAny.towerFloor or 0), 0, TowerService.MAX_FLOOR)
	local maxPlayable = math.min(TowerService.MAX_FLOOR, clearedFloor + 1)
	local targetFloor = if type(requestedFloor) == "number" then math.floor(requestedFloor) else maxPlayable
	if targetFloor < 1 or targetFloor > maxPlayable then
		return false, nil, string.format("Unlock Floor %d by clearing previous floors first.", targetFloor)
	end

	local allies: { BattleSim.CombatUnit } = {}
	for idx, uid in ipairs(profile.activeDeck or {}) do
		local inst = profile.cards[uid]
		if inst then
			local unit = BattleSim.buildPlayerUnit(idx, {
				uid = inst.uid,
				cardId = inst.cardId,
				level = inst.level,
				mutation = inst.mutation,
				trait = inst.trait,
				ascension = inst.ascension,
			})
			if unit then
				table.insert(allies, unit)
			end
		end
	end
	if #allies == 0 then
		return false, nil, "Equip at least 1 card in your active deck!"
	end

	local cfg = TowerService.getFloorConfig(targetFloor)
	local enemies: { BattleSim.CombatUnit } = {}
	for idx, spec in ipairs(cfg.enemies) do
		local cardDef = getCardSafe(spec.cardId)
		table.insert(
			enemies,
			BattleSim.buildEnemyUnit(idx, {
				id = spec.cardId,
				name = spec.name,
				rarity = if cardDef then cardDef.rarity else "Epic",
				role = if cardDef then cardDef.role else "Striker",
				hp = spec.hp,
				atk = spec.atk,
				spd = spec.spd,
				skill = {
					name = spec.skillName,
					description = spec.skillName,
					cooldown = 3,
					dmgMult = spec.skillMult,
					targeting = "Single",
				},
			})
		)
	end

	local userId = if player then player.UserId else 1
	local seed = math.floor(os.clock() * 1000) + targetFloor * 97 + (profile.rollCount or 0) * 13 + userId
	local simResult = BattleSim.run(allies, enemies, { seed = seed, maxRounds = 35 })

	local rewards = {
		yen = 0,
		gems = 0,
		traitCrystals = 0,
		firstClear = false,
		droppedCardId = nil :: string?,
	}

	if simResult.won then
		local isFirstClear = targetFloor > clearedFloor
		if isFirstClear then
			pAny.towerFloor = targetFloor
		end
		local yenEarned = if isFirstClear then cfg.yenReward else math.floor(cfg.yenReward * 0.4)
		local gemsEarned = if isFirstClear then cfg.gemReward else math.max(1, math.floor(cfg.gemReward * 0.25))
		local crystalsEarned = if isFirstClear then cfg.traitCrystalReward else 0

		profile.yen = (profile.yen or 0) + yenEarned
		profile.gems = (profile.gems or 0) + gemsEarned
		profile.traitCrystals = (profile.traitCrystals or 0) + crystalsEarned

		rewards.yen = yenEarned
		rewards.gems = gemsEarned
		rewards.traitCrystals = crystalsEarned
		rewards.firstClear = isFirstClear

		if cfg.dropCardId then
			local alreadyOwned = profile.discoveredCards[cfg.dropCardId] == true
			if (isFirstClear and not alreadyOwned) or math.random() < (cfg.dropChance or 0.15) then
				DataService.addCard(profile, cfg.dropCardId, "Normal", nil)
				rewards.droppedCardId = cfg.dropCardId
			end
		end
	else
		local consolation = math.max(20, math.floor(cfg.yenReward * 0.15))
		profile.yen = (profile.yen or 0) + consolation
		rewards.yen = consolation
	end

	return true,
		{
			mode = "Tower",
			floor = targetFloor,
			won = simResult.won,
			rounds = simResult.rounds,
			events = simResult.events,
			rewards = rewards,
			towerFloor = pAny.towerFloor or clearedFloor,
		},
		nil
end

return TowerService
