--!strict
local Rarities = require(script.Parent.Config.Rarities)
local Mutations = require(script.Parent.Config.Mutations)
local Traits = require(script.Parent.Config.Traits)
local Cards = require(script.Parent.Config.Cards)

export type LuckInputs = {
	luckUpgradeLevel: number?,
	vipPass: boolean?,
	superLuckPass: boolean?,
	ultraLuckPass: boolean?,
	potionMult: number?,
	potionBonus: number?,
	indexBonus: number?,
	streakBonus: number?,
	eventMult: number?,
	promoMult: number?,
}

export type LuckBreakdown = {
	totalMult: number,
	base: number,
	upgradeBonus: number,
	indexBonus: number,
	streakBonus: number,
	passMult: number,
	potionMult: number,
	eventMult: number,
	promoMult: number,
}

export type PityState = {
	epic: number,
	legendary: number,
	mythic: number,
}

export type CardStats = {
	hp: number,
	atk: number,
	spd: number,
	power: number,
}

export type RollOutcome = {
	card: Cards.CardDef,
	rarity: Rarities.RarityId,
	mutation: Mutations.MutationId,
	trait: Traits.TraitId?,
	nextPity: PityState,
	nextTraitPity: number,
	oddsN: number,
	isPity: boolean,
}

local LuckMath = {}

function LuckMath.makeSeededRng(seed: number): () -> number
	local state = math.abs(math.floor(seed)) % 4294967296
	if state == 0 then
		state = 1234567
	end
	return function(): number
		state = (state * 1664525 + 1013904223) % 4294967296
		return state / 4294967296
	end
end

function LuckMath.computeLuck(inputs: LuckInputs?): LuckBreakdown
	local inp = (inputs or {}) :: LuckInputs
	local base = 1.0
	local upgradeBonus = math.max(0, inp.luckUpgradeLevel or 0) * 0.15
	local indexBonus = math.max(0, inp.indexBonus or 0)
	local streakBonus = math.max(0, inp.streakBonus or 0)
	local potionBonus = math.max(0, inp.potionBonus or 0)
	local additive = base + upgradeBonus + indexBonus + streakBonus + potionBonus

	local passMult = 1.0
	if inp.vipPass then
		passMult *= 1.25
	end
	if inp.superLuckPass then
		passMult *= 1.5
	end
	if inp.ultraLuckPass then
		passMult *= 2.0
	end

	local potionMult = math.max(1.0, inp.potionMult or 1.0)
	local eventMult = math.max(1.0, inp.eventMult or 1.0)
	local promoMult = math.max(1.0, inp.promoMult or 1.0)
	local totalMult = additive * passMult * potionMult * eventMult * promoMult

	return {
		totalMult = totalMult,
		base = base,
		upgradeBonus = upgradeBonus,
		indexBonus = indexBonus,
		streakBonus = streakBonus,
		passMult = passMult,
		potionMult = potionMult,
		eventMult = eventMult,
		promoMult = promoMult,
	}
end

function LuckMath.computeRarityOdds(totalLuck: number, pity: PityState?): { [Rarities.RarityId]: number }
	local pState = pity or { epic = 0, legendary = 0, mythic = 0 }
	local luck = math.max(1.0, totalLuck)
	local remaining = 1.0
	local odds = {} :: { [Rarities.RarityId]: number }

	for _, rarityId in ipairs(Rarities.TopDownOrder) do
		local def = Rarities.ById[rarityId]
		local prob = def.baseProb * luck
		if def.softPity and def.softPitySlope then
			local count = 0
			if rarityId == "Legendary" then
				count = pState.legendary or 0
			elseif rarityId == "Mythic" then
				count = pState.mythic or 0
			end
			if count >= def.softPity then
				prob += (count - def.softPity + 1) * def.softPitySlope
			end
		end
		prob = math.min(prob, def.cap)
		local actual = math.min(prob, remaining)
		odds[rarityId] = actual
		remaining = math.max(0, remaining - actual)
	end
	odds.Common = remaining

	return odds
end

