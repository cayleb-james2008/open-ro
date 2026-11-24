--!strict
local Economy = {}

Economy.Currencies = {
	"Yen",
	"Gems",
	"RollTickets",
	"TraitCrystals",
	"BossTokens",
	"RankedMedals",
}

Economy.StartingCurrencies = {
	Yen = 600,
	Gems = 150,
	RollTickets = 15,
	TraitCrystals = 3,
	BossTokens = 0,
	RankedMedals = 0,
}

Economy.BaseStorageCap = 150
Economy.ExtraStorageBonus = 150
Economy.MaxDeckSize = 5 -- Supports up to 5 cards in vertical slice
Economy.StandardDeckSize = 4
Economy.MaxLevel = 100
Economy.MaxStars = 5
Economy.MaxRebirths = 250

Economy.LevelCostBase = 80
Economy.LevelCostGrowth = 1.09
Economy.LevelStatGrowth = 0.045 -- +4.5% per level
Economy.AscensionStatGrowth = 0.20 -- +20% per star

Economy.AscensionDupeCosts = {
	[1] = 1,
	[2] = 2,
	[3] = 4,
	[4] = 7,
	[5] = 12,
}

Economy.RebirthBaseCost = 25000
Economy.RebirthLuckPerCount = 0.15
Economy.RebirthStatPerCount = 0.10

Economy.RollCooldowns = {
	Normal = 1.60,
	Fast = 0.60,
	Min = 0.30,
}

Economy.Luck = {
	GroupFlat = 1.0,
	NoviceBlessingDurationSec = 1200,
	NoviceBlessingMult = 1.5,
	TicketMult = 2.0,
	PremiumMult = 1.15,
	VipMult = 1.20,
	Passes = {
		pass_lucky = 1.5,
		pass_2x_luck = 2.0,
		pass_mega_lucky = 3.0,
	},
	Potions = {
		Minor = { mult = 1.5, durationSec = 600, name = "Minor Luck Potion" },
		Major = { mult = 2.5, durationSec = 900, name = "Major Luck Potion" },
		Super = { mult = 4.0, durationSec = 1200, name = "Super Luck Potion" },
		Boss = { mult = 6.0, durationSec = 600, name = "Boss Luck Potion" },
	},
}

Economy.WeatherCycleSec = 600
Economy.WeatherActiveSec = 240

Economy.Weathers = {
	{
		id = "ChakraStorm",
		name = "Chakra Storm",
		worldId = 1,
		luckMult = 2.0,
		yenMult = 1.25,
		description = "2.0x Luck in Hidden Leaf Village & +25% Yen",
	},
	{
		id = "SupernovaFlare",
		name = "Supernova Flare",
		worldId = 2,
		luckMult = 2.0,
		shinyGoldenMult = 1.5,
		description = "2.0x Luck in West City & Namek & +50% Shiny/Golden",
	},
	{
		id = "GrandLineTempest",
		name = "Grand Line Tempest",
		worldId = 3,
		luckMult = 2.0,
		cooldownMult = 0.85,
		description = "2.0x Luck in Grand Line Seas & -15% Roll Cooldown",
	},
	{
		id = "ShibuyaEclipse",
		name = "Shibuya Eclipse",
		worldId = 0,
		luckMult = 1.0,
		rainbowVoidMult = 2.5,
		description = "2.5x Rainbow/Void Mutation odds everywhere",
	},
	{
		id = "SoulSurge",
		name = "Soul Surge",
		worldId = 5,
		luckMult = 2.0,
		crystalMult = 2.0,
		description = "2.0x Luck in Soul Society & 2x Trait Crystal drops",
	},
}

Economy.Codes = {
	LAUNCH100 = { Yen = 1500, Gems = 750, RollTickets = 10 },
	LEAFSHINOBI = { Yen = 1500, Gems = 750, RollTickets = 15 },
}

function Economy.getLevelUpCost(currentLevel: number, worldId: number?): number
	local tier = math.max(1, worldId or 1)
	return math.floor(Economy.LevelCostBase * (Economy.LevelCostGrowth ^ (currentLevel - 1)) * tier)
end

function Economy.getRebirthCost(currentRebirths: number): number
	local r = math.min(Economy.MaxRebirths, math.max(0, currentRebirths))
	return math.floor(Economy.RebirthBaseCost * (1.18 ^ math.min(r, 30)) + math.max(0, r - 30) * 150000)
end

return Economy
