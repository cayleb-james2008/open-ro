--!strict
export type WeatherDef = {
	id: string,
	name: string,
	description: string,
	luckBonus: number,
	yenMult: number,
	durationSec: number,
}

local Weather = {}

Weather.List = {
	{
		id = "Clear",
		name = "Clear Skies",
		description = "Standard summoning conditions",
		luckBonus = 0,
		yenMult = 1.0,
		durationSec = 180,
	},
	{
		id = "GoldenAura",
		name = "Golden Aura",
		description = "+50% Luck and +20% Yen drops",
		luckBonus = 0.50,
		yenMult = 1.20,
		durationSec = 180,
	},
	{
		id = "CrimsonEclipse",
		name = "Crimson Eclipse",
		description = "+100% Luck during blood-moon surge",
		luckBonus = 1.00,
		yenMult = 1.10,
		durationSec = 150,
	},
	{
		id = "StormSurge",
		name = "Storm Surge",
		description = "+35% Luck and +30% Yen surge",
		luckBonus = 0.35,
		yenMult = 1.30,
		durationSec = 180,
	},
	{
		id = "SakuraBloom",
		name = "Sakura Bloom",
		description = "+75% Luck under drifting blossoms",
		luckBonus = 0.75,
		yenMult = 1.15,
		durationSec = 180,
	},
} :: { WeatherDef }

Weather.ById = {} :: { [string]: WeatherDef }
for _, w in ipairs(Weather.List) do
	Weather.ById[w.id] = w
end

function Weather.get(id: string): WeatherDef?
	return Weather.ById[id]
end

return Weather
