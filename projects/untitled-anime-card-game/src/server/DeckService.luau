--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Economy = require(Config:WaitForChild("Economy"))
local Cards = require(Config:WaitForChild("Cards"))
local Rarities = require(Config:WaitForChild("Rarities"))
local LuckMath = require(Shared:WaitForChild("LuckMath"))

local DeckService = {}

local function countMapKeys(tbl: { [string]: any }): number
	local count = 0
	for _ in pairs(tbl) do
		count += 1
	end
	return count
end

local function isInList(list: { string }?, targetUid: string): boolean
	if not list then
		return false
	end
	for _, uid in ipairs(list) do
		if uid == targetUid then
			return true
		end
	end
	return false
end

local function sanitizeDeckList(rawIds: { string }, cardsMap: { [string]: any }): ({ string }, string?)
	if #rawIds > Economy.MaxDeckSize then
		return {}, string.format("Deck cannot exceed %d cards.", Economy.MaxDeckSize)
	end
	local seen: { [string]: boolean } = {}
	local cleaned: { string } = {}
	for _, uid in ipairs(rawIds) do
		if type(uid) ~= "string" or uid == "" then
			return {}, "Invalid card entry in deck."
		end
		if seen[uid] then
			return {}, "Duplicate card instance in deck."
		end
		if not cardsMap[uid] then
			return {}, "You do not own all selected cards."
		end
		seen[uid] = true
		table.insert(cleaned, uid)
	end
	return cleaned, nil
end

function DeckService.setDeck(
	profile: { [string]: any },
	deckIds: { string }?,
	defenseDeckIds: { string }?
): (boolean, string?)
	local cardsMap: { [string]: any } = profile.cards or {}

	if deckIds ~= nil then
		if type(deckIds) ~= "table" then
			return false, "Invalid deck list."
		end
		local cleaned, err = sanitizeDeckList(deckIds, cardsMap)
		if err then
			return false, err
		end
		profile.activeDeck = cleaned
	end

	if defenseDeckIds ~= nil then
		if type(defenseDeckIds) ~= "table" then
			return false, "Invalid defense deck list."
		end
		local cleanedDef, defErr = sanitizeDeckList(defenseDeckIds, cardsMap)
		if defErr then
			return false, defErr
		end
		profile.defenseDeck = cleanedDef
	end

	return true, nil
end

function DeckService.toggleLock(profile: { [string]: any }, instanceId: string): (boolean, string?)
	local cardsMap: { [string]: any } = profile.cards or {}
	local card = cardsMap[instanceId]
	if not card then
		return false, "Card not found."
	end
	card.locked = not (card.locked == true)
	return true, nil
end

function DeckService.levelUpCard(profile: { [string]: any }, instanceId: string): (boolean, string?)
	local cardsMap: { [string]: any } = profile.cards or {}
	local card = cardsMap[instanceId]
	if not card then
		return false, "Card not found."
	end
	local currentLevel = card.level or 1
	if currentLevel >= Economy.MaxLevel then
		return false, "Card is already at max level."
	end
	local cardDef = Cards.get(card.cardId)
	local worldTier = if cardDef and (cardDef :: any).worldId then (cardDef :: any).worldId else 1
	local cost = Economy.getLevelUpCost(currentLevel, worldTier)
	local yen = profile.yen or 0
	if yen < cost then
		return false, string.format("Need %d Yen to level up.", cost)
	end
	profile.yen = yen - cost
	card.level = currentLevel + 1
	return true, nil
end

function DeckService.ascendCard(profile: { [string]: any }, instanceId: string): (boolean, string?)
	local cardsMap: { [string]: any } = profile.cards or {}
	local target = cardsMap[instanceId]
	if not target then
		return false, "Card not found."
	end
	local currentAsc = target.ascension or 0
	if currentAsc >= Economy.MaxStars then
		return false, "Card is already at max ascension."
	end
	local nextAsc = currentAsc + 1
	local dupeCost = Economy.AscensionDupeCosts[nextAsc] or 1

	local fodderUids: { string } = {}
	for uid, candidate in pairs(cardsMap) do
		if
			uid ~= target.uid
			and candidate.cardId == target.cardId
			and not candidate.locked
			and not isInList(profile.activeDeck, uid)
			and not isInList(profile.defenseDeck, uid)
		then
			table.insert(fodderUids, uid)
			if #fodderUids >= dupeCost then
				break
			end
		end
	end

	if #fodderUids < dupeCost then
		return false, string.format("Need %d unlocked duplicate(s) to ascend.", dupeCost)
	end

	for _, uid in ipairs(fodderUids) do
		cardsMap[uid] = nil
	end
	target.ascension = nextAsc
	return true, nil
end

function DeckService.getSellValue(card: { [string]: any }): number
	local cardDef = Cards.get(card.cardId)
	local rarityId = if cardDef then cardDef.rarity else "Common"
	local rarityDef = Rarities.ById[rarityId] :: any
	local base = if rarityDef and type(rarityDef.sellValue) == "number" then rarityDef.sellValue else 15
	return math.max(5, math.floor(base * (1 + ((card.level or 1) - 1) * 0.15)))
end

function DeckService.sellCard(profile: { [string]: any }, instanceId: string): (boolean, string?, number?)
	local cardsMap: { [string]: any } = profile.cards or {}
	local card = cardsMap[instanceId]
	if not card then
		return false, "Card not found.", nil
	end
	if card.locked then
		return false, "Unlock this card before selling.", nil
	end
	if isInList(profile.activeDeck, instanceId) or isInList(profile.defenseDeck, instanceId) then
		return false, "Unequip this card from your deck before selling.", nil
	end
	if countMapKeys(cardsMap) <= 1 then
		return false, "Cannot sell your last remaining card.", nil
	end
	local value = DeckService.getSellValue(card)
	cardsMap[instanceId] = nil
	profile.yen = (profile.yen or 0) + value
	return true, nil, value
end

function DeckService.quickSell(profile: { [string]: any }, instanceIds: { string }): (boolean, string?, number?)
	if type(instanceIds) ~= "table" then
		return false, "Invalid card list.", nil
	end
	local cardsMap: { [string]: any } = profile.cards or {}
	local remaining = countMapKeys(cardsMap)
	local soldCount = 0
	local totalYen = 0

	for _, uid in ipairs(instanceIds) do
		if remaining <= 1 then
			break
		end
		local card = if type(uid) == "string" then cardsMap[uid] else nil
		if
			card
			and not card.locked
			and not isInList(profile.activeDeck, uid)
			and not isInList(profile.defenseDeck, uid)
		then
			totalYen += DeckService.getSellValue(card)
			cardsMap[uid] = nil
			soldCount += 1
			remaining -= 1
		end
	end

	if soldCount == 0 then
		return false, "No eligible unlocked cards to sell.", nil
	end
	profile.yen = (profile.yen or 0) + totalYen
	return true, nil, totalYen
end

function DeckService.rerollTrait(profile: { [string]: any }, instanceId: string): (boolean, string?)
	local cardsMap: { [string]: any } = profile.cards or {}
	local card = cardsMap[instanceId]
	if not card then
		return false, "Card not found."
	end
	local crystals = profile.traitCrystals or 0
	if crystals < 1 then
		return false, "Need 1 Trait Crystal to reroll."
	end
	profile.traitCrystals = crystals - 1
	local newTrait, nextPity = LuckMath.rollTrait(profile.traitPity or 0)
	profile.traitPity = nextPity
	card.trait = newTrait
	return true, nil
end

return DeckService
