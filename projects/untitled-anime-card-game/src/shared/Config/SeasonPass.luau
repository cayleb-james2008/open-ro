--!strict
export type PassReward = {
	kind: "Yen" | "Gems" | "RollTickets" | "TraitCrystals" | "BossTokens" | "RaidTokens" | "Potion",
	id: string?,
	amount: number,
	label: string,
}

export type SeasonTier = {
	tier: number,
	xpRequired: number,
	freeReward: PassReward,
	premiumReward: PassReward,
}

local SeasonPass = {}

SeasonPass.SeasonId = "season_1"
SeasonPass.SeasonName = "Season 1: Dawn of Legends"
SeasonPass.ProductId = "prod_season_pass_s1"
SeasonPass.MaxTier = 30
SeasonPass.XpPerTier = 250

local function makeReward(
	kind: "Yen" | "Gems" | "RollTickets" | "TraitCrystals" | "BossTokens" | "RaidTokens" | "Potion",
	amount: number,
	label: string,
	id: string?
): PassReward
	return {
		kind = kind,
		id = id,
		amount = amount,
		label = label,
	}
end

SeasonPass.Tiers = {} :: { SeasonTier }

for tier = 1, SeasonPass.MaxTier do
	local freeReward: PassReward
	local premiumReward: PassReward

	if tier == 30 then
		freeReward = makeReward("RollTickets", 25, "25 Roll Tickets")
		premiumReward = makeReward("Gems", 3000, "3,000 Gems")
	elseif tier == 20 then
		freeReward = makeReward("Gems", 1000, "1,000 Gems")
		premiumReward = makeReward("Potion", 2, "2x Boss Luck Potion", "Boss")
	elseif tier == 10 then
		freeReward = makeReward("TraitCrystals", 5, "5 Trait Crystals")
		premiumReward = makeReward("RollTickets", 20, "20 Roll Tickets")
	elseif tier % 5 == 0 then
		freeReward = makeReward(
			"RollTickets",
			5 + math.floor(tier / 5) * 2,
			string.format("%d Roll Tickets", 5 + math.floor(tier / 5) * 2)
		)
		premiumReward = makeReward(
			"TraitCrystals",
			3 + math.floor(tier / 5),
			string.format("%d Trait Crystals", 3 + math.floor(tier / 5))
		)
	elseif tier % 3 == 0 then
		freeReward = makeReward("Gems", 150 + tier * 20, string.format("%d Gems", 150 + tier * 20))
		premiumReward = makeReward("Potion", 1, "Super Luck Potion", "Super")
	elseif tier % 2 == 0 then
		freeReward = makeReward("Yen", 1500 * tier, string.format("%d Yen", 1500 * tier))
		premiumReward = makeReward("Gems", 300 + tier * 25, string.format("%d Gems", 300 + tier * 25))
	else
		freeReward = makeReward(
			"RollTickets",
			2 + math.floor(tier / 6),
			string.format("%d Roll Tickets", 2 + math.floor(tier / 6))
		)
		premiumReward = makeReward(
			"BossTokens",
			2 + math.floor(tier / 10),
			string.format("%d Boss Tokens", 2 + math.floor(tier / 10))
		)
	end

	table.insert(SeasonPass.Tiers, {
		tier = tier,
		xpRequired = tier * SeasonPass.XpPerTier,
		freeReward = freeReward,
		premiumReward = premiumReward,
	})
end

function SeasonPass.getTierForXp(xp: number): number
	local clamped = math.max(0, math.floor(xp))
	return math.min(SeasonPass.MaxTier, math.floor(clamped / SeasonPass.XpPerTier))
end

function SeasonPass.getTierDef(tier: number): SeasonTier?
	return SeasonPass.Tiers[tier]
end

return SeasonPass
