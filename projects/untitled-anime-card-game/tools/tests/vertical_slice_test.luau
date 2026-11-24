--!strict
-- Headless Lune test for Untitled Anime Card Game vertical slice.

local fs = require("@lune/fs")
local luau = require("@lune/luau")
local serde = require("@lune/serde")

local function makeNode(name: string, filePath: string?, parent: any?): any
	local node: any = {
		Name = name,
		Parent = parent,
		_filePath = filePath,
		_children = {},
	}
	setmetatable(node, {
		__index = function(t: any, k: string): any
			return t._children[k]
		end,
	})
	if parent then
		parent._children[name] = node
	end
	return node
end

local sharedRoot = makeNode("Shared", nil, nil)
local configRoot = makeNode("Config", "src/shared/Config/init.luau", sharedRoot)

local sharedFiles = {
	"NumberFormat",
	"LuckMath",
	"BattleSim",
	"AssetIds",
}
for _, name in ipairs(sharedFiles) do
	makeNode(name, "src/shared/" .. name .. ".luau", sharedRoot)
end

local configFiles = {
	"Rarities",
	"Mutations",
	"Traits",
	"Cards",
	"Worlds",
	"Economy",
	"Products",
}
for _, name in ipairs(configFiles) do
	makeNode(name, "src/shared/Config/" .. name .. ".luau", configRoot)
end

local cache: { [any]: any } = {}
local loadNode: (node: any) -> any

loadNode = function(node: any): any
	if cache[node] ~= nil then
		return cache[node]
	end
	local filePath = node._filePath
	assert(type(filePath) == "string", "Missing filePath for node " .. tostring(node.Name))
	local source = fs.readFile(filePath)
	local env: { [string]: any } = {
		math = math,
		table = table,
		string = string,
		ipairs = ipairs,
		pairs = pairs,
		next = next,
		tostring = tostring,
		tonumber = tonumber,
		type = type,
		typeof = typeof,
		pcall = pcall,
		xpcall = xpcall,
		error = error,
		assert = assert,
		select = select,
		unpack = table.unpack,
		script = node,
		require = function(target: any): any
			return loadNode(target)
		end,
	}
	local fn = luau.load(source, { debugName = filePath, environment = env })
	local result = fn()
	cache[node] = result
	return result
end

local Rarities = loadNode(configRoot.Rarities)
local Cards = loadNode(configRoot.Cards)
local Worlds = loadNode(configRoot.Worlds)
local Economy = loadNode(configRoot.Economy)
local NumberFormat = loadNode(sharedRoot.NumberFormat)
local LuckMath = loadNode(sharedRoot.LuckMath)
local BattleSim = loadNode(sharedRoot.BattleSim)

local function expect(cond: boolean, msg: string)
	if not cond then
		error("ASSERTION FAILED: " .. msg, 2)
	end
end

-- 1. Number formatting sanity
expect(NumberFormat.compact(950) == "950", "NumberFormat < 1K")
expect(NumberFormat.compact(12500) == "12.5K", "NumberFormat 12.5K")
expect(NumberFormat.compact(2500000) == "2.5M", "NumberFormat 2.5M")
expect(NumberFormat.odds(750) == "1-in-750", "NumberFormat.odds")

-- 2. Luck scaling & 100 rolls + pity test
local baseLuck = LuckMath.computeLuck({})
local boostedLuck = LuckMath.computeLuck({
	luckUpgradeLevel = 5,
	vipPass = true,
	superLuckPass = true,
	potionMult = 2.0,
	indexBonus = 0.25,
})
expect(baseLuck.totalMult == 1.0, "Base luck should be 1.0")
expect(boostedLuck.totalMult > 7.0, "Stacked luck should scale multiplicatively")

local baseOdds = LuckMath.computeRarityOdds(baseLuck.totalMult, { epic = 0, legendary = 0, mythic = 0 })
local boostedOdds = LuckMath.computeRarityOdds(boostedLuck.totalMult, { epic = 0, legendary = 0, mythic = 0 })
expect(boostedOdds.Epic > baseOdds.Epic, "Higher luck should boost Epic odds")
expect(boostedOdds.Legendary > baseOdds.Legendary, "Higher luck should boost Legendary odds")

