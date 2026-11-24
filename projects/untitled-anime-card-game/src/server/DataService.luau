--!strict
local DataStoreService = game:GetService("DataStoreService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = require(Shared:WaitForChild("Config"))
local ConfigFolder = Shared:WaitForChild("Config")
local Economy = require(ConfigFolder:WaitForChild("Economy"))
local LuckMath = require(Shared:WaitForChild("LuckMath"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DeckService = require(script.Parent:WaitForChild("DeckService"))

export type CardInstance = {
	uid: string,
	cardId: string,
	level: number,
	xp: number,
	mutation: string,
	trait: string?,
	ascension: number,
	locked: boolean,
	obtainedAt: number,
}

export type OwnedCard = CardInstance

export type ProfileSettings = {
	theme: string,
	reducedMotion: boolean,
	reducedFlash: boolean,
	skipBelowRarity: string,
}

export type PlayerProfile = {
	schemaVersion: number,
	yen: number,
	gems: number,
	tickets: number,
	bossTokens: number,
	raidTokens: number,
	traitCrystals: number,
	rebirths: number,
	luckUpgradeLevel: number,
	currentWorld: number,
	selectedWorld: number,
	selectedBannerId: string,
	unlockedWorlds: { [number]: boolean },
	pity: LuckMath.PityState,
	traitPity: number,
	rollCount: number,
	nextCardSeq: number,
	cards: { [string]: CardInstance },
	activeDeck: { string },
	defenseDeck: { string },
	clearedStages: { [string]: boolean },
	discoveredCards: { [string]: boolean },
	indexCount: number,
	potions: { [string]: number },
	luckPotionExpiresAt: number,
	luckPotionBonus: number,
	redeemedCodes: { [string]: boolean },
	lastDailyClaim: number,
	dailyStreak: number,
	loginStreak: number,
	lastLoginDay: number,
	playtimeSeconds: number,
	claimedPlaytime: { [string]: boolean },
	towerFloor: number,
	materials: { [string]: number },
	charms: { [string]: number },
	equippedCharms: { string },
	skillNodes: { [string]: number },
	elo: number,
	rankedWins: number,
	rankedLosses: number,
	rankedRewardClaimed: { [string]: boolean },
	quests: { [string]: any },
	processedReceipts: { [string]: boolean },
	ownedPasses: { [string]: boolean },
	settings: ProfileSettings,
	savedAt: number,
}

local DataService = {}

local DATASTORE_NAME = "UACG_PlayerProfiles_v1"
local AUTOSAVE_INTERVAL = 60

local profiles: { [number]: PlayerProfile } = {}
local memoryFallback: { [string]: PlayerProfile } = {}
local savingPlayers: { [number]: boolean } = {}
local dataStore: DataStore? = nil
local useMemoryOnly = false
local initialized = false
local onProfileReadyCallback: ((Player, PlayerProfile) -> ())? = nil

local function deepCopy(val: any): any
	if type(val) ~= "table" then
		return val
	end
	local out = {}
	for k, v in pairs(val) do
		out[k] = deepCopy(v)
	end
	return out
end

local function countKeys(tbl: { [any]: any }): number
	local n = 0
	for _ in pairs(tbl) do
		n += 1
	end
	return n
end

local function createDefaultProfile(): PlayerProfile
	local starterCardId = "w1_academy_genin"
	local starterUid = "c_1"
	local now = os.time()

	return {
		schemaVersion = 2,
		yen = 500,
		gems = 150,
		tickets = 3,
		bossTokens = 0,
		raidTokens = 0,
		traitCrystals = 1,
		rebirths = 0,
		luckUpgradeLevel = 0,
		currentWorld = 1,
		selectedWorld = 1,
		selectedBannerId = "world_1",
		unlockedWorlds = { [1] = true },
		pity = { epic = 0, legendary = 0, mythic = 0 },
		traitPity = 0,
		rollCount = 0,
		nextCardSeq = 2,
		cards = {
			[starterUid] = {
				uid = starterUid,
				cardId = starterCardId,
				level = 1,
				xp = 0,
				mutation = "Normal",
				trait = nil,
				ascension = 0,
				locked = false,
				obtainedAt = now,
			},
		},
		activeDeck = { starterUid },
		defenseDeck = { starterUid },
		clearedStages = {},
		discoveredCards = { [starterCardId] = true },
		indexCount = 1,
		potions = { luck_potion_1 = 1 },
		luckPotionExpiresAt = 0,
		luckPotionBonus = 0,
		redeemedCodes = {},
		lastDailyClaim = 0,
		dailyStreak = 0,
		loginStreak = 0,
		lastLoginDay = 0,
		playtimeSeconds = 0,
		claimedPlaytime = {},
		towerFloor = 1,
		materials = {},
		charms = {},
		equippedCharms = {},
		skillNodes = {},
		elo = 1000,
		rankedWins = 0,
		rankedLosses = 0,
		rankedRewardClaimed = {},
		quests = { daily = {}, weekly = {}, dailyDay = 0, weeklyWeek = 0 },
		processedReceipts = {},
		ownedPasses = {},
		settings = {
			theme = "Dark",
			reducedMotion = false,
			reducedFlash = false,
			skipBelowRarity = "None",
		},
		savedAt = now,
	}
end

local function reconcileProfile(raw: any): PlayerProfile
	local defaults = createDefaultProfile()
	if type(raw) ~= "table" then
		return defaults
	end
	local p = deepCopy(raw) :: PlayerProfile
	p.schemaVersion = 2
	p.yen = if type(p.yen) == "number" then math.max(0, math.floor(p.yen)) else defaults.yen
	p.gems = if type(p.gems) == "number" then math.max(0, math.floor(p.gems)) else defaults.gems
	p.tickets = if type(p.tickets) == "number" then math.max(0, math.floor(p.tickets)) else defaults.tickets
	p.bossTokens = if type(p.bossTokens) == "number" then math.max(0, math.floor(p.bossTokens)) else 0
	p.raidTokens = if type(p.raidTokens) == "number" then math.max(0, math.floor(p.raidTokens)) else 0
	p.traitCrystals = if type(p.traitCrystals) == "number" then math.max(0, math.floor(p.traitCrystals)) else 1
	p.rebirths = if type(p.rebirths) == "number" then math.max(0, math.floor(p.rebirths)) else 0
	p.luckUpgradeLevel = if type(p.luckUpgradeLevel) == "number" then math.max(0, math.floor(p.luckUpgradeLevel)) else 0
	p.currentWorld = if type(p.currentWorld) == "number" then math.max(1, math.floor(p.currentWorld)) else 1
	p.selectedWorld = if type(p.selectedWorld) == "number"
		then math.max(1, math.floor(p.selectedWorld))
		else p.currentWorld
	p.selectedBannerId = if type(p.selectedBannerId) == "string" and p.selectedBannerId ~= ""
		then p.selectedBannerId
		else ("world_" .. tostring(p.selectedWorld))
	p.unlockedWorlds = if type(p.unlockedWorlds) == "table" then p.unlockedWorlds else { [1] = true }
	p.unlockedWorlds[1] = true

	if type(p.pity) ~= "table" then
		p.pity = { epic = 0, legendary = 0, mythic = 0 }
	else
		p.pity.epic = p.pity.epic or 0
		p.pity.legendary = p.pity.legendary or 0
		p.pity.mythic = p.pity.mythic or 0
	end
	p.traitPity = p.traitPity or 0
	p.rollCount = p.rollCount or 0
	p.nextCardSeq = p.nextCardSeq or 2
	p.cards = if type(p.cards) == "table" then p.cards else defaults.cards
	p.activeDeck = if type(p.activeDeck) == "table" then p.activeDeck else {}
	p.defenseDeck = if type(p.defenseDeck) == "table" then p.defenseDeck else {}
	p.clearedStages = if type(p.clearedStages) == "table" then p.clearedStages else {}
	p.discoveredCards = if type(p.discoveredCards) == "table" then p.discoveredCards else {}
	p.indexCount = countKeys(p.discoveredCards)
	p.potions = if type(p.potions) == "table" then p.potions else {}
	p.luckPotionExpiresAt = p.luckPotionExpiresAt or 0
	p.luckPotionBonus = p.luckPotionBonus or 0
	p.redeemedCodes = if type(p.redeemedCodes) == "table" then p.redeemedCodes else {}
	p.lastDailyClaim = p.lastDailyClaim or 0
	p.dailyStreak = p.dailyStreak or 0
	p.loginStreak = if type(p.loginStreak) == "number" then p.loginStreak else p.dailyStreak
	p.lastLoginDay = p.lastLoginDay or 0
	p.playtimeSeconds = p.playtimeSeconds or 0
	p.claimedPlaytime = if type(p.claimedPlaytime) == "table" then p.claimedPlaytime else {}
	p.towerFloor = if type(p.towerFloor) == "number" then math.max(1, math.floor(p.towerFloor)) else 1
	p.materials = if type(p.materials) == "table" then p.materials else {}
	p.charms = if type(p.charms) == "table" then p.charms else {}
	p.equippedCharms = if type(p.equippedCharms) == "table" then p.equippedCharms else {}
	p.skillNodes = if type(p.skillNodes) == "table" then p.skillNodes else {}
	p.elo = if type(p.elo) == "number" then math.max(0, math.floor(p.elo)) else 1000
	p.rankedWins = p.rankedWins or 0
	p.rankedLosses = p.rankedLosses or 0
	p.rankedRewardClaimed = if type(p.rankedRewardClaimed) == "table" then p.rankedRewardClaimed else {}
	p.quests = if type(p.quests) == "table" then p.quests else defaults.quests
	p.processedReceipts = if type(p.processedReceipts) == "table" then p.processedReceipts else {}
	p.ownedPasses = if type(p.ownedPasses) == "table" then p.ownedPasses else {}

	if type(p.settings) ~= "table" then
		p.settings = defaults.settings
	else
		local themeLower = string.lower(tostring(p.settings.theme or ""))
		p.settings.theme = if themeLower == "light" then "Light" else "Dark"
		p.settings.reducedMotion = p.settings.reducedMotion == true
		p.settings.reducedFlash = p.settings.reducedFlash == true
		p.settings.skipBelowRarity = tostring(p.settings.skipBelowRarity or "None")
	end

	return p
end

local function getStoreKey(userId: number): string
	return "player_" .. tostring(userId)
end

local function initDataStore()
	if RunService:IsStudio() and game.GameId == 0 then
		useMemoryOnly = true
		return
	end
	local ok, storeOrErr = pcall(function()
		return DataStoreService:GetDataStore(DATASTORE_NAME)
	end)
	if ok and storeOrErr then
		dataStore = storeOrErr
	else
		useMemoryOnly = true
	end
end

function DataService.loadProfile(player: Player): PlayerProfile
	local userId = player.UserId
	if profiles[userId] then
		return profiles[userId]
	end
	local key = getStoreKey(userId)
	local loaded: any = nil
	if not useMemoryOnly and dataStore then
		local ok, result = pcall(function()
			return (dataStore :: DataStore):GetAsync(key)
		end)
		if ok then
			loaded = result
		else
			useMemoryOnly = true
		end
	end
	if loaded == nil and memoryFallback[key] ~= nil then
		loaded = memoryFallback[key]
	end
	local profile = reconcileProfile(loaded)
	profiles[userId] = profile
	memoryFallback[key] = deepCopy(profile)
	return profile
end

function DataService.saveProfile(player: Player): boolean
	local userId = player.UserId
	local profile = profiles[userId]
	if not profile or savingPlayers[userId] then
		return false
	end
	savingPlayers[userId] = true
	profile.savedAt = os.time()
	profile.indexCount = countKeys(profile.discoveredCards)
	local key = getStoreKey(userId)
	local snapshot = deepCopy(profile)
	memoryFallback[key] = snapshot
	local saved = true
	if not useMemoryOnly and dataStore then
		local ok = pcall(function()
			(dataStore :: DataStore):UpdateAsync(key, function(_oldData: any)
				return snapshot
			end)
		end)
		if not ok then
			saved = false
		end
	end
	savingPlayers[userId] = nil
	return saved
end

function DataService.getProfile(player: Player): PlayerProfile?
	return profiles[player.UserId]
end

function DataService.getOrLoadProfile(player: Player): PlayerProfile
	return profiles[player.UserId] or DataService.loadProfile(player)
end

function DataService.nextUid(profile: PlayerProfile): string
	local seq = profile.nextCardSeq or 1
	profile.nextCardSeq = seq + 1
	return "c_" .. tostring(seq)
end

function DataService.addCard(profile: PlayerProfile, cardId: string, mutation: string?, trait: string?): CardInstance
	local uid = DataService.nextUid(profile)
	local inst: CardInstance = {
		uid = uid,
		cardId = cardId,
		level = 1,
		xp = 0,
		mutation = mutation or "Normal",
		trait = trait,
		ascension = 0,
		locked = false,
		obtainedAt = os.time(),
	}
	profile.cards[uid] = inst
	profile.discoveredCards[cardId] = true
	profile.indexCount = countKeys(profile.discoveredCards)
	return inst
end

function DataService.getLuckBreakdown(profile: PlayerProfile): LuckMath.LuckBreakdown
	local discovered = countKeys(profile.discoveredCards)
	local totalCards = math.max(1, #Config.Cards.List)
	local indexBonus = math.min(0.5, (discovered / totalCards) * 0.5)
	local now = os.time()
	local potionBonus = if profile.luckPotionExpiresAt > now then profile.luckPotionBonus else 0
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
	return LuckMath.computeLuck({
		luckUpgradeLevel = profile.luckUpgradeLevel,
		indexBonus = indexBonus,
		streakBonus = math.min(0.25, (profile.loginStreak or profile.dailyStreak or 0) * 0.05),
		potionBonus = potionBonus,
		charmBonus = charmBonus,
		skillTreeBonus = skillTreeBonus,
		rebirthBonus = rebirthBonus,
		eventMult = 1 + rebirthBonus,
	})
end

function DataService.computeDeckSynergies(profile: PlayerProfile): { string }
	local units: { BattleSim.CombatUnit } = {}
	local cardIds: { string } = {}
	for slot, uid in ipairs(profile.activeDeck) do
		local inst = profile.cards[uid]
		if inst then
			table.insert(cardIds, inst.cardId)
			local unit = BattleSim.buildPlayerUnit(slot, inst)
			if unit then
				table.insert(units, unit)
			end
		end
	end
	local labels = BattleSim.computeSynergies(units).labels or {}
	local simAny = BattleSim :: any
	if type(simAny.evaluateBonds) == "function" then
		local ok, bondsRes = pcall(simAny.evaluateBonds, cardIds)
		if ok and type(bondsRes) == "table" and type(bondsRes.labels) == "table" then
			for _, lbl in ipairs(bondsRes.labels) do
				if not table.find(labels, lbl) then
					table.insert(labels, lbl)
				end
			end
		end
	end
	return labels
end

local function formatLuckSummary(profile: PlayerProfile, luck: LuckMath.LuckBreakdown): string
	local lmAny = LuckMath :: any
	if type(lmAny.luckSummary) == "function" then
		local ok, res = pcall(lmAny.luckSummary, luck)
		if ok and type(res) == "string" and res ~= "" then
			return res
		end
	end
	local parts: { string } = {}
	if luck.upgradeBonus > 0 then
		table.insert(parts, string.format("Upgrades +%d%%", math.round(luck.upgradeBonus * 100)))
	end
	if luck.indexBonus > 0 then
		table.insert(parts, string.format("Index +%d%%", math.round(luck.indexBonus * 100)))
	end
	if luck.streakBonus > 0 then
		table.insert(parts, string.format("Streak +%d%%", math.round(luck.streakBonus * 100)))
	end
	if (profile.luckPotionExpiresAt or 0) > os.time() and (profile.luckPotionBonus or 0) > 0 then
		table.insert(parts, string.format("Potion +%d%%", math.round(profile.luckPotionBonus * 100)))
	end
	if luck.eventMult > 1.001 then
		table.insert(parts, string.format("Rebirth %.2gx", luck.eventMult))
	end
	if #parts == 0 then
		return if luck.totalMult > 1.001 then string.format("Total %.2gx", luck.totalMult) else "Base 1.0x"
	end
	return table.concat(parts, " • ")
end

function DataService.serializeForClient(profile: PlayerProfile): { [string]: any }
	local luck = DataService.getLuckBreakdown(profile)
	local odds = LuckMath.computeRarityOdds(luck.totalMult, profile.pity)
	local formattedOdds: { [string]: number } = {}
	for rarityId, prob in pairs(odds) do
		formattedOdds[rarityId] = LuckMath.formatOddsN(prob)
	end

	local unlockedDupesByCardId: { [string]: number } = {}
	for _, inst in pairs(profile.cards) do
		if not inst.locked then
			unlockedDupesByCardId[inst.cardId] = (unlockedDupesByCardId[inst.cardId] or 0) + 1
		end
	end

	local enrichedCards: { [string]: any } = {}
	for uid, inst in pairs(profile.cards) do
		local cardDef = Config.Cards.get(inst.cardId)
		local level = inst.level or 1
		local mutation = inst.mutation or "Normal"
		local ascension = inst.ascension or 0
		local stats = if cardDef
			then LuckMath.computeCardStats(cardDef, level, mutation, inst.trait, ascension)
			else { hp = 100, atk = 20, spd = 10, power = 130 }
		local worldTier = if cardDef and (cardDef :: any).worldId then (cardDef :: any).worldId else 1
		local totalUnlocked = unlockedDupesByCardId[inst.cardId] or 0
		local otherUnlocked = if inst.locked then totalUnlocked else math.max(0, totalUnlocked - 1)
		local displayName = if cardDef then ((cardDef :: any).name or cardDef.displayName) else inst.cardId

		local entry = deepCopy(inst)
		entry.id = inst.cardId
		entry.name = displayName
		entry.displayName = displayName
		entry.rarity = if cardDef then cardDef.rarity else "Common"
		entry.role = if cardDef then cardDef.role else "Striker"
		entry.showName = if cardDef then cardDef.showName else "Unknown"
		entry.hp = stats.hp
		entry.atk = stats.atk
		entry.spd = stats.spd
		entry.power = stats.power
		entry.levelUpCost = Economy.getLevelUpCost(level, worldTier)
		entry.sellValue = DeckService.getSellValue(inst)
		entry.dupeCount = otherUnlocked
		entry.artKey = if cardDef and cardDef.artKey then cardDef.artKey else ("cards/" .. inst.cardId)
		enrichedCards[uid] = entry
	end

	local deckPower = 0
	for _, uid in ipairs(profile.activeDeck) do
		local cardEntry = enrichedCards[uid]
		if cardEntry and type(cardEntry.power) == "number" then
			deckPower += cardEntry.power
		end
	end

	local synergies = DataService.computeDeckSynergies(profile)
	local synergySummary = if #synergies > 0
		then table.concat(synergies, " • ")
		else "Equip 2+ cards from the same show or bond for synergy bonuses"

	local worldDef = Config.Worlds.get(profile.selectedWorld or profile.currentWorld) or Config.Worlds.List[1]
	local rollCost = if worldDef then worldDef.rollCostYen else 100
	local discoveredCount = countKeys(profile.discoveredCards)
	profile.indexCount = discoveredCount
	local cardCount = countKeys(profile.cards)

	local normalizedSettings = deepCopy(profile.settings)
	local themeLower = string.lower(tostring(normalizedSettings.theme or ""))
	normalizedSettings.theme = if themeLower == "light" then "Light" else "Dark"

	return {
		schemaVersion = profile.schemaVersion,
		yen = profile.yen,
		gems = profile.gems,
		tickets = profile.tickets,
		bossTokens = profile.bossTokens,
		raidTokens = profile.raidTokens,
		traitCrystals = profile.traitCrystals,
		rebirths = profile.rebirths,
		luckUpgradeLevel = profile.luckUpgradeLevel,
		currentWorld = profile.currentWorld,
		selectedWorld = profile.selectedWorld,
		selectedBannerId = profile.selectedBannerId,
		unlockedWorlds = deepCopy(profile.unlockedWorlds),
		pity = deepCopy(profile.pity),
		traitPity = profile.traitPity,
		rollCount = profile.rollCount,
		rollCostYen = rollCost,
		cards = enrichedCards,
		cardCount = cardCount,
		discoveredCount = discoveredCount,
		indexCount = discoveredCount,
		activeDeck = deepCopy(profile.activeDeck),
		defenseDeck = deepCopy(profile.defenseDeck),
		deckPower = deckPower,
		clearedStages = deepCopy(profile.clearedStages),
		discoveredCards = deepCopy(profile.discoveredCards),
		potions = deepCopy(profile.potions),
		luckPotionExpiresAt = profile.luckPotionExpiresAt,
		lastDailyClaim = profile.lastDailyClaim,
		dailyStreak = profile.dailyStreak,
		loginStreak = profile.loginStreak,
		towerFloor = profile.towerFloor,
		materials = deepCopy(profile.materials),
		charms = deepCopy(profile.charms),
		equippedCharms = deepCopy(profile.equippedCharms),
		skillNodes = deepCopy(profile.skillNodes),
		elo = profile.elo,
		rankedWins = profile.rankedWins,
		rankedLosses = profile.rankedLosses,
		ownedPasses = deepCopy(profile.ownedPasses),
		settings = normalizedSettings,
		luckBreakdown = luck,
		luckMultiplier = luck.totalMult,
		luckSummary = formatLuckSummary(profile, luck),
		oddsN = formattedOdds,
		synergies = synergies,
		synergySummary = synergySummary,
	}
end

function DataService.onProfileReady(cb: (Player, PlayerProfile) -> ())
	onProfileReadyCallback = cb
end

function DataService.init()
	if initialized then
		return
	end
	initialized = true
	initDataStore()

	local function handlePlayerAdded(player: Player)
		local profile = DataService.loadProfile(player)
		if onProfileReadyCallback then
			onProfileReadyCallback(player, profile)
		end
	end

	Players.PlayerAdded:Connect(handlePlayerAdded)
	Players.PlayerRemoving:Connect(function(player: Player)
		DataService.saveProfile(player)
		profiles[player.UserId] = nil
	end)

	for _, player in ipairs(Players:GetPlayers()) do
		task.spawn(handlePlayerAdded, player)
	end

	task.spawn(function()
		while true do
			task.wait(AUTOSAVE_INTERVAL)
			for _, player in ipairs(Players:GetPlayers()) do
				task.spawn(DataService.saveProfile, player)
			end
		end
	end)

	game:BindToClose(function()
		if useMemoryOnly then
			return
		end
		for _, player in ipairs(Players:GetPlayers()) do
			pcall(DataService.saveProfile, player)
		end
	end)
end

return DataService
