--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local DataService = require(script.Parent:WaitForChild("DataService"))

local RebirthService = {}

export type SkillNodeDef = {
	id: string,
	name: string,
	description: string,
	cost: number,
	prereq: string?,
	effectType: string,
	effectValue: number,
}

local DEFAULT_SKILL_NODES: { SkillNodeDef } = {
	{
		id = "st_luck_1",
		name = "Astral Fortune I",
		description = "+15% Summon Luck",
		cost = 1,
		prereq = nil,
		effectType = "luck",
		effectValue = 0.15,
	},
	{
		id = "st_atk_1",
		name = "Spirit Edge I",
		description = "+10% Squad ATK",
		cost = 1,
		prereq = nil,
		effectType = "atk",
		effectValue = 0.10,
	},
	{
		id = "st_hp_1",
		name = "Iron Will I",
		description = "+12% Squad HP",
		cost = 1,
		prereq = nil,
		effectType = "hp",
		effectValue = 0.12,
	},
	{
		id = "st_yen_1",
		name = "Bounty Hunter I",
		description = "+20% Yen Earned",
		cost = 1,
		prereq = "st_atk_1",
		effectType = "yen",
		effectValue = 0.20,
	},
	{
		id = "st_luck_2",
		name = "Astral Fortune II",
		description = "+25% Summon Luck",
		cost = 2,
		prereq = "st_luck_1",
		effectType = "luck",
		effectValue = 0.25,
	},
	{
		id = "st_atk_2",
		name = "Transcendent Strike",
		description = "+18% Squad ATK",
		cost = 2,
		prereq = "st_atk_1",
		effectType = "atk",
		effectValue = 0.18,
	},
	{
		id = "st_hp_2",
		name = "Immortal Core",
		description = "+22% Squad HP",
		cost = 2,
		prereq = "st_hp_1",
		effectType = "hp",
		effectValue = 0.22,
	},
	{
		id = "st_crit_1",
		name = "Flash Step Mastery",
		description = "+6 Squad SPD",
		cost = 2,
		prereq = "st_yen_1",
		effectType = "spd",
		effectValue = 6,
	},
}

local function getSkillNodesList(): { SkillNodeDef }
	local mod = Config:FindFirstChild("SkillTree")
	if mod and mod:IsA("ModuleScript") then
		local ok, tbl = pcall(require, mod)
		if ok and type(tbl) == "table" then
			local list = tbl.Nodes or tbl.List or tbl
			if type(list) == "table" and #list > 0 then
				return list :: { SkillNodeDef }
			end
		end
	end
	return DEFAULT_SKILL_NODES
end

local function findNode(nodeId: string): SkillNodeDef?
	for _, node in ipairs(getSkillNodesList()) do
		if node.id == nodeId then
			return node
		end
	end
	return nil
end

function RebirthService.getRequirement(
	rebirths: number
): { yenCost: number, requiredStageId: string, requiredWorld: number }
	local r = math.max(0, math.floor(rebirths or 0))
	local requiredWorld = math.min(5, 1 + math.floor(r / 2))
	return {
		yenCost = math.floor(2500 * math.pow(2.2, r)),
		requiredStageId = string.format("w%d_act5", requiredWorld),
		requiredWorld = requiredWorld,
	}
end

function RebirthService.canRebirth(profile: DataService.PlayerProfile): (boolean, string?)
	local req = RebirthService.getRequirement(profile.rebirths or 0)
	if (profile.yen or 0) < req.yenCost then
		return false, string.format("Need %d Yen to Rebirth.", req.yenCost)
	end
	local cleared = profile.clearedStages or {}
	local worldCleared = cleared[req.requiredStageId] == true
		or (profile.unlockedWorlds and profile.unlockedWorlds[req.requiredWorld + 1] == true)
	if not worldCleared then
		return false, string.format("Clear World %d Act 5 first to Rebirth.", req.requiredWorld)
	end
	return true, nil
end

