--!strict
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

local Charms = {}

Charms.List = {
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
		description = "+12% Squad ATK, +2 SPD, +10% Luck",
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
		description = "+18% Squad HP, +12% ATK, +5% Luck",
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
		description = "+20% HP, +15% ATK, +5 SPD, +8% Luck",
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
		description = "+22% ATK, +4 SPD, +15% Luck",
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
} :: { CharmDef }

Charms.ById = {} :: { [string]: CharmDef }
for _, c in ipairs(Charms.List) do
	Charms.ById[c.id] = c
end

function Charms.get(id: string): CharmDef?
	return Charms.ById[id]
end

return Charms
