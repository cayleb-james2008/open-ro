--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Cards = require(Config:WaitForChild("Cards"))
local Worlds = require(Config:WaitForChild("Worlds"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local WorldBossService = {}

export type WorldBossDef = {
	worldId: number,
	stageId: string,
	bossCardId: string,
	name: string,
	hp: number,
	atk: number,
	spd: number,
	skillName: string,
	skillMult: number,
	cooldownSec: number,
	dropChance: number,
	pityThreshold: number,
	yenReward: number,
	gemReward: number,
	traitCrystalReward: number,
	materialId: string,
}

local DEFAULT_BOSSES: { [number]: WorldBossDef } = {
	[1] = {
		worldId = 1,
		stageId = "w1_boss",
		bossCardId = "boss_ten_tails_vessel",
		name = "Ten-Tails Vessel",
		hp = 1800,
		atk = 95,
		spd = 22,
		skillName = "Truth-Seeking Cataclysm",
		skillMult = 1.65,
		cooldownSec = 180,
		dropChance = 0.15,
		pityThreshold = 10,
		yenReward = 1400,
		gemReward = 45,
		traitCrystalReward = 1,
		materialId = "chakra_orb",
	},
	[2] = {
		worldId = 2,
		stageId = "w2_boss",
		bossCardId = "boss_golden_tyrant",
		name = "Golden Tyrant",
		hp = 4200,
		atk = 165,
		spd = 28,
		skillName = "Golden Supernova",
		skillMult = 1.7,
		cooldownSec = 180,
		dropChance = 0.14,
		pityThreshold = 10,
		yenReward = 3200,
		gemReward = 65,
		traitCrystalReward = 2,
		materialId = "namek_core",
	},
	[3] = {
		worldId = 3,
		stageId = "w3_boss",
		bossCardId = "boss_strongest_creature",
		name = "Strongest Creature",
		hp = 9500,
		atk = 260,
		spd = 31,
		skillName = "Thunder Bagua",
		skillMult = 1.75,
		cooldownSec = 240,
		dropChance = 0.12,
		pityThreshold = 12,
		yenReward = 6800,
		gemReward = 90,
		traitCrystalReward = 2,
		materialId = "dragon_scale",
	},
	[4] = {
		worldId = 4,
		stageId = "w4_boss",
		bossCardId = "boss_divine_general",
		name = "Divine General",
		hp = 18000,
		atk = 390,
		spd = 35,
		skillName = "Eight-Handled Adaptation",
		skillMult = 1.8,
		cooldownSec = 240,
		dropChance = 0.10,
		pityThreshold = 14,
		yenReward = 12500,
		gemReward = 120,
		traitCrystalReward = 3,
		materialId = "dharma_wheel",
	},
	[5] = {
		worldId = 5,
		stageId = "w5_boss",
		bossCardId = "boss_almighty_king",
		name = "Almighty King",
		hp = 32000,
		atk = 560,
		spd = 42,
		skillName = "The Almighty Rewrite",
		skillMult = 1.9,
		cooldownSec = 300,
		dropChance = 0.08,
		pityThreshold = 16,
		yenReward = 24000,
		gemReward = 175,
		traitCrystalReward = 4,
		materialId = "soul_fragment",
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

function WorldBossService.getBossDef(worldId: number): WorldBossDef?
	local def = DEFAULT_BOSSES[worldId]
	if not def then
		return nil
	end
	local worldsAny = Worlds :: any
	if type(worldsAny.getStage) == "function" then
		local stage = worldsAny.getStage(def.stageId)
		if type(stage) == "table" and type(stage.enemies) == "table" and #stage.enemies > 0 then
			local copy = table.clone(def)
			local e = stage.enemies[1]
			copy.hp = e.hp or copy.hp
			copy.atk = e.atk or copy.atk
			copy.spd = e.spd or copy.spd
			return copy
		end
	end
	return def
end

function WorldBossService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	local now = os.time()
	local cooldowns = if type(pAny.worldBossCooldowns) == "table" then pAny.worldBossCooldowns else {}
	local pityMap = if type(pAny.worldBossPity) == "table" then pAny.worldBossPity else {}
	local out = {}
	for worldId = 1, 5 do
		local def = WorldBossService.getBossDef(worldId)
		if def then
			local readyAt = cooldowns[worldId] or 0
			out[tostring(worldId)] = {
				worldId = worldId,
				stageId = def.stageId,
				bossCardId = def.bossCardId,
				name = def.name,
				unlocked = profile.unlockedWorlds[worldId] == true,
				cooldownRemaining = math.max(0, readyAt - now),
				pity = pityMap[worldId] or 0,
				pityThreshold = def.pityThreshold,
			}
		end
	end
	return { bossTokens = profile.bossTokens, bosses = out }
end

function WorldBossService.challengeBoss(
	player: Player?,
	profile: DataService.PlayerProfile,
	worldId: number,
	useBossToken: boolean?
): (boolean, { [string]: any }?, string?)
	local wid = math.floor(worldId)
	local def = WorldBossService.getBossDef(wid)
	if not def then
		return false, nil, "Unknown World Boss"
	end
	if profile.unlockedWorlds[wid] ~= true then
		return false, nil, string.format("Unlock World %d first", wid)
	end

	local pAny = profile :: any
	pAny.worldBossCooldowns = if type(pAny.worldBossCooldowns) == "table" then pAny.worldBossCooldowns else {}
	pAny.worldBossPity = if type(pAny.worldBossPity) == "table" then pAny.worldBossPity else {}
	pAny.charmMaterials = if type(pAny.charmMaterials) == "table" then pAny.charmMaterials else {}

	local now = os.time()
	local readyAt = pAny.worldBossCooldowns[wid] or 0
	local spentToken = false
	if now < readyAt then
		if useBossToken and (profile.bossTokens or 0) >= 1 then
			profile.bossTokens -= 1
			spentToken = true
		else
			return false,
				nil,
				string.format("World Boss cooling down (%ds) — use a Boss Token to enter now", readyAt - now)
		end
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

	local cardDef = getCardSafe(def.bossCardId)
	local enemy = BattleSim.buildEnemyUnit(1, {
		id = def.bossCardId,
		name = def.name,
		rarity = if cardDef then cardDef.rarity else "Mythic",
		role = if cardDef then cardDef.role else "Boss",
		hp = def.hp,
		atk = def.atk,
		spd = def.spd,
		skill = {
			name = def.skillName,
			description = def.skillName,
			cooldown = 3,
			dmgMult = def.skillMult,
			targeting = "Single",
		},
	})

	local userId = if player then player.UserId else 1
	local seed = math.floor(os.clock() * 1000) + wid * 211 + userId + (profile.rollCount or 0)
	local sim = BattleSim.run(allies, { enemy }, { seed = seed, maxRounds = 35 })

	local droppedCardId: string? = nil
	local droppedUid: string? = nil
	if sim.won then
		if not spentToken then
			pAny.worldBossCooldowns[wid] = now + def.cooldownSec
		end
		profile.clearedStages[def.stageId] = true
		profile.yen += def.yenReward
		profile.gems += def.gemReward
		profile.traitCrystals += def.traitCrystalReward
		pAny.charmMaterials[def.materialId] = (pAny.charmMaterials[def.materialId] or 0) + 1

		local pity = (pAny.worldBossPity[wid] or 0) + 1
		local rng = Random.new(seed + 17)
		if pity >= def.pityThreshold or rng:NextNumber() <= def.dropChance then
			pAny.worldBossPity[wid] = 0
			local inst = DataService.addCard(profile, def.bossCardId, "Normal", nil)
			droppedCardId = def.bossCardId
			droppedUid = inst.uid
		else
			pAny.worldBossPity[wid] = pity
		end
	end

	return true,
		{
			worldId = wid,
			stageId = def.stageId,
			won = sim.won,
			rounds = sim.rounds,
			events = sim.events,
			rewards = if sim.won
				then {
					yen = def.yenReward,
					gems = def.gemReward,
					traitCrystals = def.traitCrystalReward,
					materialId = def.materialId,
					droppedCardId = droppedCardId,
					droppedUid = droppedUid,
				}
				else { yen = 0, gems = 0, traitCrystals = 0 },
		},
		nil
end

return WorldBossService
