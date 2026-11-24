--!strict
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local Config = Shared:WaitForChild("Config")

local WeatherService = {}

export type WeatherDef = {
	id: string,
	name: string,
	description: string,
	luckBonus: number,
	yenMult: number,
	durationSec: number,
}

local DEFAULT_WEATHERS: { WeatherDef } = {
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
		luckBonus = 0.5,
		yenMult = 1.2,
		durationSec = 180,
	},
	{
		id = "CrimsonEclipse",
		name = "Crimson Eclipse",
		description = "+100% Luck during blood-moon surge",
		luckBonus = 1.0,
		yenMult = 1.1,
		durationSec = 150,
	},
	{
		id = "StormSurge",
		name = "Storm Surge",
		description = "+35% Luck and +30% Yen surge",
		luckBonus = 0.35,
		yenMult = 1.3,
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
}

local currentIndex = 1
local currentEndsAt = os.time() + 180
local started = false
local pushRemoteRef: RemoteEvent? = nil

local function getWeatherList(): { WeatherDef }
	local mod = Config:FindFirstChild("Weather")
	if mod and mod:IsA("ModuleScript") then
		local ok, loaded = pcall(require, mod)
		if ok and type(loaded) == "table" and type((loaded :: any).List) == "table" and #(loaded :: any).List > 0 then
			return (loaded :: any).List
		end
	end
	return DEFAULT_WEATHERS
end

function WeatherService.getCurrentWeather(): WeatherDef
	local list = getWeatherList()
	if currentIndex < 1 or currentIndex > #list then
		currentIndex = 1
	end
	return list[currentIndex]
end

function WeatherService.getLuckBonus(): number
	return WeatherService.getCurrentWeather().luckBonus or 0
end

function WeatherService.getYenMultiplier(): number
	return WeatherService.getCurrentWeather().yenMult or 1.0
end

function WeatherService.getState(): { [string]: any }
	local w = WeatherService.getCurrentWeather()
	return {
		id = w.id,
		name = w.name,
		description = w.description,
		luckBonus = w.luckBonus,
		yenMult = w.yenMult,
		endsAt = currentEndsAt,
		remainingSec = math.max(0, currentEndsAt - os.time()),
	}
end

function WeatherService.setWeather(weatherId: string): boolean
	local list = getWeatherList()
	for idx, w in ipairs(list) do
		if w.id == weatherId then
			currentIndex = idx
			currentEndsAt = os.time() + (w.durationSec or 180)
			if pushRemoteRef then
				pushRemoteRef:FireAllClients({ type = "WeatherUpdate", weather = WeatherService.getState() })
			end
			return true
		end
	end
	return false
end

function WeatherService.advance(): WeatherDef
	local list = getWeatherList()
	currentIndex = (currentIndex % #list) + 1
	local w = list[currentIndex]
	currentEndsAt = os.time() + (w.durationSec or 180)
	if pushRemoteRef then
		pushRemoteRef:FireAllClients({ type = "WeatherUpdate", weather = WeatherService.getState() })
	end
	return w
end

function WeatherService.start(pushRemote: RemoteEvent?)
	if pushRemote then
		pushRemoteRef = pushRemote
	end
	if started then
		return
	end
	started = true
	currentEndsAt = os.time() + (WeatherService.getCurrentWeather().durationSec or 180)

	task.spawn(function()
		while true do
			local waitSec = math.max(10, currentEndsAt - os.time())
			task.wait(waitSec)
			WeatherService.advance()
		end
	end)
end

return WeatherService