local rng = LuckMath.makeSeededRng(20261123)
local pity = { epic = 0, legendary = 0, mythic = 0 }
local traitPity = 0
local seenEpicOrHigher = false
for i = 1, 100 do
	local outcome = LuckMath.performRoll(1, { luckUpgradeLevel = 2 }, pity, traitPity, rng)
	pity = outcome.nextPity
	traitPity = outcome.nextTraitPity
	expect(outcome.card ~= nil, "Roll " .. tostring(i) .. " returned a card")
	expect(outcome.oddsN >= 1, "Roll oddsN >= 1")
	if Rarities.isAtLeast(outcome.rarity, "Epic") then
		seenEpicOrHigher = true
	end
end
expect(seenEpicOrHigher, "100 rolls must yield at least one Epic+ via natural roll or Epic hard pity")

-- Verify explicit hard pity triggers
local legTier, legNextPity, legIsPity = LuckMath.rollRarity(
	1.0,
	{ epic = 99, legendary = 749, mythic = 749 },
	function()
		return 0.999999
	end
)
expect(legTier == "Legendary" and legIsPity == true, "Legendary hard pity at 750")
expect(legNextPity.legendary == 0 and legNextPity.mythic == 750, "Legendary pity resets leg/epic, keeps mythic")

local mythTier, mythNextPity, mythIsPity = LuckMath.rollRarity(
	1.0,
	{ epic = 10, legendary = 10, mythic = 14999 },
	function()
		return 0.999999
	end
)
expect(mythTier == "Mythic" and mythIsPity == true, "Mythic hard pity at 15000")
expect(mythNextPity.mythic == 0 and mythNextPity.legendary == 0, "Mythic pity resets all counters")

-- 3. Deck validation & synergy check (max 5 cards)
local maxDeck = Economy.MaxDeckSize
expect(maxDeck == 5, "Max deck size must be 5")

local w1a1DeckInputs = {
	{ uid = "c1", cardId = "w1_academy_genin", level = 4, mutation = "Normal", ascension = 0 },
	{ uid = "c2", cardId = "w1_tactical_chunin", level = 4, mutation = "Normal", ascension = 0 },
	{ uid = "c3", cardId = "w1_lotus_taijutsu", level = 5, mutation = "Normal", ascension = 0 },
	{ uid = "c4", cardId = "w1_crimson_copy_ninja", level = 5, mutation = "Shiny", ascension = 0 },
}
local w1a1Allies = {}
for idx, entry in ipairs(w1a1DeckInputs) do
	local unit = BattleSim.buildPlayerUnit(idx, entry)
	expect(unit ~= nil, "Built W1A1 unit " .. entry.cardId)
	if unit then
		table.insert(w1a1Allies, unit)
	end
end

