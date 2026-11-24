--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = require(Shared:WaitForChild("Config"))
local ConfigFolder = Shared:WaitForChild("Config")
local Economy = require(ConfigFolder:WaitForChild("Economy"))
local Rarities = require(ConfigFolder:WaitForChild("Rarities"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))

local DeckService = {}

local MAX_DECK_SIZE = 5

local RARITY_ORDER: { [string]: number } = {
	Common = 1,
	Uncommon = 2,
	Rare = 3,
	Epic = 4,
	Legendary = 5,
	Mythic = 6,
	Secret = 7,
	Universal = 8,
	Nexus = 9,
}

local SELL_BASE: { [string]: number } = {
	Common = 15,
	Uncommon = 35,
	Rare = 80,
	Epic = 200,
	Legendary = 500,
	Mythic = 1500,
	Secret = 4000,
	Universal = 10000,
	Nexus = 25000,
}

local function isEquippedInAnyDeck(profile: any, uid: string): boolean
	for _, id in ipairs(profile.activeDeck or {}) do
		if id == uid then
			return true
		end
	end
	for _, id in ipairs(profile.defenseDeck or {}) do
		if id == uid then
			return true
		end
	end
	return false
end

function DeckService.getSellValue(inst: any): number
	local cardDef = Config.Cards.get(inst.cardId)
	local rarity = if cardDef then cardDef.rarity else "Common"
	local base = SELL_BASE[rarity] or 15
	local levelMult = 1 + ((inst.level or 1) - 1) * 0.15
	local ascMult = 1 + (inst.ascension or 0) * 0.5
	return math.max(10, math.floor(base * levelMult * ascMult))
end

local function validateDeckIds(profile: any, deckIds: { string }): (boolean, string?, { string })
	if type(deckIds) ~= "table" then
		return false, "Invalid deck list.", {}
	end
	if #deckIds == 0 then
		return false, "Deck must contain at least 1 card.", {}
	end
	if #deckIds > MAX_DECK_SIZE then
		return false, "Deck cannot exceed 5 cards.", {}
	end
	local seen: { [string]: boolean } = {}
	local clean: { string } = {}
	for _, uid in ipairs(deckIds) do
		if type(uid) ~= "string" or not profile.cards[uid] then
			return false, "Card not owned.", {}
		end
		if seen[uid] then
			return false, "Duplicate card instance in deck.", {}
		end
		seen[uid] = true
		table.insert(clean, uid)
	end
	return true, nil, clean
end

function DeckService.setDeck(profile: any, deckIds: { string }): (boolean, string?)
	local ok, err, clean = validateDeckIds(profile, deckIds)
	if not ok then
		return false, err
	end
	profile.activeDeck = clean
	return true, nil
end

function DeckService.saveDefenseDeck(profile: any, deckIds: { string }): (boolean, string?)
	local ok, err, clean = validateDeckIds(profile, deckIds)
	if not ok then
		return false, err
	end
	profile.defenseDeck = clean
	return true, nil
end

function DeckService.toggleLock(profile: any, instanceId: string): (boolean, string?, boolean?)
	local inst = profile.cards[instanceId]
	if not inst then
		return false, "Card not found.", nil
	end
	inst.locked = not (inst.locked == true)
	return true, nil, inst.locked
end

function DeckService.sellCard(profile: any, instanceId: string): (boolean, string?, number?)
	local inst = profile.cards[instanceId]
	if not inst then
		return false, "Card not found.", nil
	end
	if inst.locked then
		return false, "Card is locked.", nil
	end
	if isEquippedInAnyDeck(profile, instanceId) then
		return false, "Unequip card from Active/Defense deck before selling.", nil
	end
	local totalCards = 0
	for _ in pairs(profile.cards) do
		totalCards += 1
	end
	if totalCards <= 1 then
		return false, "Cannot sell your last remaining card.", nil
	end
	local value = DeckService.getSellValue(inst)
	profile.cards[instanceId] = nil
	profile.yen = (profile.yen or 0) + value
	return true, nil, value
end

