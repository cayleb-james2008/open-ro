--!strict
export type SkillNodeBranch = "Luck" | "Combat" | "Economy"

export type SkillNodeDef = {
	id: string,
	name: string,
	branch: SkillNodeBranch,
	description: string,
	maxRank: number,
	costPerRank: number,
	minRebirths: number,
	requires: { string },
	effectKey: "luckBonus" | "atkPct" | "hpPct" | "spdPct" | "critBonus" | "yenMult" | "gemMult" | "cooldownReduction",
	effectPerRank: number,
}

export type SkillBonuses = {
	luckBonus: number,
	atkPct: number,
	hpPct: number,
	spdPct: number,
	critBonus: number,
	yenMult: number,
	gemMult: number,
	cooldownReduction: number,
}

local SkillTree = {}

SkillTree.Nodes = {
	{
		id = "luck_root",
		name = "Fortune Awakening",
		branch = "Luck",
		description = "+0.15x additive Luck per rank.",
		maxRank = 5,
		costPerRank = 1,
		minRebirths = 1,
		requires = {},
		effectKey = "luckBonus",
		effectPerRank = 0.15,
	},
	{
		id = "luck_nexus",
		name = "Astral Resonance",
		branch = "Luck",
		description = "+0.30x additive Luck per rank.",
		maxRank = 5,
		costPerRank = 2,
		minRebirths = 3,
		requires = { "luck_root" },
		effectKey = "luckBonus",
		effectPerRank = 0.30,
	},
	{
		id = "luck_haste",
		name = "Swift Summoner",
		branch = "Luck",
		description = "-4% Roll cooldown per rank.",
		maxRank = 5,
		costPerRank = 2,
		minRebirths = 2,
		requires = { "luck_root" },
		effectKey = "cooldownReduction",
		effectPerRank = 0.04,
	},
	{
		id = "combat_might",
		name = "Spirit Power",
		branch = "Combat",
		description = "+8% Team ATK per rank.",
		maxRank = 5,
		costPerRank = 1,
		minRebirths = 1,
		requires = {},
		effectKey = "atkPct",
		effectPerRank = 0.08,
	},
	{
		id = "combat_vitality",
		name = "Titan Vigor",
		branch = "Combat",
		description = "+10% Team HP per rank.",
		maxRank = 5,
		costPerRank = 1,
		minRebirths = 1,
		requires = {},
		effectKey = "hpPct",
		effectPerRank = 0.10,
	},
	{
		id = "combat_tempo",
		name = "Flash Step Mastery",
		branch = "Combat",
		description = "+5% Team SPD per rank.",
		maxRank = 4,
		costPerRank = 2,
		minRebirths = 2,
		requires = { "combat_might" },
		effectKey = "spdPct",
		effectPerRank = 0.05,
	},
	{
		id = "combat_crit",
		name = "Lethal Focus",
		branch = "Combat",
		description = "+3% Team Crit Chance per rank.",
		maxRank = 5,
		costPerRank = 2,
		minRebirths = 4,
		requires = { "combat_might", "combat_vitality" },
		effectKey = "critBonus",
		effectPerRank = 0.03,
	},
	{
		id = "econ_yen",
		name = "Golden Harvest",
		branch = "Economy",
		description = "+12% Yen earned from battles per rank.",
		maxRank = 5,
		costPerRank = 1,
		minRebirths = 1,
		requires = {},
		effectKey = "yenMult",
		effectPerRank = 0.12,
	},
	{
		id = "econ_gems",
		name = "Starlight Harvester",
		branch = "Economy",
		description = "+10% Gem rewards per rank.",
		maxRank = 5,
		costPerRank = 2,
		minRebirths = 3,
		requires = { "econ_yen" },
		effectKey = "gemMult",
		effectPerRank = 0.10,
	},
} :: { SkillNodeDef }

SkillTree.ById = {} :: { [string]: SkillNodeDef }
for _, node in ipairs(SkillTree.Nodes) do
	SkillTree.ById[node.id] = node
end

function SkillTree.get(id: string): SkillNodeDef?
	return SkillTree.ById[id]
end

function SkillTree.canUpgrade(
	nodeId: string,
	currentNodes: { [string]: number },
	rebirths: number,
	availablePoints: number
): (boolean, string?)
	local def = SkillTree.ById[nodeId]
	if not def then
		return false, "Unknown skill node"
	end
	local rank = currentNodes[nodeId] or 0
	if rank >= def.maxRank then
		return false, "Node already at max rank"
	end
	if rebirths < def.minRebirths then
		return false, string.format("Requires %d Rebirths", def.minRebirths)
	end
	for _, reqId in ipairs(def.requires) do
		if (currentNodes[reqId] or 0) <= 0 then
			return false, "Missing prerequisite node"
		end
	end
	if availablePoints < def.costPerRank then
		return false, "Not enough Rebirth Points"
	end
	return true, nil
end

function SkillTree.computeBonuses(ownedNodes: { [string]: number }?): SkillBonuses
	local bonuses: SkillBonuses = {
		luckBonus = 0,
		atkPct = 0,
		hpPct = 0,
		spdPct = 0,
		critBonus = 0,
		yenMult = 1.0,
		gemMult = 1.0,
		cooldownReduction = 0,
	}
	if not ownedNodes then
		return bonuses
	end
	for nodeId, rank in pairs(ownedNodes) do
		local def = SkillTree.ById[nodeId]
		if def and rank > 0 then
			local clampedRank = math.min(def.maxRank, math.floor(rank))
			local total = clampedRank * def.effectPerRank
			if def.effectKey == "yenMult" then
				bonuses.yenMult += total
			elseif def.effectKey == "gemMult" then
				bonuses.gemMult += total
			elseif def.effectKey == "luckBonus" then
				bonuses.luckBonus += total
			elseif def.effectKey == "atkPct" then
				bonuses.atkPct += total
			elseif def.effectKey == "hpPct" then
				bonuses.hpPct += total
			elseif def.effectKey == "spdPct" then
				bonuses.spdPct += total
			elseif def.effectKey == "critBonus" then
				bonuses.critBonus += total
			elseif def.effectKey == "cooldownReduction" then
				bonuses.cooldownReduction += total
			end
		end
	end
	return bonuses
end

return SkillTree
