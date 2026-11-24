--!strict
export type BannerDef = {
	id: string,
	name: string,
	worldIndex: number,
	worldId: string,
	show: string,
	costYen: number,
	costTickets: number,
	unlockWorld: number,
	featuredIds: { string },
	featuredRateBoost: number,
	description: string,
}

local Banners: { BannerDef } = {
	{
		id = "banner_w1",
		name = "Leaf Village Summons",
		worldIndex = 1,
		worldId = "w1_leaf",
		show = "Leaf Shinobi",
		costYen = 100,
		costTickets = 1,
		unlockWorld = 1,
		featuredIds = { "w1_seventh_hokage", "w1_rinnegan_shadow", "w1_ghost_uchiha" },
		featuredRateBoost = 1.25,
		description = "Standard shinobi pack featuring Leaf Village legends.",
	},
	{
		id = "banner_w2",
		name = "West City & Namek Capsule",
		worldIndex = 2,
		worldId = "w2_namek",
		show = "Z-Fighters",
		costYen = 500,
		costTickets = 1,
		unlockWorld = 2,
		featuredIds = { "w2_silver_autonomous_master", "w2_fused_god_warrior", "w2_legendary_berserker" },
		featuredRateBoost = 1.25,
		description = "High-power Z-Fighter capsule with god-ki transformations.",
	},
	{
		id = "banner_w3",
		name = "Grand Line Bounty Pack",
		worldIndex = 3,
		worldId = "w3_grandline",
		show = "Grand Line",
		costYen = 2500,
		costTickets = 2,
		unlockWorld = 3,
		featuredIds = { "w3_warrior_of_liberation", "w3_darkness_quake", "w3_red_haired_emperor" },
		featuredRateBoost = 1.25,
		description = "Voyage across the Grand Line for Emperors and Awakening fruits.",
	},
	{
		id = "banner_w4",
		name = "Tokyo Jujutsu Archive",
		worldIndex = 4,
		worldId = "w4_jujutsu",
		show = "Cursed Sorcerers",
		costYen = 10000,
		costTickets = 3,
		unlockWorld = 4,
		featuredIds = { "w4_honored_one", "w4_king_of_curses", "w4_brain_of_curses" },
		featuredRateBoost = 1.25,
		description = "Special-grade sorcerers and domain-expansion masters.",
	},
	{
		id = "banner_w5",
		name = "Soul Society Seireitei Pack",
		worldIndex = 5,
		worldId = "w5_seireitei",
		show = "Soul Society",
		costYen = 35000,
		costTickets = 4,
		unlockWorld = 5,
		featuredIds = { "w5_true_bankai_substitute", "w5_transcendent_strategist", "w5_head_captain_inferno" },
		featuredRateBoost = 1.25,
		description = "Captains and transcendent Soul Reapers from the Seireitei.",
	},
}

local ById: { [string]: BannerDef } = {}
for _, b in ipairs(Banners) do
	ById[b.id] = b
end

return {
	List = Banners,
	ById = ById,
	DefaultId = "banner_w1",
}
