--!strict
-- Headless Lune verification suite for UACG Milestone 2 (all 54 cards + 14 systems).

local fs = require("@lune/fs")
local luau = require("@lune/luau")
local task = require("@lune/task")

local function makeNode(name: string, filePath: string?, parent: any?): any
	local node: any = {
		Name = name,
		Parent = parent,
		_filePath = filePath,
		_children = {},
	}
	function node:WaitForChild(childName: string): any
		return self._children[childName]
	end
	function node:FindFirstChild(childName: string): any
		return self._children[childName]
	end
	function node:IsA(className: string): boolean
		if className == "ModuleScript" then
			return self._filePath ~= nil
		end
		return false
	end
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

local replicatedStorage = makeNode("ReplicatedStorage", nil, nil)
local sharedRoot = makeNode("Shared", nil, replicatedStorage)
local configRoot = makeNode("Config", "src/shared/Config/init.luau", sharedRoot)
local serverRoot = makeNode("Server", nil, nil)

for _, entry in ipairs(fs.readDir("src/shared")) do
	if string.sub(entry, -5) == ".luau" then
		local name = string.sub(entry, 1, #entry - 5)
		makeNode(name, "src/shared/" .. entry, sharedRoot)
	end
end

for _, entry in ipairs(fs.readDir("src/shared/Config")) do
	if string.sub(entry, -5) == ".luau" and entry ~= "init.luau" then
		local name = string.sub(entry, 1, #entry - 5)
		makeNode(name, "src/shared/Config/" .. entry, configRoot)
	end
end

for _, entry in ipairs(fs.readDir("src/server")) do
	if string.sub(entry, -5) == ".luau" and entry ~= "Main.server.luau" and entry ~= "WorldBuild.luau" then
		local name = string.sub(entry, 1, #entry - 5)
		makeNode(name, "src/server/" .. entry, serverRoot)
	end
end

local function makeSignal()
	return {
		Connect = function()
			return { Disconnect = function() end }
		end,
	}
end

local mockServices: { [string]: any } = {
	ReplicatedStorage = replicatedStorage,
	RunService = {
		IsStudio = function()
			return true
		end,
		IsServer = function()
			return true
		end,
		Heartbeat = makeSignal(),
	},
	DataStoreService = {
		GetOrderedDataStore = function()
			error("mock datastore")
		end,
		GetDataStore = function()
			error("mock datastore")
		end,
	},
	MarketplaceService = { ProcessReceipt = nil },
	Players = {
		GetPlayers = function()
			return {}
		end,
		PlayerAdded = makeSignal(),
		PlayerRemoving = makeSignal(),
	},
	BadgeService = { AwardBadge = function() end },
	AnalyticsService = {
		LogOnboardingFunnelStepEvent = function() end,
		LogCustomEvent = function() end,
	},
	HttpService = {
		GenerateGUID = function()
			return "guid-" .. tostring(math.random(10000, 99999))
		end,
	},
}

local mockGame: any = {
	GameId = 0,
	BindToClose = function() end,
	GetService = function(_, name: string): any
		if mockServices[name] == nil then
			mockServices[name] = {}
		end
		return mockServices[name]
	end,
}

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
		os = os,
		task = task,
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
		warn = function() end,
		Enum = { MembershipType = { Premium = "Premium", None = "None" } },
		Random = {
			new = function(_seed: number?)
				return {
					NextNumber = function()
						return math.random()
					end,
					NextInteger = function(_, a: number, b: number)
						return math.random(a, b)
					end,
				}
			end,
		},
		game = mockGame,
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

local function expect(cond: boolean, msg: string)
	if not cond then
		error("ASSERTION FAILED: " .. msg, 2)
	end
end

-- Load Shared & Config modules
local Cards = loadNode(configRoot.Cards)
local Worlds = loadNode(configRoot.Worlds)
local Bonds = loadNode(configRoot.Bonds)
local Products = loadNode(configRoot.Products)
local LuckMath = loadNode(sharedRoot.LuckMath)
local BattleSim = loadNode(sharedRoot.BattleSim)

-- Load Server services
local DataService = loadNode(serverRoot.DataService)
local TowerService = loadNode(serverRoot.TowerService)
local RaidService = loadNode(serverRoot.RaidService)
local WorldBossService = loadNode(serverRoot.WorldBossService)
local RankedService = loadNode(serverRoot.RankedService)
local RebirthService = loadNode(serverRoot.RebirthService)
local CharmService = loadNode(serverRoot.CharmService)
local RetentionService = loadNode(serverRoot.RetentionService)
local WeatherService = loadNode(serverRoot.WeatherService)
local MonetizationService = loadNode(serverRoot.MonetizationService)

-- 1. Cards: 54 cards across W1-W5 (45 banner + 9 non-banner exclusives/bosses) + art/cards/<id>/ dirs
expect(#Cards.List == 54, "Cards.List must have 54 cards, got " .. tostring(#Cards.List))
local seenIds: { [string]: boolean } = {}
local bannerCount = 0
local exclCount = 0
for _, c in ipairs(Cards.List) do
	expect(not seenIds[c.id], "Duplicate card id: " .. c.id)
	seenIds[c.id] = true
	expect(fs.isDir("art/cards/" .. c.id), "Missing art dir for card: " .. c.id)
	if c.bannerPool then
		bannerCount += 1
	else
		exclCount += 1
	end
end
expect(bannerCount == 45, "Expected 45 banner cards, got " .. tostring(bannerCount))
expect(exclCount == 9, "Expected 9 non-banner exclusive/boss cards, got " .. tostring(exclCount))

-- 2. Worlds: 5 worlds x 6 stages (5 acts + 1 world boss = 30 stages) + Tower + 2 Raids
expect(#Worlds.List == 5, "Worlds.List must have 5 worlds")
local stageCount = 0
for _, w in ipairs(Worlds.List) do
	expect(#w.stages == 6, "World " .. tostring(w.id) .. " must have 6 stages")
	stageCount += #w.stages
end
expect(stageCount == 30, "Total world stages must be 30")
expect(TowerService.MAX_FLOOR == 100, "TowerService.MAX_FLOOR must be 100")

-- 3. Bonds: 5 Show Bonds + 5 Cross-Card Bonds + BattleSim.evaluateBonds
local bondList = Bonds.List or Bonds
local showBonds, crossBonds = 0, 0
for _, b in ipairs(bondList) do
	if b.kind == "Show" or b.show or b.requiredShow then
		showBonds += 1
	elseif b.kind == "CrossCard" or b.requiredCards or b.requiredCardIds then
		crossBonds += 1
	end
end
expect(showBonds == 5, "Expected 5 Show Bonds, got " .. tostring(showBonds))
expect(crossBonds == 5, "Expected 5 Cross-Card Bonds, got " .. tostring(crossBonds))

local evalShow = BattleSim.evaluateBonds({ "w1_academy_genin", "w1_tactical_chunin" })
expect(#evalShow.activeBonds >= 1, "BattleSim.evaluateBonds should trigger Show bond")

local evalCross = BattleSim.evaluateBonds({ "w1_seventh_hokage", "w1_rinnegan_shadow", "w1_crimson_copy_ninja" })
expect(#evalCross.activeBonds >= 2, "BattleSim.evaluateBonds should trigger both Show and Cross-Card bonds")

-- 4. LuckMath: stacked luck includes weather, skill tree, rebirth, charms, index, passes, Premium, Group
local baseLuck = LuckMath.computeLuck({})
local stackedLuck = LuckMath.computeLuck({
	luckUpgradeLevel = 5,
	vipPass = true,
	superLuckPass = true,
	potionMult = 2.0,
	indexBonus = 0.25,
	weatherBonus = 0.50,
	skillTreeLuckBonus = 0.40,
	rebirthLuckBonus = 0.30,
	charmLuckBonus = 0.20,
	isPremium = true,
	inGroup = true,
})
expect(stackedLuck.totalMult > baseLuck.totalMult * 5, "Stacked M2 luck should scale strongly")

-- 5. DataService v2 profile + addCard + serializeForClient
local mockPlayer: any = { UserId = 999001, Name = "TestShinobi" }
local profile = DataService.loadProfile(mockPlayer)
expect(profile.schemaVersion >= 2, "DataService.loadProfile returns v2 profile")
local c1 = DataService.addCard(profile, "w1_seventh_hokage", "Holo", nil)
local c2 = DataService.addCard(profile, "w5_true_bankai_substitute", "Cosmic", nil)
local c3 = DataService.addCard(profile, "w4_honored_one", "Rainbow", nil)
c1.level = 35
c2.level = 45
c3.level = 40
profile.yen = 50000
profile.gems = 10000
profile.activeDeck = { c1.uid, c2.uid, c3.uid }
profile.defenseDeck = { c1.uid, c2.uid, c3.uid }
local clientView = DataService.serializeForClient(profile)
expect(type(clientView) == "table" and clientView.schemaVersion >= 2, "serializeForClient should return v2 table")

-- 6. TowerService (floor 1 + floor 100 excl_temporal_attendant)
local f1 = TowerService.getFloorConfig(1)
local f100 = TowerService.getFloorConfig(100)
expect(f1.floor == 1 and #f1.enemies >= 1, "Tower floor 1 valid")
expect(
	f100.floor == 100 and f100.dropCardId == "excl_temporal_attendant",
	"Tower floor 100 drops excl_temporal_attendant"
)
local towerOk, towerRes, towerErr = TowerService.challengeFloor(mockPlayer, profile, 1)
expect(
	towerOk == true and type(towerRes) == "table",
	"TowerService.challengeFloor(1) should succeed: " .. tostring(towerErr)
)

-- 7. RaidService (raid_skull_island + raid_shibuya_breach)
local raidList = RaidService.getStatus(profile)
expect(type(raidList) == "table", "RaidService.getStatus returns table")
profile.unlockedWorlds[3] = true
profile.unlockedWorlds[4] = true
local r1Ok, r1Res, r1Err = RaidService.startRaid(mockPlayer, profile, "raid_skull_island", true)
local r2Ok, r2Res, r2Err = RaidService.startRaid(mockPlayer, profile, "raid_shibuya_breach", true)
expect(r1Ok == true and type(r1Res) == "table", "raid_skull_island failed: " .. tostring(r1Err))
expect(r2Ok == true and type(r2Res) == "table", "raid_shibuya_breach failed: " .. tostring(r2Err))

-- 8. WorldBossService (all 5 bosses)
for wId = 1, 5 do
	profile.unlockedWorlds[wId] = true
	local wbOk, wbRes, wbErr = WorldBossService.challengeBoss(mockPlayer, profile, wId, true)
	expect(
		wbOk == true and type(wbRes) == "table",
		"WorldBossService.challengeBoss failed for world " .. tostring(wId) .. ": " .. tostring(wbErr)
	)
end

-- 9. RankedService (saveDefenseSnapshot + playMatch + Elo + Grandmaster excl_hokage_flash_kcm)
local snap = RankedService.saveDefenseSnapshot(mockPlayer.UserId, mockPlayer.Name, profile)
expect(snap ~= nil, "saveDefenseSnapshot should succeed")
local matchOk, matchRes, matchErr = RankedService.playMatch(mockPlayer, profile)
expect(
	matchOk == true and type(matchRes) == "table" and type(matchRes.newElo) == "number",
	"RankedService.playMatch should update Elo: " .. tostring(matchErr)
)
expect(RankedService.REWARD_CARD_ID == "excl_hokage_flash_kcm", "Grandmaster reward card must be excl_hokage_flash_kcm")

-- 10. RebirthService (performRebirth + buySkillNode)
profile.yen = 1000000
local rbOk, rbRes, rbErr = RebirthService.performRebirth(profile)
expect(
	rbOk == true and type(rbRes) == "table" and (profile.rebirths or 0) >= 1,
	"performRebirth should succeed: " .. tostring(rbErr)
)
local skOk, skRes, skErr = RebirthService.buySkillNode(profile, "luck_root")
if not skOk then
	skOk, skRes, skErr = RebirthService.buySkillNode(profile, "st_luck_1")
end
expect(skOk == true and type(skRes) == "table", "buySkillNode should succeed: " .. tostring(skErr))

-- 11. CharmService (craft + equip up to 3)
profile.yen = 500000
profile.gems = 50000
profile.bossTokens = 200
profile.raidTokens = 100
local testMats = {
	mat_shinobi_seal = 5,
	mat_tyrant_core = 5,
	mat_dragon_scale = 5,
	mat_dharma_wheel = 5,
	mat_pirate_crest = 5,
	mat_cursed_ember = 5,
}
profile.materials = testMats
profile.charmMaterials = testMats
local c1Ok = CharmService.craftCharm(profile, "charm_shinobi_scroll")
local c2Ok = CharmService.craftCharm(profile, "charm_tyrant_scouter")
local c3Ok = CharmService.craftCharm(profile, "charm_dragon_scale")
local c4Ok = CharmService.craftCharm(profile, "charm_dharma_wheel")
expect(c1Ok == true and c2Ok == true and c3Ok == true and c4Ok == true, "Craft 4 charms")
local eqOk, eqRes = CharmService.equipCharms(
	profile,
	{ "charm_shinobi_scroll", "charm_tyrant_scouter", "charm_dragon_scale", "charm_dharma_wheel" }
)
expect(eqOk == true and eqRes ~= nil and #eqRes.equippedCharms == 3, "equipCharms must cap at 3")

-- 12. RetentionService (quests + 7-day streak + playtime + promo codes + leaderboards)
RetentionService.trackProgress(profile, "roll", 10)
local streakRes = RetentionService.claimLoginStreak(profile)
expect(streakRes ~= nil, "claimLoginStreak should succeed")
local ptOk = RetentionService.claimPlaytime(profile, "pt_5m")
expect(type(ptOk) == "boolean", "claimPlaytime callable")
local codeRes = RetentionService.redeemCode(profile, "LAUNCH100")
expect(codeRes ~= nil, "redeemCode returns result")
RetentionService.recordLeaderboard(mockPlayer, profile)
local lbs = RetentionService.getLeaderboards()
expect(type(lbs) == "table", "getLeaderboards returns table")

-- 13. WeatherService (cycle + luck bonus)
local setOk = WeatherService.setWeather("CrimsonEclipse")
expect(setOk == true and WeatherService.getLuckBonus() == 1.0, "WeatherService CrimsonEclipse gives +1.0 luck")

-- 14. MonetizationService (22 products/passes + idempotent ProcessReceipt + Premium/Group perks)
local gpCount, dpCount = 0, 0
for _ in pairs(Products.Gamepasses) do
	gpCount += 1
end
for _ in pairs(Products.DevProducts) do
	dpCount += 1
end
expect(gpCount + dpCount == 22, "Products must have 22 total passes/products, got " .. tostring(gpCount + dpCount))
local gemsBefore = profile.gems or 0
local gOk = MonetizationService.grantProduct(mockPlayer, profile, "prod_gems_pouch", "rcpt_test_1")
local gDup = MonetizationService.grantProduct(mockPlayer, profile, "prod_gems_pouch", "rcpt_test_1")
expect(gOk == true and gDup == true and profile.gems == gemsBefore + 1200, "Idempotent receipt grant")
local perks = MonetizationService.getPerks(mockPlayer, profile)
expect(type(perks) == "table", "getPerks returns table")

print("M2_FULL_SYSTEMS_TEST_PASSED: 54 cards, 30 stages, 10 bonds, 14 M2 systems verified!")
