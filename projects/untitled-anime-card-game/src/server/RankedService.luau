--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Cards = require(Config:WaitForChild("Cards"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DataService = require(script.Parent:WaitForChild("DataService"))

local RankedService = {}

RankedService.REWARD_CARD_ID = "excl_hokage_flash_kcm"
RankedService.GRANDMASTER_ELO = 2200

export type RankTier = {
	id: string,
	name: string,
	minElo: number,
	yenWin: number,
	gemWin: number,
}

export type DefenseSnapshot = {
	ownerId: number,
	ownerName: string,
	elo: number,
	power: number,
	cards: { { cardId: string, level: number, mutation: string, trait: string?, ascension: number } },
	updatedAt: number,
}

local TIERS: { RankTier } = {
	{ id = "Bronze", name = "Bronze", minElo = 0, yenWin = 400, gemWin = 15 },
	{ id = "Silver", name = "Silver", minElo = 1000, yenWin = 700, gemWin = 22 },
	{ id = "Gold", name = "Gold", minElo = 1250, yenWin = 1100, gemWin = 30 },
	{ id = "Platinum", name = "Platinum", minElo = 1500, yenWin = 1600, gemWin = 40 },
	{ id = "Diamond", name = "Diamond", minElo = 1800, yenWin = 2300, gemWin = 55 },
	{ id = "Master", name = "Master", minElo = 2000, yenWin = 3200, gemWin = 75 },
	{ id = "Grandmaster", name = "Grandmaster", minElo = 2200, yenWin = 4500, gemWin = 110 },
}

local NPC_OPPONENTS: { DefenseSnapshot } = {
	{
		ownerId = -101,
		ownerName = "KonohaAcademyBot",
		elo = 900,
		power = 420,
		updatedAt = 0,
		cards = {
			{ cardId = "w1_academy_genin", level = 5, mutation = "Normal", trait = nil, ascension = 0 },
			{ cardId = "w1_tactical_chunin", level = 5, mutation = "Normal", trait = nil, ascension = 0 },
		},
	},
	{
		ownerId = -102,
		ownerName = "WestCitySpar",
		elo = 1200,
		power = 980,
		updatedAt = 0,
		cards = {
			{ cardId = "w1_crimson_copy_ninja", level = 12, mutation = "Foil", trait = nil, ascension = 1 },
			{ cardId = "w2_namekian_guardian", level = 12, mutation = "Normal", trait = nil, ascension = 0 },
			{ cardId = "w2_saiyan_prince", level = 10, mutation = "Normal", trait = nil, ascension = 0 },
		},
	},
	{
		ownerId = -103,
		ownerName = "GrandLineFleet",
		elo = 1550,
		power = 2100,
		updatedAt = 0,
		cards = {
			{ cardId = "w3_pirate_hunter", level = 22, mutation = "Holo", trait = nil, ascension = 1 },
			{ cardId = "w3_surgeon_of_death", level = 22, mutation = "Foil", trait = nil, ascension = 1 },
			{ cardId = "w3_flame_emperor", level = 20, mutation = "Normal", trait = nil, ascension = 1 },
		},
	},
	{
		ownerId = -104,
		ownerName = "ShibuyaSpecialGrade",
		elo = 1900,
		power = 3800,
		updatedAt = 0,
		cards = {
			{ cardId = "w4_queen_of_curses", level = 32, mutation = "Holo", trait = nil, ascension = 2 },
			{ cardId = "w4_sorcerer_killer", level = 32, mutation = "Holo", trait = nil, ascension = 2 },
			{ cardId = "w4_king_of_curses", level = 30, mutation = "Foil", trait = nil, ascension = 1 },
		},
	},
	{
		ownerId = -105,
		ownerName = "SoulKingVanguard",
		elo = 2250,
		power = 6400,
		updatedAt = 0,
		cards = {
			{ cardId = "w5_head_captain_inferno", level = 45, mutation = "Cosmic", trait = nil, ascension = 3 },
			{ cardId = "w5_transcendent_strategist", level = 45, mutation = "Holo", trait = nil, ascension = 2 },
			{ cardId = "excl_hokage_flash_kcm", level = 45, mutation = "Holo", trait = nil, ascension = 2 },
		},
	},
}

local defenseRegistry: { [number]: DefenseSnapshot } = {}

function RankedService.getTier(elo: number): RankTier
	local best = TIERS[1]
	for _, tier in ipairs(TIERS) do
		if elo >= tier.minElo then
			best = tier
		end
	end
	return best
end

function RankedService.saveDefenseSnapshot(
	userId: number,
	ownerName: string,
	profile: DataService.PlayerProfile
): DefenseSnapshot?
	local pAny = profile :: any
	local deck = if type(profile.defenseDeck) == "table" and #profile.defenseDeck > 0
		then profile.defenseDeck
		else profile.activeDeck
	local cards = {}
	local power = 0
	for _, uid in ipairs(deck or {}) do
		local inst = profile.cards[uid]
		if inst then
			table.insert(cards, {
				cardId = inst.cardId,
				level = inst.level or 1,
				mutation = inst.mutation or "Normal",
				trait = inst.trait,
				ascension = inst.ascension or 0,
			})
			local unit = BattleSim.buildPlayerUnit(#cards, inst)
			if unit then
				power += unit.maxHp + unit.atk * 4 + unit.spd * 2
			end
		end
	end
	if #cards == 0 then
		return nil
	end
	local snap: DefenseSnapshot = {
		ownerId = userId,
		ownerName = ownerName,
		elo = if type(pAny.elo) == "number"
			then pAny.elo
			elseif type(pAny.rankedElo) == "number" then pAny.rankedElo
			else 1000,
		power = math.floor(power),
		cards = cards,
		updatedAt = os.time(),
	}
	defenseRegistry[userId] = snap
	return snap
end

function RankedService.pickOpponent(userId: number, playerElo: number): DefenseSnapshot
	local best: DefenseSnapshot? = nil
	local bestDiff = math.huge
	for uid, snap in pairs(defenseRegistry) do
		if uid ~= userId and #snap.cards > 0 then
			local diff = math.abs(snap.elo - playerElo)
			if diff < bestDiff then
				bestDiff = diff
				best = snap
			end
		end
	end
	for _, npc in ipairs(NPC_OPPONENTS) do
		local diff = math.abs(npc.elo - playerElo)
		if diff < bestDiff then
			bestDiff = diff
			best = npc
		end
	end
	return best or NPC_OPPONENTS[1]
end

local function readElo(pAny: any): number
	if type(pAny.elo) == "number" then
		return pAny.elo
	elseif type(pAny.rankedElo) == "number" then
		return pAny.rankedElo
	end
	return 1000
end

function RankedService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	local elo = readElo(pAny)
	local tier = RankedService.getTier(elo)
	return {
		elo = elo,
		tier = tier.name,
		wins = pAny.rankedWins or 0,
		losses = pAny.rankedLosses or 0,
		grandmasterClaimed = pAny.rankedRewardClaimed == true,
		rewardCardId = RankedService.REWARD_CARD_ID,
		canClaimReward = elo >= RankedService.GRANDMASTER_ELO and pAny.rankedRewardClaimed ~= true,
	}
end

function RankedService.claimGrandmasterReward(
	profile: DataService.PlayerProfile
): (boolean, { [string]: any }?, string?)
	local pAny = profile :: any
	local elo = readElo(pAny)
	if elo < RankedService.GRANDMASTER_ELO then
		return false, nil, string.format("Reach Grandmaster (%d Elo) to claim Minato", RankedService.GRANDMASTER_ELO)
	end
	if pAny.rankedRewardClaimed == true then
		return false, nil, "Grandmaster seasonal reward already claimed"
	end
	pAny.rankedRewardClaimed = true
	local inst = DataService.addCard(profile, RankedService.REWARD_CARD_ID, "Normal", nil)
	return true, { cardId = RankedService.REWARD_CARD_ID, uid = inst.uid }, nil
end

function RankedService.playMatch(
	player: Player?,
	profile: DataService.PlayerProfile
): (boolean, { [string]: any }?, string?)
	local pAny = profile :: any
	local currentElo = readElo(pAny)
	pAny.elo = currentElo
	pAny.rankedElo = currentElo
	pAny.rankedWins = pAny.rankedWins or 0
	pAny.rankedLosses = pAny.rankedLosses or 0

	local userId = if player then player.UserId else 1
	local playerName = if player then player.Name else "Tester"
	RankedService.saveDefenseSnapshot(userId, playerName, profile)

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

	local opp = RankedService.pickOpponent(userId, currentElo)
	local enemies: { BattleSim.CombatUnit } = {}
	for idx, spec in ipairs(opp.cards) do
		local cardDef = (Cards :: any).get(spec.cardId)
		if cardDef then
			local unit = BattleSim.buildPlayerUnit(idx, {
				uid = "opp_" .. tostring(idx),
				cardId = spec.cardId,
				level = spec.level,
				mutation = spec.mutation,
				trait = spec.trait,
				ascension = spec.ascension,
			})
			if unit then
				unit.team = "Enemy"
				table.insert(enemies, unit)
			end
		end
	end
	if #enemies == 0 then
		table.insert(
			enemies,
			BattleSim.buildEnemyUnit(1, {
				id = "w1_academy_genin",
				name = opp.ownerName,
				rarity = "Common",
				role = "Striker",
				hp = 300,
				atk = 40,
				spd = 15,
			})
		)
	end

	local seed = math.floor(os.clock() * 1000) + userId * 31 + currentElo
	local sim = BattleSim.run(allies, enemies, { seed = seed, maxRounds = 35 })

	local expected = 1 / (1 + math.pow(10, (opp.elo - currentElo) / 400))
	local score = if sim.won then 1 else 0
	local delta = math.round(32 * (score - expected))
	if sim.won and delta < 8 then
		delta = 8
	elseif not sim.won and delta > -4 then
		delta = -4
	end

	local oldElo = currentElo
	local newElo = math.max(0, oldElo + delta)
	pAny.elo = newElo
	pAny.rankedElo = newElo
	if sim.won then
		pAny.rankedWins += 1
	else
		pAny.rankedLosses += 1
	end

	local tier = RankedService.getTier(newElo)
	local yenEarned = if sim.won then tier.yenWin else 100
	local gemEarned = if sim.won then tier.gemWin else 3
	profile.yen += yenEarned
	profile.gems += gemEarned

	local grandmasterCard: string? = nil
	if newElo >= RankedService.GRANDMASTER_ELO and pAny.rankedRewardClaimed ~= true then
		pAny.rankedRewardClaimed = true
		DataService.addCard(profile, RankedService.REWARD_CARD_ID, "Normal", nil)
		grandmasterCard = RankedService.REWARD_CARD_ID
	end

	return true,
		{
			won = sim.won,
			rounds = sim.rounds,
			events = sim.events,
			opponent = { name = opp.ownerName, elo = opp.elo },
			oldElo = oldElo,
			newElo = newElo,
			delta = newElo - oldElo,
			tier = tier.name,
			rewards = { yen = yenEarned, gems = gemEarned, grandmasterCard = grandmasterCard },
		},
		nil
end

return RankedService
