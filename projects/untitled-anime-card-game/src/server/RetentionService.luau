--!strict
local DataStoreService = game:GetService("DataStoreService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local ConfigFolder = Shared:WaitForChild("Config")
local Codes = require(ConfigFolder:WaitForChild("Codes"))

local RetentionService = {}

local function getQuestsConfig(): { [string]: any }
	local mod = ConfigFolder:FindFirstChild("Quests")
	if mod and mod:IsA("ModuleScript") then
		local ok, res = pcall(require, mod)
		if ok and type(res) == "table" then
			return res :: { [string]: any }
		end
	end
	return {}
end

local memoryBoards: { [string]: { [string]: { name: string, score: number } } } = {
	rolls = {},
	tower = {},
	elo = {},
	weekly_rolls = {},
}

local orderedStores: { [string]: OrderedDataStore } = {}
local storesChecked = false
local bindRegistered = false

local function getOrderedStore(boardId: string): OrderedDataStore?
	if not bindRegistered then
		bindRegistered = true
		pcall(function()
			game:BindToClose(function() end)
		end)
	end
	if RunService:IsStudio() and game.GameId == 0 then
		return nil
	end
	if not storesChecked then
		storesChecked = true
		local names = {
			rolls = "UACG_LB_Rolls_v1",
			tower = "UACG_LB_Tower_v1",
			elo = "UACG_LB_Elo_v1",
			weekly_rolls = "UACG_LB_WeeklyRolls_v1",
		}
		for id, storeName in pairs(names) do
			local ok, store = pcall(function()
				return DataStoreService:GetOrderedDataStore(storeName)
			end)
			if ok and store then
				orderedStores[id] = store
			end
		end
	end
	return orderedStores[boardId]
end

local function currentDayKey(): number
	return math.floor(os.time() / 86400)
end

local function currentWeekKey(): number
	return math.floor(os.time() / (86400 * 7))
end

local function ensureQuestState(profile: { [string]: any })
	if type(profile.quests) ~= "table" then
		profile.quests = {}
	end
	local q = profile.quests
	local dayKey = currentDayKey()
	local weekKey = currentWeekKey()
	if q.dayKey ~= dayKey then
		q.dayKey = dayKey
		q.dailyProgress = {}
		q.dailyClaimed = {}
	end
	if q.weekKey ~= weekKey then
		q.weekKey = weekKey
		q.weeklyProgress = {}
		q.weeklyClaimed = {}
	end
	if type(q.dailyProgress) ~= "table" then
		q.dailyProgress = {}
	end
	if type(q.dailyClaimed) ~= "table" then
		q.dailyClaimed = {}
	end
	if type(q.weeklyProgress) ~= "table" then
		q.weeklyProgress = {}
	end
	if type(q.weeklyClaimed) ~= "table" then
		q.weeklyClaimed = {}
	end
end

local function applyRewards(profile: { [string]: any }, reward: { [string]: any })
	profile.yen = (profile.yen or 0) + (reward.yen or reward.Yen or 0)
	profile.gems = (profile.gems or 0) + (reward.gems or reward.Gems or 0)
	profile.tickets = (profile.tickets or 0) + (reward.tickets or reward.RollTickets or 0)
	profile.traitCrystals = (profile.traitCrystals or 0) + (reward.traitCrystals or reward.TraitCrystals or 0)
	profile.bossTokens = (profile.bossTokens or 0) + (reward.bossTokens or reward.BossTokens or 0)
	profile.raidTokens = (profile.raidTokens or 0) + (reward.raidTokens or reward.RaidTokens or 0)
	if type(reward.potions) == "table" then
		if type(profile.potions) ~= "table" then
			profile.potions = {}
		end
		for potionId, qty in pairs(reward.potions) do
			profile.potions[potionId] = (profile.potions[potionId] or 0) + (qty :: number)
		end
	end
end

function RetentionService.trackProgress(profile: { [string]: any }, questType: string, amount: number?)
	ensureQuestState(profile)
	local delta = math.max(1, math.floor(amount or 1))
	local q = profile.quests
	q.dailyProgress[questType] = (q.dailyProgress[questType] or 0) + delta
	q.weeklyProgress[questType] = (q.weeklyProgress[questType] or 0) + delta
end

function RetentionService.claimQuest(
	profile: { [string]: any },
	questId: string
): (boolean, string?, { [string]: any }?)
	ensureQuestState(profile)
	local cfg = getQuestsConfig()
	local q = profile.quests

	local function tryList(
		list: any,
		progressTbl: { [string]: number },
		claimedTbl: { [string]: boolean }
	): (boolean, string?, { [string]: any }?)
		if type(list) ~= "table" then
			return false, nil, nil
		end
		for _, def in ipairs(list) do
			if type(def) == "table" and def.id == questId then
				if claimedTbl[questId] then
					return true, "Quest already claimed.", nil
				end
				local statKey = def.stat or def.questType or def.type or "roll"
				local target = def.target or def.goal or 1
				local cur = progressTbl[statKey] or 0
				if cur < target then
					return true, string.format("Progress %d/%d", cur, target), nil
				end
				claimedTbl[questId] = true
				local reward = def.reward or def
				applyRewards(profile, reward)
				return true, nil, { questId = questId, reward = reward }
			end
		end
		return false, nil, nil
	end

	local foundD, errD, resD = tryList(cfg.Daily, q.dailyProgress, q.dailyClaimed)
	if foundD then
		return resD ~= nil, errD, resD
	end
	local foundW, errW, resW = tryList(cfg.Weekly, q.weeklyProgress, q.weeklyClaimed)
	if foundW then
		return resW ~= nil, errW, resW
	end
	return false, "Quest not found.", nil
end

function RetentionService.claimLoginStreak(profile: { [string]: any }): (boolean, string?, { [string]: any }?)
	local dayKey = currentDayKey()
	local lastDay = profile.lastLoginDay or 0
	if lastDay == dayKey then
		return false, "Today's login streak already claimed.", nil
	end

	local streak = profile.loginStreak or profile.dailyStreak or 0
	if lastDay == dayKey - 1 then
		streak += 1
	else
		streak = 1
	end
	local cycleDay = ((streak - 1) % 7) + 1
	profile.lastLoginDay = dayKey
	profile.lastDailyClaim = os.time()
	profile.loginStreak = streak
	profile.dailyStreak = streak

	local cfg = getQuestsConfig()
	local reward: { [string]: any } =
		{ yen = 250 + cycleDay * 100, gems = 50 + cycleDay * 20, tickets = if cycleDay == 7 then 5 else 1 }
	if type(cfg.LoginStreak) == "table" and type(cfg.LoginStreak[cycleDay]) == "table" then
		reward = cfg.LoginStreak[cycleDay]
	end
	applyRewards(profile, reward)
	return true, nil, { day = cycleDay, streak = streak, reward = reward }
end

function RetentionService.claimPlaytime(
	profile: { [string]: any },
	milestoneId: string
): (boolean, string?, { [string]: any }?)
	if type(profile.claimedPlaytime) ~= "table" then
		profile.claimedPlaytime = {}
	end
	if profile.claimedPlaytime[milestoneId] then
		return false, "Playtime milestone already claimed.", nil
	end

	local cfg = getQuestsConfig()
	local milestones = cfg.Playtime or cfg.PlaytimeMilestones
	local targetDef: { [string]: any }? = nil
	if type(milestones) == "table" then
		for _, def in ipairs(milestones) do
			if type(def) == "table" and def.id == milestoneId then
				targetDef = def
				break
			end
		end
	end
	if not targetDef then
		return false, "Unknown playtime milestone.", nil
	end
	local reqSec = targetDef.seconds or ((targetDef.minutes or 5) * 60)
	local played = profile.playtimeSeconds or 0
	if played < reqSec then
		return false, string.format("Play %ds more to claim.", reqSec - played), nil
	end
	profile.claimedPlaytime[milestoneId] = true
	local reward = targetDef.reward or targetDef
	applyRewards(profile, reward)
	return true, nil, { milestoneId = milestoneId, reward = reward }
end

function RetentionService.redeemCode(
	profile: { [string]: any },
	rawCode: string
): (boolean, string?, { [string]: any }?)
	local key = Codes.normalize(rawCode)
	local entry = Codes.get(key)
	if not entry then
		return false, "Invalid or expired promo code.", nil
	end
	if type(profile.redeemedCodes) ~= "table" then
		profile.redeemedCodes = {}
	end
	if profile.redeemedCodes[key] then
		return false, "Code already redeemed.", nil
	end
	profile.redeemedCodes[key] = true
	applyRewards(profile, entry :: any)
	return true, nil, { code = key, description = entry.description, reward = entry }
end

function RetentionService.recordLeaderboard(player: Player, profile: { [string]: any })
	local uidStr = tostring(player.UserId)
	local scores = {
		rolls = math.floor(profile.rollCount or 0),
		tower = math.floor(profile.towerFloor or 1),
		elo = math.floor(profile.elo or 1000),
		weekly_rolls = math.floor(profile.rollCount or 0),
	}
	for boardId, score in pairs(scores) do
		memoryBoards[boardId][uidStr] = { name = player.Name, score = score }
		local store = getOrderedStore(boardId)
		if store and score > 0 then
			pcall(function()
				store:SetAsync(uidStr, score)
			end)
		end
	end
end

function RetentionService.getLeaderboards(): { [string]: { { name: string, score: number, rank: number } } }
	local out: { [string]: { { name: string, score: number, rank: number } } } = {}
	for boardId, memMap in pairs(memoryBoards) do
		local list: { { name: string, score: number, rank: number } } = {}
		local store = getOrderedStore(boardId)
		if store then
			local ok, pages = pcall(function()
				return store:GetSortedAsync(false, 10)
			end)
			if ok and pages then
				local items = pages:GetCurrentPage()
				for i, item in ipairs(items) do
					table.insert(list, { name = tostring(item.key), score = tonumber(item.value) or 0, rank = i })
				end
			end
		end
		if #list == 0 then
			for _, entry in pairs(memMap) do
				table.insert(list, { name = entry.name, score = entry.score, rank = 0 })
			end
			table.sort(list, function(a, b)
				return a.score > b.score
			end)
			for i, entry in ipairs(list) do
				entry.rank = i
			end
		end
		out[boardId] = list
	end
	return out
end

function RetentionService.getStatus(profile: { [string]: any }): { [string]: any }
	ensureQuestState(profile)
	local dayKey = currentDayKey()
	return {
		quests = profile.quests,
		loginStreak = profile.loginStreak or 0,
		canClaimStreak = (profile.lastLoginDay or 0) ~= dayKey,
		playtimeSeconds = profile.playtimeSeconds or 0,
		claimedPlaytime = profile.claimedPlaytime or {},
	}
end

return RetentionService
