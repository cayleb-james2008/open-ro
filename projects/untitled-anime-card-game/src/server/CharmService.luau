--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")
local DataService = require(script.Parent:WaitForChild("DataService"))

local CharmService = {}

CharmService.MAX_EQUIPPED = 3

export type CharmDef = {
	id: string,
	name: string,
	description: string,
	materialId: string,
	materialCost: number,
	bossTokenCost: number,
	yenCost: number,
	hpBonus: number,
	atkBonus: number,
	spdBonus: number,
	luckBonus: number,
	yenBonus: number,
}

local DEFAULT_CHARMS: { CharmDef } = {
	{
		id = "charm_shinobi_scroll",
		name = "Shinobi Seal Scroll",
		description = "+10% Squad HP, +5% ATK",
		materialId = "mat_shinobi_seal",
		materialCost = 1,
		bossTokenCost = 10,
		yenCost = 500,
		hpBonus = 0.10,
		atkBonus = 0.05,
		spdBonus = 0,
		luckBonus = 0,
		yenBonus = 0,
	},
	{
		id = "charm_tyrant_scouter",
		name = "Golden Tyrant Scouter",
		description = "+12% Squad ATK, +10% Luck",
		materialId = "mat_tyrant_core",
		materialCost = 1,
		bossTokenCost = 18,
		yenCost = 1200,
		hpBonus = 0,
		atkBonus = 0.12,
		spdBonus = 2,
		luckBonus = 0.10,
		yenBonus = 0,
	},
	{
		id = "charm_dragon_scale",
		name = "Azure Dragon Scale",
		description = "+18% Squad HP, +12% ATK",
		materialId = "mat_dragon_scale",
		materialCost = 1,
		bossTokenCost = 25,
		yenCost = 2200,
		hpBonus = 0.18,
		atkBonus = 0.12,
		spdBonus = 0,
		luckBonus = 0.05,
		yenBonus = 0,
	},
	{
		id = "charm_dharma_wheel",
		name = "Dharma Adaptation Wheel",
		description = "+20% HP, +15% ATK, +5 SPD",
		materialId = "mat_dharma_wheel",
		materialCost = 1,
		bossTokenCost = 35,
		yenCost = 3500,
		hpBonus = 0.20,
		atkBonus = 0.15,
		spdBonus = 5,
		luckBonus = 0.08,
		yenBonus = 0,
	},
	{
		id = "charm_pirate_compass",
		name = "Log Pose of Laugh Tale",
		description = "+25% Luck, +25% Yen",
		materialId = "mat_pirate_crest",
		materialCost = 2,
		bossTokenCost = 30,
		yenCost = 3000,
		hpBonus = 0.08,
		atkBonus = 0.08,
		spdBonus = 3,
		luckBonus = 0.25,
		yenBonus = 0.25,
	},
	{
		id = "charm_cursed_ember",
		name = "Shibuya Ember Talisman",
		description = "+22% ATK, +15% Luck",
		materialId = "mat_cursed_ember",
		materialCost = 2,
		bossTokenCost = 40,
		yenCost = 4500,
		hpBonus = 0.10,
		atkBonus = 0.22,
		spdBonus = 4,
		luckBonus = 0.15,
		yenBonus = 0,
	},
}

local function getCatalog(): { CharmDef }
	local mod = Config:FindFirstChild("Charms")
	if mod and mod:IsA("ModuleScript") then
		local ok, tbl = pcall(require, mod)
		if ok and type(tbl) == "table" then
			local list = tbl.List or tbl.Charms or tbl
			if type(list) == "table" and #list > 0 then
				return list :: { CharmDef }
			end
		end
	end
	return DEFAULT_CHARMS
end

local function findCharm(charmId: string): CharmDef?
	for _, c in ipairs(getCatalog()) do
		if c.id == charmId then
			return c
		end
	end
	return nil
end

function CharmService.craftCharm(
	profile: DataService.PlayerProfile,
	charmId: string
): (boolean, { [string]: any }?, string?)
	local def = findCharm(charmId)
	if not def then
		return false, nil, "Unknown charm."
	end
	local pAny = profile :: any
	if type(pAny.charms) ~= "table" then
		pAny.charms = {}
	end
	if pAny.charms[charmId] then
		return false, nil, "You already crafted this charm."
	end
	if (profile.yen or 0) < (def.yenCost or 0) then
		return false, nil, string.format("Need %d Yen to craft.", def.yenCost)
	end
	if (profile.bossTokens or 0) < (def.bossTokenCost or 0) then
		return false, nil, string.format("Need %d Boss Tokens to craft.", def.bossTokenCost)
	end
	local mats = if type(pAny.materials) == "table" then pAny.materials else {}
	if (def.materialCost or 0) > 0 and (mats[def.materialId] or 0) < def.materialCost then
		return false, nil, string.format("Need %d %s to craft.", def.materialCost, def.materialId)
	end

	profile.yen -= (def.yenCost or 0)
	profile.bossTokens -= (def.bossTokenCost or 0)
	if (def.materialCost or 0) > 0 then
		pAny.materials[def.materialId] = (mats[def.materialId] or 0) - def.materialCost
	end
	pAny.charms[charmId] = true

	return true, { crafted = charmId, charms = pAny.charms }, nil
end

function CharmService.equipCharms(
	profile: DataService.PlayerProfile,
	charmIds: { string }
): (boolean, { [string]: any }?, string?)
	if type(charmIds) ~= "table" then
		return false, nil, "Invalid charm list."
	end
	local pAny = profile :: any
	local owned = if type(pAny.charms) == "table" then pAny.charms else {}
	local equipped: { string } = {}
	local seen: { [string]: boolean } = {}
	for _, id in ipairs(charmIds) do
		if #equipped >= CharmService.MAX_EQUIPPED then
			break
		end
		if type(id) == "string" and not seen[id] and owned[id] and findCharm(id) then
			seen[id] = true
			table.insert(equipped, id)
		end
	end
	pAny.equippedCharms = equipped
	return true, { equippedCharms = equipped, bonuses = CharmService.getBonuses(profile) }, nil
end

function CharmService.getBonuses(profile: DataService.PlayerProfile): {
	hpMult: number,
	atkMult: number,
	spdBonus: number,
	luckBonus: number,
	yenMult: number,
}
	local pAny = profile :: any
	local equipped = if type(pAny.equippedCharms) == "table" then pAny.equippedCharms else {}
	local owned = if type(pAny.charms) == "table" then pAny.charms else {}
	local hpBonus = 0
	local atkBonus = 0
	local spdBonus = 0
	local luckBonus = 0
	local yenBonus = 0
	for _, id in ipairs(equipped) do
		if owned[id] then
			local def = findCharm(id)
			if def then
				hpBonus += def.hpBonus or 0
				atkBonus += def.atkBonus or 0
				spdBonus += def.spdBonus or 0
				luckBonus += def.luckBonus or 0
				yenBonus += def.yenBonus or 0
			end
		end
	end
	return {
		hpMult = 1 + hpBonus,
		atkMult = 1 + atkBonus,
		spdBonus = spdBonus,
		luckBonus = luckBonus,
		yenMult = 1 + yenBonus,
	}
end

function CharmService.getStatus(profile: DataService.PlayerProfile): { [string]: any }
	local pAny = profile :: any
	return {
		catalog = getCatalog(),
		owned = pAny.charms or {},
		equipped = pAny.equippedCharms or {},
		maxEquipped = CharmService.MAX_EQUIPPED,
		bonuses = CharmService.getBonuses(profile),
	}
end

return CharmService