function RebirthService.performRebirth(profile: DataService.PlayerProfile): (boolean, { [string]: any }?, string?)
	local ok, err = RebirthService.canRebirth(profile)
	if not ok then
		return false, nil, err
	end

	local pAny = profile :: any
	local newRebirths = (profile.rebirths or 0) + 1
	local skillPointsGained = 1 + math.floor(newRebirths / 3)

	profile.rebirths = newRebirths
	pAny.skillPoints = (pAny.skillPoints or 0) + skillPointsGained
	profile.gems = (profile.gems or 0) + 50 * newRebirths

	-- Soft reset world progression & upgrade tiers while keeping cards, index, passes, charms, and rebirth tree
	profile.yen = 150
	profile.currentWorld = 1
	profile.unlockedWorlds = { [1] = true }
	profile.clearedStages = {}
	profile.luckUpgradeLevel = 0
	pAny.towerFloor = 0
	pAny.selectedBannerId = "banner_w1"

	return true,
		{
			rebirths = newRebirths,
			skillPoints = pAny.skillPoints,
			skillPointsGained = skillPointsGained,
			luckBonusPercent = newRebirths * 10,
		},
		nil
end

function RebirthService.buySkillNode(
	profile: DataService.PlayerProfile,
	nodeId: string
): (boolean, { [string]: any }?, string?)
	local node = findNode(nodeId)
	if not node then
		return false, nil, "Unknown skill tree node."
	end
	local pAny = profile :: any
	if type(pAny.skillNodes) ~= "table" then
		pAny.skillNodes = {}
	end
	if pAny.skillNodes[nodeId] == true then
		return false, nil, "Skill node already unlocked."
	end
	if node.prereq and pAny.skillNodes[node.prereq] ~= true then
		return false, nil, "Unlock prerequisite node first."
	end
	local points = pAny.skillPoints or 0
	if points < node.cost then
		return false, nil, string.format("Need %d Skill Points.", node.cost)
	end

	pAny.skillPoints = points - node.cost
	pAny.skillNodes[nodeId] = true

	return true, {
		nodeId = nodeId,
		skillPoints = pAny.skillPoints,
		skillNodes = pAny.skillNodes,
	}, nil
end

function RebirthService.getBonuses(profile: DataService.PlayerProfile): {
	luckBonus: number,
	atkMult: number,
	hpMult: number,
	yenMult: number,
	spdBonus: number,
	rebirthLuckMult: number,
}
	local pAny = profile :: any
	local ownedNodes = if type(pAny.skillNodes) == "table" then pAny.skillNodes else {}
	local luckBonus = 0
	local atkBonus = 0
	local hpBonus = 0
	local yenBonus = 0
	local spdBonus = 0

	for _, node in ipairs(getSkillNodesList()) do
		if ownedNodes[node.id] == true then
			local et = node.effectType
			local val = node.effectValue or 0
			if et == "luck" then
				luckBonus += val
			elseif et == "atk" then
				atkBonus += val
			elseif et == "hp" then
				hpBonus += val
			elseif et == "yen" then
				yenBonus += val
			elseif et == "spd" then
				spdBonus += val
			end
		end
	end

	local rebirths = profile.rebirths or 0
	local rebirthCombatBonus = rebirths * 0.10
	return {
		luckBonus = luckBonus,
		atkMult = 1 + rebirthCombatBonus + atkBonus,
		hpMult = 1 + rebirthCombatBonus + hpBonus,
		yenMult = 1 + yenBonus,
		spdBonus = spdBonus,
		rebirthLuckMult = 1 + rebirths * 0.10,
	}
end

function RebirthService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	local can, reason = RebirthService.canRebirth(profile)
	return {
		rebirths = profile.rebirths or 0,
		skillPoints = pAny.skillPoints or 0,
		skillNodes = pAny.skillNodes or {},
		requirement = RebirthService.getRequirement(profile.rebirths or 0),
		canRebirth = can,
		reason = reason,
		bonuses = RebirthService.getBonuses(profile),
		catalog = getSkillNodesList(),
	}
end

return RebirthService