function LuckMath.formatOddsN(prob: number): number
	if prob <= 0 then
		return 999999999
	end
	return math.max(1, math.round(1 / prob))
end

function LuckMath.rollRarity(
	totalLuck: number,
	pity: PityState?,
	rng: (() -> number)?
): (Rarities.RarityId, PityState, boolean)
	local randFn = rng or math.random
	local pState = pity or { epic = 0, legendary = 0, mythic = 0 }
	local odds = LuckMath.computeRarityOdds(totalLuck, pState)

	local rollVal = randFn()
	local cum = 0
	local rolled: Rarities.RarityId = "Common"
	for _, rarityId in ipairs(Rarities.TopDownOrder) do
		cum += odds[rarityId] or 0
		if rollVal <= cum then
			rolled = rarityId
			break
		end
	end

	local nextEpic = (pState.epic or 0) + 1
	local nextLeg = (pState.legendary or 0) + 1
	local nextMythic = (pState.mythic or 0) + 1
	local isPity = false

	local mythicHard = Rarities.ById.Mythic.hardPity or 15000
	local legHard = Rarities.ById.Legendary.hardPity or 750
	local epicHard = Rarities.ById.Epic.hardPity or 100

	if nextMythic >= mythicHard and not Rarities.isAtLeast(rolled, "Mythic") then
		rolled = "Mythic"
		isPity = true
	elseif nextLeg >= legHard and not Rarities.isAtLeast(rolled, "Legendary") then
		rolled = "Legendary"
		isPity = true
	elseif nextEpic >= epicHard and not Rarities.isAtLeast(rolled, "Epic") then
		rolled = "Epic"
		isPity = true
	end

	local outPity: PityState
	if Rarities.isAtLeast(rolled, "Mythic") then
		outPity = { epic = 0, legendary = 0, mythic = 0 }
	elseif Rarities.isAtLeast(rolled, "Legendary") then
		outPity = { epic = 0, legendary = 0, mythic = nextMythic }
	elseif Rarities.isAtLeast(rolled, "Epic") then
		outPity = { epic = 0, legendary = nextLeg, mythic = nextMythic }
	else
		outPity = { epic = nextEpic, legendary = nextLeg, mythic = nextMythic }
	end

	return rolled, outPity, isPity
end

function LuckMath.rollMutation(totalLuck: number, rng: (() -> number)?): Mutations.MutationId
	local randFn = rng or math.random
	local luckBoost = math.min(3.0, 1.0 + math.log10(math.max(1.0, totalLuck)) * 0.5)
	local weights: { { id: Mutations.MutationId, w: number } } = {}
	local totalWeight = 0

	for _, mutId in ipairs(Mutations.Order) do
		local def = Mutations.ById[mutId]
		local w = if mutId == "Normal" then def.baseWeight else def.baseWeight * luckBoost
		totalWeight += w
		table.insert(weights, { id = mutId, w = w })
	end

	local target = randFn() * totalWeight
	local acc = 0
	for _, entry in ipairs(weights) do
		acc += entry.w
		if target <= acc then
			return entry.id
		end
	end
	return "Normal"
end

function LuckMath.rollTrait(traitPity: number?, rng: (() -> number)?): (Traits.TraitId, number)
	local randFn = rng or math.random
	local nextCount = (traitPity or 0) + 1
	local forceEpicPlus = nextCount >= Traits.PityEpicPlus

	local pool: { { id: Traits.TraitId, w: number, tier: Traits.TraitTier } } = {}
	local totalWeight = 0
	for _, traitId in ipairs(Traits.Order) do
		local def = Traits.ById[traitId]
		if not forceEpicPlus or def.tier == "Epic" or def.tier == "Mythic" then
			totalWeight += def.weight
			table.insert(pool, { id = traitId, w = def.weight, tier = def.tier })
		end
	end

	local target = randFn() * totalWeight
	local acc = 0
	local chosen: Traits.TraitId = "Swift"
	local chosenTier: Traits.TraitTier = "Common"
	for _, entry in ipairs(pool) do
		acc += entry.w
		if target <= acc then
			chosen = entry.id
			chosenTier = entry.tier
			break
		end
	end

	local outPity = if chosenTier == "Epic" or chosenTier == "Mythic" then 0 else nextCount
	return chosen, outPity
