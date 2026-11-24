--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local ConfigFolder = Shared:WaitForChild("Config")
local Worlds = require(ConfigFolder:WaitForChild("Worlds"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))
local DataService = require(script.Parent:WaitForChild("DataService"))
local DeckService = require(script.Parent:WaitForChild("DeckService"))

local BattleService = {}

local function cloneUnits(units: { BattleSim.CombatUnit }): { { [string]: any } }
	local out: { { [string]: any } } = {}
	for _, u in ipairs(units) do
		table.insert(out, {
			slot = u.slot,
			name = u.name,
			cardId = u.cardId,
			role = u.role,
			showName = u.showName,
			hp = u.maxHp,
			maxHp = u.maxHp,
			atk = u.atk,
			spd = u.spd,
		})
	end
	return out
end

local function buildEnemiesFromStage(stage: any): { BattleSim.CombatUnit }
	local enemies: { BattleSim.CombatUnit } = {}
	local list = stage.enemies or {}
	for idx, e in ipairs(list) do
		local hp = math.max(10, math.floor(e.hp or 100))
		local atk = math.max(5, math.floor(e.atk or 20))
		local spd = math.max(5, math.floor(e.spd or 10))
		table.insert(
			enemies,
			{
				slot = idx,
				uid = "enemy_" .. tostring(idx),
				cardId = e.cardId or "enemy",
				name = e.name or ("Enemy " .. tostring(idx)),
				role = e.role or "Striker",
				showName = e.showName or "Enemy",
				hp = hp,
				maxHp = hp,
				atk = atk,
				spd = spd,
				shield = 0,
				critChance = 0.05,
				critMult = 1.5,
				lifesteal = 0,
				alive = true,
				isBoss = e.isBoss == true or stage.isBoss == true,
			} :: any
		)
	end
	return enemies
end

function BattleService.startBattle(
	profile: DataService.PlayerProfile,
	stageId: string
): (boolean, { [string]: any }?, string?)
	if type(stageId) ~= "string" or stageId == "" then
		return false, nil, "Missing stageId."
	end
	local stage = Worlds.getStage(stageId)
	if not stage then
		return false, nil, "Unknown stage."
	end
	local worldId = (stage :: any).worldId or 1
	if not profile.unlockedWorlds[worldId] then
		return false, nil, "World is locked."
	end

	local allies: { BattleSim.CombatUnit } = {}
	for slot, uid in ipairs(profile.activeDeck) do
		local inst = profile.cards[uid]
		if inst then
			local unit = BattleSim.buildPlayerUnit(slot, inst)
			if unit then
				table.insert(allies, unit)
			end
		end
	end
	if #allies == 0 then
		return false, nil, "Your active deck is empty."
	end

	local enemies = buildEnemiesFromStage(stage)
	if #enemies == 0 then
		return false, nil, "Stage has no enemies."
	end

	local squadBonuses = DeckService.getSquadBonuses(profile)
	local initialAllies = cloneUnits(allies)
	local initialEnemies = cloneUnits(enemies)

	local simResult = (BattleSim.run :: any)(allies, enemies, squadBonuses)
	local won = simResult.winner == "allies" or simResult.won == true

	local rewards: { [string]: any } = {
		yen = 0,
		gems = 0,
		tickets = 0,
		bossTokens = 0,
		materials = {},
		unlockedWorld = nil,
	}

	if won then
		local firstClear = profile.clearedStages[stageId] ~= true
		profile.clearedStages[stageId] = true

		local baseYen = (stage :: any).rewardYen or 100
		local yenMult = squadBonuses.yenMult or 1
		local okW, weatherMod = pcall(function()
			return require(script.Parent:FindFirstChild("WeatherService") :: any)
		end)
		if okW and type(weatherMod) == "table" and type(weatherMod.getYenMult) == "function" then
			yenMult *= weatherMod.getYenMult()
		end
		local yenEarned = math.max(10, math.floor(baseYen * yenMult))
		local gemsEarned = if firstClear
			then ((stage :: any).firstClearGems or 25)
			else ((stage :: any).rewardGems or 0)
		local ticketsEarned = if firstClear then ((stage :: any).firstClearTickets or 0) else 0
		local bossTokensEarned = if (stage :: any).isBoss then ((stage :: any).rewardBossTokens or 1) else 0

		profile.yen = (profile.yen or 0) + yenEarned
		profile.gems = (profile.gems or 0) + gemsEarned
		profile.tickets = (profile.tickets or 0) + ticketsEarned
		profile.bossTokens = (profile.bossTokens or 0) + bossTokensEarned

		rewards.yen = yenEarned
		rewards.gems = gemsEarned
		rewards.tickets = ticketsEarned
		rewards.bossTokens = bossTokensEarned

		local matId = (stage :: any).materialDrop
		if type(matId) == "string" and matId ~= "" then
			profile.materials[matId] = (profile.materials[matId] or 0) + 1
			rewards.materials[matId] = 1
		end

		if (stage :: any).isBoss then
			local nextWorld = worldId + 1
			if Worlds.get(nextWorld) and not profile.unlockedWorlds[nextWorld] then
				profile.unlockedWorlds[nextWorld] = true
				profile.currentWorld = math.max(profile.currentWorld or 1, nextWorld)
				rewards.unlockedWorld = nextWorld
			end
			local bossCardId = (stage :: any).bossCardId
			if type(bossCardId) == "string" and bossCardId ~= "" and firstClear then
				DataService.addCard(profile, bossCardId, "Normal", nil)
				rewards.bossCardId = bossCardId
			end
		end

		local okRet, retMod = pcall(function()
			return require(script.Parent:FindFirstChild("RetentionService") :: any)
		end)
		if okRet and type(retMod) == "table" and type(retMod.trackProgress) == "function" then
			pcall(retMod.trackProgress, profile, "battles_won", 1)
			if (stage :: any).isBoss then
				pcall(retMod.trackProgress, profile, "bosses_cleared", 1)
			end
		end
	end

	local normalizedEvents: { { [string]: any } } = {}
	for _, ev in ipairs(simResult.events or {}) do
		local killed = ev.killed == true or (ev.targetHp ~= nil and ev.targetHp <= 0)
		table.insert(normalizedEvents, {
			round = ev.round,
			turn = ev.round,
			kind = ev.kind,
			actorSide = ev.actorSide,
			actorIndex = ev.actorSlot or ev.actorIndex,
			actorName = ev.actorName,
			targetSide = ev.targetSide,
			targetIndex = ev.targetSlot or ev.targetIndex,
			targetName = ev.targetName,
			damage = ev.damage,
			isCrit = ev.isCrit,
			skillName = ev.skillName,
			targetHp = ev.targetHp,
			targetHpAfter = ev.targetHp,
			killed = killed,
			defeated = killed,
		})
	end

	local battlePayload = {
		stageId = stageId,
		worldId = worldId,
		won = simResult.won,
		victory = simResult.won,
		winner = if won then "allies" else "enemies",
		allies = initialAllies,
		enemies = initialEnemies,
		events = normalizedEvents,
		alliesSummary = simResult.alliesSummary,
		enemiesSummary = simResult.enemiesSummary,
		synergies = simResult.synergies,
		mvpName = simResult.mvpName,
		rewards = rewards,
	}

	return true, {
		battle = battlePayload,
		state = DataService.serializeForClient(profile),
	}, nil
end

return BattleService
