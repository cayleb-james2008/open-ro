--!strict
export type CodeReward = {
	code: string,
	description: string,
	Yen: number?,
	Gems: number?,
	RollTickets: number?,
	TraitCrystals: number?,
	BossTokens: number?,
	RaidTokens: number?,
	potions: { [string]: number }?,
	expiresAt: number?,
	active: boolean,
}

local Codes = {}

Codes.List = {
	LAUNCH100 = {
		code = "LAUNCH100",
		description = "Launch Celebration Pack",
		Yen = 1500,
		Gems = 750,
		RollTickets = 10,
		active = true,
	},
	LEAFSHINOBI = {
		code = "LEAFSHINOBI",
		description = "Hidden Leaf Welcome Gift",
		Yen = 1500,
		Gems = 750,
		RollTickets = 15,
		active = true,
	},
	SAIYANPRIDE = {
		code = "SAIYANPRIDE",
		description = "World 2 Capsule Boost",
		Yen = 5000,
		Gems = 500,
		RollTickets = 10,
		TraitCrystals = 3,
		active = true,
	},
	GRANDLINE = {
		code = "GRANDLINE",
		description = "Pirate Bounty Cache",
		Yen = 8000,
		Gems = 600,
		RollTickets = 10,
		potions = { Major = 1 },
		active = true,
	},
	DOMAINEXPANSION = {
		code = "DOMAINEXPANSION",
		description = "Cursed Energy Supply",
		Gems = 800,
		TraitCrystals = 5,
		BossTokens = 2,
		active = true,
	},
	SOULREAPER = {
		code = "SOULREAPER",
		description = "Soul Society Supply Drop",
		Yen = 12000,
		Gems = 1000,
		RollTickets = 15,
		potions = { Super = 1 },
		active = true,
	},
} :: { [string]: CodeReward }

function Codes.normalize(raw: string): string
	local trimmed = string.gsub(raw, "^%s*(.-)%s*$", "%1")
	return string.upper(trimmed)
end

function Codes.get(raw: string): CodeReward?
	local key = Codes.normalize(raw)
	local entry = Codes.List[key]
	if entry and entry.active then
		return entry
	end
	return nil
end

return Codes
