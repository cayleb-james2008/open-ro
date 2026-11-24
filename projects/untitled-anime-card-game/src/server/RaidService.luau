--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Cards = require(Config:WaitForChild("Cards"))
local Worlds = require(Config:WaitForChild("Worlds"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local RaidService = {}

export type RaidEnemySpec = {
	cardId: string,
	name: string,
	hp: number,
	atk: number,
	spd: number,
	skillName: string,
	skillMult: number,
}

export type RaidDef = {
	id: string,
	name: string,
	worldReq: number,
	entryCooldownSec: number,
	dropCardId: string,
	dropChance: number,
	pityThreshold: number,
	yenReward: number,
	gemReward: number,
	raidTokenReward: number,
	materialId: string,
	materialQty: number,
	waves: { { RaidEnemySpec } },
}

local DEFAULT_RAIDS: { [string]: RaidDef } = {
	raid_skull_island = {
		id = "raid_skull_island",
		name = "Skull Island Raid",
		worldReq = 3,
		entryCooldownSec = 300,
		dropCardId = "excl_pirate_king",
		dropChance = 0.15,
		pityThreshold = 15,
		yenReward = 4500,
		gemReward = 90,
		raidTokenReward = 15,
		materialId = "conqueror_crest",
		materialQty = 2,
		waves = {
			{
				{
					cardId = "w3_sniper_king",
					name = "Skull Lookout",
					hp = 950,
					atk = 95,
					spd = 22,
					skillName = "Lead Volley",
					skillMult = 1.25,
				},
				{
					cardId = "w3_pirate_hunter",
					name = "Deck Enforcer",
					hp = 1200,
					atk = 115,
					spd = 24,
					skillName = "Cross Slash",
					skillMult = 1.35,
				},
			},
			{
				{
					cardId = "w3_black_leg_chef",
					name = "First Mate",
					hp = 1550,
					atk = 135,
					spd = 28,
					skillName = "Gale Kick",
					skillMult = 1.4,
				},
				{
					cardId = "w3_red_haired_emperor",
					name = "Fleet Captain",
					hp = 1800,
					atk = 150,
					spd = 27,
					skillName = "Haki Burst",
					skillMult = 1.45,
				},
			},
			{
				{
					cardId = "excl_pirate_king",
					name = "Gol D. Roger",
					hp = 3800,
					atk = 210,
					spd = 34,
					skillName = "Divine Departure",
					skillMult = 1.7,
				},
			},
		},
	},
	raid_shibuya_breach = {
		id = "raid_shibuya_breach",
		name = "Shibuya Breach Raid",
		worldReq = 4,
		entryCooldownSec = 360,
		dropCardId = "excl_disaster_flame",
		dropChance = 0.20,
		pityThreshold = 12,
		yenReward = 7500,
		gemReward = 130,
		raidTokenReward = 22,
		materialId = "ember_core",
		materialQty = 2,
		waves = {
			{
				{
					cardId = "w4_cursed_speech_student",
					name = "Cursed Sentry",
					hp = 1600,
					atk = 140,
					spd = 26,
					skillName = "Hex Echo",
					skillMult = 1.3,
				},
				{
					cardId = "w4_black_flash_vessel",
					name = "Veil Striker",
					hp = 1900,
					atk = 165,
					spd = 29,
					skillName = "Cursed Impact",
					skillMult = 1.4,
				},
			},
			{
				{
					cardId = "w4_brain_of_curses",
					name = "Veil Architect",
					hp = 2600,
					atk = 195,
					spd = 31,
					skillName = "Gravity Domain",
					skillMult = 1.5,
				},
			},
			{
				{
					cardId = "excl_disaster_flame",
					name = "Disaster Flame",
					hp = 5400,
					atk = 275,
					spd = 38,
					skillName = "Coffin of Iron Mountain",
					skillMult = 1.8,
				},
			},
		},
	},
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

function RaidService.getRaidDef(raidId: string): RaidDef?
	local worldsAny = Worlds :: any
	if type(worldsAny.getRaid) == "function" then
		local custom = worldsAny.getRaid(raidId)
		if type(custom) == "table" and type(custom.waves) == "table" then
			return custom :: RaidDef
		end
	elseif type(worldsAny.Raids) == "table" and type(worldsAny.Raids[raidId]) == "table" then
		local custom = worldsAny.Raids[raidId]
		if type(custom.waves) == "table" then
			return custom :: RaidDef
		end
	end
	return DEFAULT_RAIDS[raidId]
end

function RaidService.listRaids(): { RaidDef }
	local list: { RaidDef } = {}
	for _, id in ipairs({ "raid_skull_island", "raid_shibuya_breach" }) do
		local def = RaidService.getRaidDef(id)
		if def then
			table.insert(list, def)
		end
	end
	return list
end

function RaidService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	local now = os.time()
	local cooldowns = if type(pAny.raidCooldowns) == "table" then pAny.raidCooldowns else {}
	local clears = if type(pAny.raidClears) == "table" then pAny.raidClears else {}
	local pityMap = if type(pAny.raidPity) == "table" then pAny.raidPity else {}
	local out = {}
	for _, def in ipairs(RaidService.listRaids()) do
		local readyAt = cooldowns[def.id] or 0
		out[def.id] = {
			id = def.id,
			name = def.name,
			worldReq = def.worldReq,
			unlocked = profile.unlockedWorlds[def.worldReq] == true,
			cooldownRemaining = math.max(0, readyAt - now),
			clears = clears[def.id] or 0,
			pity = pityMap[def.id] or 0,
			dropCardId = def.dropCardId,
		}
	end
	return { raidTokens = pAny.raidTokens or 0, raids = out }
end

function RaidService.startRaid(
	player: Player?,
	profile: DataService.PlayerProfile,
	raidId: string,
	bypassCooldown: boolean?
): (boolean, { [string]: any }?, string?)
	local def = RaidService.getRaidDef(raidId)
	if not def then
		return false, nil, "Unknown raid"
	end
	if profile.unlockedWorlds[def.worldReq] ~= true then
		return false, nil, string.format("Unlock World %d to enter %s", def.worldReq, def.name)
	end

	local pAny = profile :: any
	pAny.raidCooldowns = if type(pAny.raidCooldowns) == "table" then pAny.raidCooldowns else {}
	pAny.raidClears = if type(pAny.raidClears) == "table" then pAny.raidClears else {}
	pAny.raidPity = if type(pAny.raidPity) == "table" then pAny.raidPity else {}
	pAny.raidTokens = pAny.raidTokens or 0
	pAny.charmMaterials = if type(pAny.charmMaterials) == "table" then pAny.charmMaterials else {}

	local now = os.time()
	local readyAt = pAny.raidCooldowns[raidId] or 0
	if not bypassCooldown and now < readyAt then
		return false, nil, string.format("Raid cooling down (%ds remaining)", readyAt - now)
	end

	local allies: { BattleSim.CombatUnit } = {}
	for idx, uid in ipairs(profile.activeDeck or {}) do
		local inst = profile.cards[uid]
		if inst then
			local unit = BattleSim.buildPlayerUnit(idx, inst)
			if unit then
				table.insert(allies, unit)
			end
		end
	end
	if #allies == 0 then
		return false, nil, "Equip at least 1 card in your active deck!"
	end

	local userId = if player then player.UserId else 1
	local waveLogs = {}
	local finalWon = true

	for waveIdx, waveSpecs in ipairs(def.waves) do
		local livingAllies: { BattleSim.CombatUnit } = {}
		for _, a in ipairs(allies) do
			if a.hp > 0 then
				local copy = table.clone(a)
				table.insert(livingAllies, copy)
			end
		end
		if #livingAllies == 0 then
			finalWon = false
			break
		end

		local enemies: { BattleSim.CombatUnit } = {}
		for idx, spec in ipairs(waveSpecs) do
			local cardDef = getCardSafe(spec.cardId)
			table.insert(
				enemies,
				BattleSim.buildEnemyUnit(idx, {
					id = spec.cardId,
					name = spec.name,
					rarity = if cardDef then cardDef.rarity else "Mythic",
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

		local seed = math.floor(os.clock() * 1000) + waveIdx * 131 + userId + (profile.rollCount or 0)
		local sim = BattleSim.run(livingAllies, enemies, { seed = seed, maxRounds = 30 })
		-- Carry surviving HP forward to next wave
		for _, a in ipairs(allies) do
			for _, u in ipairs(livingAllies) do
				if a.uid == u.uid then
					a.hp = math.max(0, u.hp)
				end
			end
		end
		table.insert(waveLogs, { wave = waveIdx, won = sim.won, rounds = sim.rounds, events = sim.events })
		if not sim.won then
			finalWon = false
			break
		end
	end

	local droppedCardId: string? = nil
	local droppedUid: string? = nil
	if finalWon then
		pAny.raidCooldowns[raidId] = now + def.entryCooldownSec
		pAny.raidClears[raidId] = (pAny.raidClears[raidId] or 0) + 1
		profile.yen += def.yenReward
		profile.gems += def.gemReward
		pAny.raidTokens += def.raidTokenReward
		pAny.charmMaterials[def.materialId] = (pAny.charmMaterials[def.materialId] or 0) + def.materialQty

		local pity = (pAny.raidPity[raidId] or 0) + 1
		local rng = Random.new(math.floor(os.clock() * 1000) + userId + pAny.raidClears[raidId])
		if pity >= def.pityThreshold or rng:NextNumber() <= def.dropChance then
			pAny.raidPity[raidId] = 0
			local inst = DataService.addCard(profile, def.dropCardId, "Normal", nil)
			droppedCardId = def.dropCardId
			droppedUid = inst.uid
		else
			pAny.raidPity[raidId] = pity
		end
	end

	return true,
		{
			raidId = raidId,
			won = finalWon,
			wavesCleared = if finalWon then #def.waves else math.max(0, #waveLogs - 1),
			totalWaves = #def.waves,
			waveLogs = waveLogs,
			rewards = if finalWon
				then {
					yen = def.yenReward,
					gems = def.gemReward,
					raidTokens = def.raidTokenReward,
					materialId = def.materialId,
					materialQty = def.materialQty,
					droppedCardId = droppedCardId,
					droppedUid = droppedUid,
				}
				else { yen = 0, gems = 0, raidTokens = 0 },
		},
		nil
end

return RaidService