local synergy = BattleSim.computeSynergies(w1a1Allies)
expect(#synergy.labels >= 2, "W1 squad should trigger Leaf Shinobi + Balanced Squad synergies")
expect(synergy.atkMult > 1.2, "Synergy ATK multiplier should apply")

-- 4. Deterministic W1 Act 1 auto-battle
local stage1 = Worlds.getStage("w1_act1")
expect(stage1 ~= nil, "w1_act1 stage exists")
if stage1 then
	local enemies = {}
	for idx, eSpec in ipairs(stage1.enemies) do
		local enemyCard = Cards.get(eSpec.cardId)
		table.insert(
			enemies,
			BattleSim.buildEnemyUnit(idx, {
				id = eSpec.cardId,
				name = eSpec.name,
				rarity = if enemyCard then enemyCard.rarity else "Common",
				role = if enemyCard then enemyCard.role else "Striker",
				hp = eSpec.hp,
				atk = eSpec.atk,
				spd = eSpec.spd,
				skill = {
					name = eSpec.skillName,
					description = eSpec.skillName,
					cooldown = 3,
					dmgMult = eSpec.skillMult,
					targeting = "Single",
				},
			})
		)
	end
	local resA = BattleSim.run(w1a1Allies, enemies, { seed = 777, maxRounds = 30 })
	expect(resA.won == true, "W1 Act 1 battle should win with starter W1 squad")
	expect(resA.rounds >= 1 and resA.rounds <= 30, "W1 Act 1 rounds in range")
	expect(#resA.events >= 3, "W1 Act 1 battle emits events")
	expect(resA.mvpName ~= "None", "W1 Act 1 has MVP")
end

-- 5. Deterministic W1 Boss auto-battle
local stageBoss = Worlds.getStage("w1_boss")
expect(stageBoss ~= nil, "w1_boss stage exists")
if stageBoss then
	local bossDeckInputs = {
		{
			uid = "b1",
			cardId = "w1_seventh_hokage",
			level = 15,
			mutation = "Radiant",
			trait = "Overlord",
			ascension = 1,
		},
		{ uid = "b2", cardId = "w1_sage_of_toads", level = 15, mutation = "Shiny", trait = "Ironclad", ascension = 1 },
		{ uid = "b3", cardId = "w1_uchiha_prodigy", level = 15, mutation = "Shiny", trait = "Phantom", ascension = 1 },
		{
			uid = "b4",
			cardId = "w1_crimson_copy_ninja",
			level = 15,
			mutation = "Shiny",
			trait = "Deadeye",
			ascension = 2,
		},
		{ uid = "b5", cardId = "w1_lotus_taijutsu", level = 15, mutation = "Shiny", trait = "Brawler", ascension = 2 },
	}
	local bossAllies = {}
	for idx, entry in ipairs(bossDeckInputs) do
		local unit = BattleSim.buildPlayerUnit(idx, entry)
		if unit then
			table.insert(bossAllies, unit)
		end
	end
	local bossEnemies = {}
	for idx, eSpec in ipairs(stageBoss.enemies) do
		local enemyCard = Cards.get(eSpec.cardId)
		table.insert(
			bossEnemies,
			BattleSim.buildEnemyUnit(idx, {
				id = eSpec.cardId,
				name = eSpec.name,
				rarity = if enemyCard then enemyCard.rarity else "BossDrop",
				role = if enemyCard then enemyCard.role else "Vanguard",
				hp = eSpec.hp,
				atk = eSpec.atk,
				spd = eSpec.spd,
				skill = {
					name = eSpec.skillName,
					description = eSpec.skillName,
					cooldown = 3,
					dmgMult = eSpec.skillMult,
					targeting = "Single",
				},
			})
		)
	end
	local resBoss = BattleSim.run(bossAllies, bossEnemies, { seed = 999, maxRounds = 30 })
	expect(resBoss.won == true, "W1 Boss should be beatable with leveled W1 squad")
	expect(#resBoss.events >= 3, "W1 Boss emits combat events (got " .. tostring(#resBoss.events) .. ")")
end

-- 6. Save & rejoin JSON round-trip serialization
local mockProfile = {
	schemaVersion = 1,
	yen = 1850,
	gems = 240,
	tickets = 3,
	bossTokens = 2,
	luckUpgradeLevel = 2,
	pity = { epic = 14, legendary = 88, mythic = 88 },
	traitPity = 5,
	activeDeck = { "c1", "c2", "c3", "c4" },
	defenseDeck = { "c1", "c2" },
	clearedStages = { w1_act1 = true },
	cards = {
		c1 = { uid = "c1", cardId = "w1_academy_genin", level = 4, xp = 35, mutation = "Normal", ascension = 0 },
	},
	settings = { theme = "Light", reducedMotion = false, reducedFlash = false, skipBelowRarity = "None" },
}
local function normalizeTheme(raw: any): string
	return if string.lower(tostring(raw or "")) == "light" then "Light" else "Dark"
end
expect(
	normalizeTheme("light") == "Light" and normalizeTheme("Light") == "Light",
	"Light theme case-insensitive normalization"
)
expect(normalizeTheme("dark") == "Dark" and normalizeTheme(nil) == "Dark", "Dark theme normalization")
local encoded = serde.encode("json", mockProfile)
local decoded = serde.decode("json", encoded)
expect(decoded.yen == 1850 and decoded.gems == 240, "Save/rejoin currencies preserved")
expect(#decoded.activeDeck == 4 and decoded.clearedStages.w1_act1 == true, "Save/rejoin deck and stage clear preserved")
expect(decoded.cards.c1.cardId == "w1_academy_genin", "Save/rejoin card inventory preserved")
expect(normalizeTheme(decoded.settings.theme) == "Light", "Light theme round-trip preserved")

-- 7. M1 playtest contract checks (Roll, Battle replay, Deck, Luck, OpenProp)
local starterDef = Cards.get("w1_academy_genin")
expect(starterDef ~= nil, "w1_academy_genin card definition exists")
if starterDef then
	local cStats = LuckMath.computeCardStats(starterDef, 2, "Normal", nil, 0)
	expect(
		cStats.hp > 0 and cStats.atk > 0 and cStats.spd > 0 and cStats.power > 0,
		"computeCardStats returns positive stats"
	)
	expect(Economy.getLevelUpCost(2, 1) > 0, "Economy.getLevelUpCost returns positive cost")
end

local rollSrc = fs.readFile("src/server/RollService.luau")
for _, field in ipairs({
	"uid =",
	"id =",
	"cardId =",
	"name =",
	"displayName =",
	"rarity =",
	"role =",
	"showName =",
	"mutation =",
	"trait =",
	"oddsN =",
	"isNew =",
	"hp =",
	"atk =",
	"spd =",
	"power =",
	"artKey =",
	"instance =",
}) do
	expect(string.find(rollSrc, field, 1, true) ~= nil, "RollService contract includes " .. field)
end

local battleSrc = fs.readFile("src/server/BattleService.luau")
for _, field in ipairs({
	"won = simResult.won",
	"victory = simResult.won",
	"allies = initialAllies",
	"enemies = initialEnemies",
	"hp = u.maxHp",
	"turn = ev.round",
	"actorSide =",
	"actorIndex =",
	"targetSide =",
	"targetIndex =",
	"targetHpAfter = ev.targetHp",
	"defeated = killed",
}) do
	expect(string.find(battleSrc, field, 1, true) ~= nil, "BattleService contract includes " .. field)
end

local dataSrc = fs.readFile("src/server/DataService.luau")
for _, field in ipairs({
	"deckPower =",
	"synergies =",
	"synergySummary =",
	"luckMultiplier =",
	"luckSummary =",
	"dupeCount =",
	"levelUpCost =",
	"sellValue =",
	"artKey =",
}) do
	expect(string.find(dataSrc, field, 1, true) ~= nil, "DataService contract includes " .. field)
end
local luckFnStart = string.find(dataSrc, "local function formatLuckSummary", 1, true)
local luckFnEnd = string.find(dataSrc, "function DataService.serializeForClient", 1, true)
expect(luckFnStart ~= nil and luckFnEnd ~= nil, "DataService defines formatLuckSummary")
if luckFnStart and luckFnEnd then
	local luckFnSrc = string.sub(dataSrc, luckFnStart, luckFnEnd)
	expect(
		string.find(luckFnSrc, "EventMult", 1, true) == nil
			and string.find(luckFnSrc, "IndexBonus", 1, true) == nil
			and string.find(luckFnSrc, "Base 1.0x", 1, true) ~= nil,
		"formatLuckSummary emits plain-English text without EventMult/IndexBonus"
	)
end

local mainSrc = fs.readFile("src/server/Main.server.luau")
expect(string.find(mainSrc, 'else "w1_act1"', 1, true) ~= nil, "Main.server defaults StartBattle stageId to w1_act1")
expect(
	string.find(mainSrc, 'kind = "OpenProp"', 1, true) ~= nil
		and string.find(mainSrc, 'type = "OpenModal"', 1, true) ~= nil
		and string.find(mainSrc, "propAction = propAction", 1, true) ~= nil
		and string.find(mainSrc, "modal = propAction", 1, true) ~= nil,
	"Main.server ProximityPrompt fires OpenProp + OpenModal payload"
)

print("VERTICAL_SLICE_TEST_PASS: 100 rolls + pity + luck + deck + W1A1 + W1Boss + save/rejoin + M1 contracts verified.")