end

function LuckMath.pickCardForRarity(worldId: number, rarityId: Rarities.RarityId, rng: (() -> number)?): Cards.CardDef
	local randFn = rng or math.random
	local worldPool = Cards.getPoolForWorld(worldId)
	local exact: { Cards.CardDef } = {}
	for _, card in ipairs(worldPool) do
		if card.rarity == rarityId then
			table.insert(exact, card)
		end
	end
	if #exact > 0 then
		local idx = math.clamp(math.floor(randFn() * #exact) + 1, 1, #exact)
		return exact[idx]
	end

	local globalExact: { Cards.CardDef } = {}
	for _, card in ipairs(Cards.List) do
		if card.bannerPool and card.rarity == rarityId then
			table.insert(globalExact, card)
		end
	end
	if #globalExact > 0 then
		local idx = math.clamp(math.floor(randFn() * #globalExact) + 1, 1, #globalExact)
		return globalExact[idx]
	end

	if #worldPool > 0 then
		return worldPool[1]
	end
	return Cards.List[1]
end

function LuckMath.computeCardStats(
	cardDef: Cards.CardDef,
	level: number?,
	mutationId: string?,
	traitId: string?,
	ascension: number?
): CardStats
	local lvl = math.max(1, level or 1)
	local asc = math.max(0, ascension or 0)
	local lvlScale = 1 + (lvl - 1) * 0.12
	local ascScale = 1 + asc * 0.25
	local mutMult = Mutations.get(mutationId or "Normal").mult
	local traitDef = Traits.get(traitId)

	local atkTrait = if traitDef then traitDef.atkMult else 1.0
	local hpTrait = if traitDef then traitDef.hpMult else 1.0
	local spdTrait = if traitDef then traitDef.spdMult else 1.0
	local powerTrait = if traitDef then traitDef.powerMult else 1.0

	local hp = math.floor(cardDef.baseHP * lvlScale * ascScale * mutMult * hpTrait)
	local atk = math.floor(cardDef.baseATK * lvlScale * ascScale * mutMult * atkTrait)
	local spd = math.floor(cardDef.baseSPD * spdTrait)
	local baseRarityPower = Rarities.get(cardDef.rarity).basePower
	local power = math.floor(baseRarityPower * lvlScale * ascScale * mutMult * powerTrait)

	return {
		hp = hp,
		atk = atk,
		spd = spd,
		power = power,
	}
end

function LuckMath.performRoll(
	worldId: number,
	luckInputs: LuckInputs?,
	pity: PityState?,
	traitPity: number?,
	rng: (() -> number)?
): RollOutcome
	local randFn = rng or math.random
	local breakdown = LuckMath.computeLuck(luckInputs)
	local oddsMap = LuckMath.computeRarityOdds(breakdown.totalMult, pity)
	local rarity, nextPity, isPity = LuckMath.rollRarity(breakdown.totalMult, pity, randFn)
	local mutation = LuckMath.rollMutation(breakdown.totalMult, randFn)
	local card = LuckMath.pickCardForRarity(worldId, rarity, randFn)

	local trait: Traits.TraitId? = nil
	local nextTraitPity = traitPity or 0
	if Rarities.isAtLeast(rarity, "Rare") then
		local rolledTrait, updatedTP = LuckMath.rollTrait(nextTraitPity, randFn)
		trait = rolledTrait
		nextTraitPity = updatedTP
	end

	local prob = oddsMap[rarity] or Rarities.get(rarity).baseProb
	local oddsN = LuckMath.formatOddsN(prob)

	return {
		card = card,
		rarity = rarity,
		mutation = mutation,
		trait = trait,
		nextPity = nextPity,
		nextTraitPity = nextTraitPity,
		oddsN = oddsN,
		isPity = isPity,
	}
end

return LuckMath