function DeckService.quickSell(
	profile: any,
	maxRarity: string?
): (boolean, string?, { soldCount: number, yenGained: number }?)
	local capRank = RARITY_ORDER[maxRarity or "Uncommon"] or 2
	local toSell: { string } = {}
	local totalCards = 0
	for _ in pairs(profile.cards) do
		totalCards += 1
	end
	for uid, inst in pairs(profile.cards) do
		if not inst.locked and not isEquippedInAnyDeck(profile, uid) then
			local cardDef = Config.Cards.get(inst.cardId)
			local rarity = if cardDef then cardDef.rarity else "Common"
			local rank = RARITY_ORDER[rarity] or 1
			if rank <= capRank and totalCards - #toSell > 1 then
				table.insert(toSell, uid)
			end
		end
	end
	if #toSell == 0 then
		return false, "No eligible unlocked cards to quick-sell.", nil
	end
	local yenGained = 0
	for _, uid in ipairs(toSell) do
		local inst = profile.cards[uid]
		if inst then
			yenGained += DeckService.getSellValue(inst)
			profile.cards[uid] = nil
		end
	end
	profile.yen = (profile.yen or 0) + yenGained
	return true, nil, { soldCount = #toSell, yenGained = yenGained }
end

function DeckService.levelUpCard(profile: any, instanceId: string): (boolean, string?, { [string]: any }?)
	local inst = profile.cards[instanceId]
	if not inst then
		return false, "Card not found.", nil
	end
	local maxLevel = 100 + (inst.ascension or 0) * 20
	if (inst.level or 1) >= maxLevel then
		return false, "Card is at max level for its current ascension.", nil
	end
	local cardDef = Config.Cards.get(inst.cardId)
	local worldTier = if cardDef and (cardDef :: any).worldId then (cardDef :: any).worldId else 1
	local cost = Economy.getLevelUpCost(inst.level or 1, worldTier)
	if (profile.yen or 0) < cost then
		return false, string.format("Need %d Yen to level up.", cost), nil
	end
	profile.yen -= cost
	inst.level = (inst.level or 1) + 1
	return true, nil, { uid = instanceId, level = inst.level, spentYen = cost }
end

function DeckService.ascendCard(profile: any, instanceId: string): (boolean, string?, { [string]: any }?)
	local inst = profile.cards[instanceId]
	if not inst then
		return false, "Card not found.", nil
	end
	local asc = inst.ascension or 0
	if asc >= 5 then
		return false, "Card is already at maximum ascension (5).", nil
	end
	local dupeUid: string? = nil
	for uid, other in pairs(profile.cards) do
		if
			uid ~= instanceId
			and other.cardId == inst.cardId
			and not other.locked
			and not isEquippedInAnyDeck(profile, uid)
		then
			dupeUid = uid
			break
		end
	end
	if not dupeUid then
		return false, "Need an unlocked, unequipped duplicate of this card to ascend.", nil
	end
	local cost = 250 * (asc + 1)
	if (profile.yen or 0) < cost then
		return false, string.format("Need %d Yen to ascend.", cost), nil
	end
	profile.yen -= cost
	profile.cards[dupeUid :: string] = nil
	inst.ascension = asc + 1
	return true, nil, { uid = instanceId, ascension = inst.ascension, consumedUid = dupeUid }
end

function DeckService.getSquadBonuses(profile: any): { [string]: any }
	local hpBonus = 0
	local atkBonus = 0
	local spdBonus = 0
	local yenBonus = 0

	local charmsCfg = (Config :: any).Charms
	if charmsCfg and type(profile.equippedCharms) == "table" then
		for _, charmId in ipairs(profile.equippedCharms) do
			local cDef = if charmsCfg.get then charmsCfg.get(charmId) else charmsCfg.ById and charmsCfg.ById[charmId]
			if cDef then
				hpBonus += cDef.hpBonus or 0
				atkBonus += cDef.atkBonus or 0
				spdBonus += cDef.spdBonus or 0
				yenBonus += cDef.yenBonus or 0
			end
		end
	end

	local rebirths = profile.rebirths or 0
	hpBonus += rebirths * 0.15
	atkBonus += rebirths * 0.15
	yenBonus += rebirths * 0.10

	local treeCfg = (Config :: any).SkillTree
	if treeCfg and type(profile.skillNodes) == "table" then
		for nodeId, rank in pairs(profile.skillNodes) do
			local nDef = if treeCfg.get then treeCfg.get(nodeId) else treeCfg.ById and treeCfg.ById[nodeId]
			local rankNum = if type(rank) == "number" then rank elseif rank == true then 1 else 0
			if nDef and rankNum > 0 then
				hpBonus += (nDef.hpBonus or 0) * rankNum
				atkBonus += (nDef.atkBonus or 0) * rankNum
				spdBonus += (nDef.spdBonus or 0) * rankNum
				yenBonus += (nDef.yenBonus or 0) * rankNum
			end
		end
	end

	local cardIds: { string } = {}
	for _, uid in ipairs(profile.activeDeck or {}) do
		local inst = profile.cards[uid]
		if inst then
			table.insert(cardIds, inst.cardId)
		end
	end
	local bondLabels: { string } = {}
	local simAny = BattleSim :: any
	if type(simAny.evaluateBonds) == "function" then
		local ok, res = pcall(simAny.evaluateBonds, cardIds)
		if ok and type(res) == "table" then
			hpBonus += res.hpBonus or 0
			atkBonus += res.atkBonus or 0
			spdBonus += res.spdBonus or 0
			if type(res.labels) == "table" then
				bondLabels = res.labels
			end
		end
	end

	local _keepRarities = Rarities
	return {
		hpMult = 1 + hpBonus,
		atkMult = 1 + atkBonus,
		spdMult = 1 + spdBonus,
		yenMult = 1 + yenBonus,
		bondLabels = bondLabels,
	}
end

return DeckService
