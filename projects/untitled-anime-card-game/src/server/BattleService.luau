--!strict
local AnalyticsService = game:GetService("AnalyticsService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local Cards = require(Config:WaitForChild("Cards"))
local Worlds = require(Config:WaitForChild("Worlds"))
local BattleSim = require(Shared:WaitForChild("BattleSim"))

local DataService = require(script.Parent:WaitForChild("DataService"))

local BattleService = {}

local function findStage(stageId: string): (Worlds.WorldDef?, Worlds.StageDef?, number?)
	for _, world in ipairs(Worlds.List) do
		for idx, stage in ipairs(world.stages) do
			if stage.id == stageId then
				return world, stage, idx
			end
		end
	end
	return nil, nil, nil
end

local function isStageUnlocked(profile: DataService.PlayerProfile, world: Worlds.WorldDef, stageIdx: number): boolean
	local unlockedWorlds = profile.unlockedWorlds or {}
	if world.id > 1 and not unlockedWorlds[world.id] and (profile.currentWorld or 1) < world.id then
		return false
	end
	if stageIdx <= 1 then
		return true
	end
	local prevStage = world.stages[stageIdx - 1]
	if not prevStage then
		return true
	end
	return profile.clearedStages[prevStage.id] == true
end

local function logBattleAnalytics(player: Player?, stageId: string, won: boolean, yenEarned: number)
	if not player then
		return
	end
	pcall(function()
		(AnalyticsService :: any):LogCustomEvent(player, "StageBattle", 1, {
			stageId = stageId,
			won = won,
			yenEarned = yenEarned,
		})
	end)
end

function BattleService.startBattle(player: Player, stageId: string): (boolean, { [string]: any }?, string?)
	local profile = DataService.getProfile(player)
	if not profile then
		return false, nil, "Profile not loaded"
	end
	if type(stageId) ~= "string" or stageId == "" then
		return false, nil, "Invalid stage"
	end

	local world, stage, stageIdx = findStage(stageId)
	if not world or not stage or not stageIdx then
		return false, nil, "Unknown stage"
	end
	if not isStageUnlocked(profile, world, stageIdx) then
		return false, nil, "Clear the previous stage first!"
	end

	local deckCards: { DataService.CardInstance } = {}
	for _, uid in ipairs(profile.activeDeck) do
		local inst = profile.cards[uid]
		if inst then
			table.insert(deckCards, inst)
		end
	end
	if #deckCards == 0 then
		return false, nil, "Equip at least 1 card in your deck before battling!"
	end

	local allies: { BattleSim.CombatUnit } = {}
	for idx, card in ipairs(deckCards) do
		local unit = BattleSim.buildPlayerUnit(idx, {
			uid = card.uid,
			cardId = card.cardId,
			level = card.level,
			mutation = card.mutation,
			trait = card.trait,
			ascension = card.ascension,
		})
		if unit then
			table.insert(allies, unit)
		end
	end
	if #allies == 0 then
		return false, nil, "No valid cards in your active deck!"
	end

	local enemies: { BattleSim.CombatUnit } = {}
	for idx, enemySpec in ipairs((stage :: any).enemies or {}) do
		local enemyCard: any = nil
		if type(enemySpec.cardId) == "string" and enemySpec.cardId ~= "" then
			local cardsAny = Cards :: any
			if type(cardsAny.get) == "function" then
				enemyCard = cardsAny.get(enemySpec.cardId)
			elseif type(cardsAny.ById) == "table" then
				enemyCard = cardsAny.ById[enemySpec.cardId]
			elseif type(cardsAny.List) == "table" then
				for _, cardDef in ipairs(cardsAny.List) do
					if cardDef.id == enemySpec.cardId then
						enemyCard = cardDef
						break
					end
				end
			end
		end
		table.insert(
			enemies,
			BattleSim.buildEnemyUnit(idx, {
				id = enemySpec.cardId or ("enemy_" .. tostring(idx)),
				name = enemySpec.name,
				rarity = if enemyCard then enemyCard.rarity else "Common",
				role = if enemyCard then enemyCard.role else "Striker",
				hp = enemySpec.hp,
				atk = enemySpec.atk,
				spd = enemySpec.spd,
				skill = {
					name = enemySpec.skillName or "Strike",
					description = enemySpec.skillName or "Strike",
					cooldown = 3,
					dmgMult = enemySpec.skillMult or 1.25,
					targeting = "Single",
				},
			})
		)
	end
	if #enemies == 0 then
		return false, nil, "Stage has no enemies configured!"
	end

	local seed = math.floor(os.clock() * 1000) + profile.rollCount * 31 + player.UserId
	local simResult = BattleSim.run(allies, enemies, { seed = seed, maxRounds = 30 })

	local rewards = {
		yen = 0,
		gems = 0,
		tickets = 0,
		bossTokens = 0,
		xp = 0,
		firstClear = false,
		droppedCardId = nil :: string?,
	}

	if simResult.won then
		local isFirstClear = profile.clearedStages[stage.id] ~= true
		profile.clearedStages[stage.id] = true

		local yenEarned = stage.yenReward
		local gemsEarned = stage.gemReward + (if isFirstClear then stage.firstClearGems else 0)
		local ticketsEarned = if isFirstClear then stage.firstClearTickets else 0
		local bossTokensEarned = stage.bossTokenReward or 0

		profile.yen += yenEarned
		profile.gems += gemsEarned
		profile.tickets += ticketsEarned
		profile.bossTokens += bossTokensEarned

		rewards.yen = yenEarned
		rewards.gems = gemsEarned
		rewards.tickets = ticketsEarned
		rewards.bossTokens = bossTokensEarned
		local xpEarned = stage.xpReward or 0
		rewards.xp = xpEarned
		rewards.firstClear = isFirstClear

		for _, inst in ipairs(deckCards) do
			inst.xp = (inst.xp or 0) + xpEarned
		end

		if stage.dropCardId and stage.dropChance and math.random() < stage.dropChance then
			DataService.addCard(profile, stage.dropCardId, "Normal", nil)
			rewards.droppedCardId = stage.dropCardId
		end

		if stage.isBoss then
			local nextWorldId = world.id + 1
			if Worlds.get(nextWorldId) then
				profile.unlockedWorlds[nextWorldId] = true
			end
		end
	else
		local consolationYen = math.max(15, math.floor(stage.yenReward * 0.2))
		profile.yen += consolationYen
		rewards.yen = consolationYen
	end

	logBattleAnalytics(player, stage.id, simResult.won, rewards.yen)

	local battlePayload = {
		stageId = stage.id,
		stageName = stage.name,
		won = simResult.won,
		rounds = simResult.rounds,
		events = simResult.events,
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
