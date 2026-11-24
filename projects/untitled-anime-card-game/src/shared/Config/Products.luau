--!strict
-- Draft monetization catalog matching docs/MONETIZATION.md.
-- All entries remain disabled for live sale until priceApproved is flipped to true.

export type ProductKind = "Gamepass" | "DevProduct"

export type ProductEntry = {
	id: string,
	displayName: string,
	kind: ProductKind,
	robuxPrice: number,
	priceApproved: boolean,
	robloxId: number?,
	summary: string,
}

local Products = {}

Products.PriceApproved = false
Products.PrivateServerPrice = 0
Products.PrivateServerApproved = false

Products.Gamepasses = {
	pass_extra_storage = {
		id = "pass_extra_storage",
		displayName = "Extra Card Storage (+150)",
		kind = "Gamepass",
		robuxPrice = 49,
		priceApproved = false,
		summary = "Expands card inventory cap by +150 slots (300 total).",
	},
	pass_extra_deck_slots = {
		id = "pass_extra_deck_slots",
		displayName = "+Deck Slots (+2)",
		kind = "Gamepass",
		robuxPrice = 79,
		priceApproved = false,
		summary = "Unlocks 2 extra saved Active/Defense deck presets.",
	},
	pass_lucky = {
		id = "pass_lucky",
		displayName = "Lucky (1.5x)",
		kind = "Gamepass",
		robuxPrice = 99,
		priceApproved = false,
		summary = "Permanent 1.5x multiplicative Luck on all World Banners.",
	},
	pass_fast_roll = {
		id = "pass_fast_roll",
		displayName = "Fast Roll",
		kind = "Gamepass",
		robuxPrice = 129,
		priceApproved = false,
		summary = "Cuts roll cooldown and reveal animation from 1.60s to 0.60s.",
	},
	pass_2x_luck = {
		id = "pass_2x_luck",
		displayName = "2x Luck",
		kind = "Gamepass",
		robuxPrice = 149,
		priceApproved = false,
		summary = "Permanent 2.0x multiplicative Luck on all World Banners.",
	},
	pass_auto_roll = {
		id = "pass_auto_roll",
		displayName = "Auto Roll",
		kind = "Gamepass",
		robuxPrice = 149,
		priceApproved = false,
		summary = "Continuous rolling with rarity and mutation auto-sell filters.",
	},
	pass_mega_lucky = {
		id = "pass_mega_lucky",
		displayName = "Mega Lucky (3x)",
		kind = "Gamepass",
		robuxPrice = 199,
		priceApproved = false,
		summary = "Permanent 3.0x multiplicative Luck on all World Banners.",
	},
	pass_multi_roll = {
		id = "pass_multi_roll",
		displayName = "Multi-Roll (5x)",
		kind = "Gamepass",
		robuxPrice = 299,
		priceApproved = false,
		summary = "Rolls 5 cards simultaneously per pull cycle on Yen banners.",
	},
	pass_vip = {
		id = "pass_vip",
		displayName = "VIP",
		kind = "Gamepass",
		robuxPrice = 349,
		priceApproved = false,
		summary = "1.20x Luck, +15% Potion duration, +25% Yen & Gem drops, 100 Gems daily.",
	},
	pass_mutation_hunter = {
		id = "pass_mutation_hunter",
		displayName = "Mutation & Shiny Hunter",
		kind = "Gamepass",
		robuxPrice = 449,
		priceApproved = false,
		summary = "Doubles (2.0x) Shiny, Golden, Rainbow, and Void mutation weights.",
	},
} :: { [string]: ProductEntry }

Products.DevProducts = {
	prod_starter_bundle = {
		id = "prod_starter_bundle",
		displayName = "Shinobi Starter Bundle",
		kind = "DevProduct",
		robuxPrice = 149,
		priceApproved = false,
		summary = "2,500 Gems, 25 Roll Tickets, 2x Major Luck Potions, 10,000 Yen, 1x Epic+ Card.",
	},
	prod_gems_pouch = {
		id = "prod_gems_pouch",
		displayName = "Pouch of Gems (1,200)",
		kind = "DevProduct",
		robuxPrice = 99,
		priceApproved = false,
		summary = "Grants 1,200 Gems.",
	},
	prod_gems_chest = {
		id = "prod_gems_chest",
		displayName = "Chest of Gems (3,500)",
		kind = "DevProduct",
		robuxPrice = 249,
		priceApproved = false,
		summary = "Grants 3,500 Gems.",
	},
	prod_gems_vault = {
		id = "prod_gems_vault",
		displayName = "Vault of Gems (8,000)",
		kind = "DevProduct",
		robuxPrice = 499,
		priceApproved = false,
		summary = "Grants 8,000 Gems.",
	},
	prod_gems_treasury = {
		id = "prod_gems_treasury",
		displayName = "Treasury of Gems (20,000)",
		kind = "DevProduct",
		robuxPrice = 999,
		priceApproved = false,
		summary = "Grants 20,000 Gems.",
	},
	prod_tickets_15 = {
		id = "prod_tickets_15",
		displayName = "15 Roll Tickets",
		kind = "DevProduct",
		robuxPrice = 79,
		priceApproved = false,
		summary = "Grants 15 Roll Tickets (2.0x built-in Ticket Luck).",
	},
	prod_tickets_50 = {
		id = "prod_tickets_50",
		displayName = "50 Roll Tickets",
		kind = "DevProduct",
		robuxPrice = 229,
		priceApproved = false,
		summary = "Grants 50 Roll Tickets.",
	},
	prod_tickets_150 = {
		id = "prod_tickets_150",
		displayName = "150 Roll Tickets",
		kind = "DevProduct",
		robuxPrice = 599,
		priceApproved = false,
		summary = "Grants 150 Roll Tickets.",
	},
	prod_potions_alchemist = {
		id = "prod_potions_alchemist",
		displayName = "Alchemist Luck Pack",
		kind = "DevProduct",
		robuxPrice = 99,
		priceApproved = false,
		summary = "3x Major Luck Potions + 1x Super Luck Potion.",
	},
	prod_potions_boss = {
		id = "prod_potions_boss",
		displayName = "Boss Hunter Potion Pack",
		kind = "DevProduct",
		robuxPrice = 249,
		priceApproved = false,
		summary = "3x Super Luck Potions + 2x Boss Luck Potions.",
	},
	prod_trait_crystals_25 = {
		id = "prod_trait_crystals_25",
		displayName = "25 Trait Crystals",
		kind = "DevProduct",
		robuxPrice = 149,
		priceApproved = false,
		summary = "Grants 25 Trait Crystals.",
	},
	prod_season_pass_s1 = {
		id = "prod_season_pass_s1",
		displayName = "Season 1 Pass: Dawn of Legends",
		kind = "DevProduct",
		robuxPrice = 399,
		priceApproved = false,
		summary = "Unlocks the 30-Tier Premium Battle Pass track.",
	},
} :: { [string]: ProductEntry }

function Products.getById(id: string): ProductEntry?
	return Products.Gamepasses[id] or Products.DevProducts[id]
end

function Products.isPurchaseEnabled(id: string): boolean
	local entry = Products.getById(id)
	if entry == nil then
		return false
	end
	return entry.priceApproved == true and entry.robloxId ~= nil and entry.robloxId > 0
end

return Products
