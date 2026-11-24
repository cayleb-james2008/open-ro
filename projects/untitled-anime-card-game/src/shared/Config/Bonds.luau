--!strict
export type BondBonus = { atkPct: number?, hpPct: number?, spdPct: number?, critPct: number?, defPct: number? }
export type BondDef = {
	id: string,
	name: string,
	kind: "Show" | "CrossCard",
	show: string?,
	minCount: number?,
	requiredCards: { string }?,
	bonus: BondBonus,
	description: string,
}

local Bonds: { BondDef } = {
	{ id = "bond_show_w1", name = "Will of Fire", kind = "Show", show = "Leaf Shinobi", minCount = 2, bonus = { atkPct = 0.10, spdPct = 0.08 }, description = "2+ Leaf Shinobi cards: +10% ATK, +8% SPD" },
	{ id = "bond_show_w2", name = "Saiyan Pride", kind = "Show", show = "Z-Fighters", minCount = 2, bonus = { atkPct = 0.12, critPct = 0.05 }, description = "2+ Z-Fighters cards: +12% ATK, +5% CRIT" },
	{ id = "bond_show_w3", name = "Grand Fleet", kind = "Show", show = "Grand Line", minCount = 2, bonus = { hpPct = 0.12, atkPct = 0.08 }, description = "2+ Grand Line cards: +12% HP, +8% ATK" },
	{ id = "bond_show_w4", name = "Jujutsu Tech", kind = "Show", show = "Cursed Sorcerers", minCount = 2, bonus = { atkPct = 0.11, defPct = 0.08 }, description = "2+ Cursed Sorcerers cards: +11% ATK, +8% DEF" },
	{ id = "bond_show_w5", name = "Gotei 13", kind = "Show", show = "Soul Society", minCount = 2, bonus = { spdPct = 0.10, hpPct = 0.10 }, description = "2+ Soul Society cards: +10% SPD, +10% HP" },
	{ id = "bond_duo_hokage_shadow", name = "Two Halves of the Leaf", kind = "CrossCard", requiredCards = { "w1_seventh_hokage", "w1_rinnegan_shadow" }, bonus = { atkPct = 0.18, hpPct = 0.12 }, description = "Seventh Hokage + Rinnegan Shadow: +18% ATK, +12% HP" },
	{ id = "bond_duo_saiyan_rivals", name = "Limit Breaker Rivals", kind = "CrossCard", requiredCards = { "w2_silver_autonomous_master", "w2_saiyan_prince" }, bonus = { atkPct = 0.20, critPct = 0.08 }, description = "Autonomous Master + Saiyan Prince: +20% ATK, +8% CRIT" },
	{ id = "bond_duo_monster_trio", name = "Pirate King's Wings", kind = "CrossCard", requiredCards = { "w3_warrior_of_liberation", "w3_pirate_hunter", "w3_black_leg_chef" }, bonus = { atkPct = 0.22, spdPct = 0.10 }, description = "Liberation Warrior + Pirate Hunter + Black Leg: +22% ATK, +10% SPD" },
	{ id = "bond_duo_strongest", name = "Pinnacle of Curses", kind = "CrossCard", requiredCards = { "w4_honored_one", "w4_king_of_curses" }, bonus = { atkPct = 0.25, critPct = 0.10 }, description = "Honored One + King of Curses: +25% ATK, +10% CRIT" },
	{ id = "bond_duo_shinigami", name = "Bankai Resonance", kind = "CrossCard", requiredCards = { "w5_true_bankai_substitute", "w5_noble_senbonzakura" }, bonus = { atkPct = 0.18, spdPct = 0.12 }, description = "True Bankai + Noble Senbonzakura: +18% ATK, +12% SPD" },
}

local ById: { [string]: BondDef } = {}
for _, b in ipairs(Bonds) do
	ById[b.id] = b
end

return {
	List = Bonds,
	ById = ById,
}
