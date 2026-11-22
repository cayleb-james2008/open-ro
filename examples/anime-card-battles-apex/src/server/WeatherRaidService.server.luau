-- WeatherRaidService.server.luau (ServerScriptService.WeatherRaidService)
-- Cycles 7 Weather Biomes (Clear, Rain, Snow, Sandstorm, Blood Moon, Eclipse 2x Luck, Starfall)
-- with Lighting/Atmosphere/ColorCorrection tweens and manages the Timed World Raid Boss countdown.

local Players = game:GetService("Players")
local Lighting = game:GetService("Lighting")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ServerStorage = game:GetService("ServerStorage")
local TweenService = game:GetService("TweenService")

local Modules = ReplicatedStorage:WaitForChild("Modules")
local CardDatabase = require(Modules:WaitForChild("CardDatabase"))

local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local weatherRaidEvent = Remotes:FindFirstChild("WeatherRaidEvent")
if not weatherRaidEvent then
    weatherRaidEvent = Instance.new("RemoteEvent")
    weatherRaidEvent.Name = "WeatherRaidEvent"
    weatherRaidEvent.Parent = Remotes
end

local StateFolder = ReplicatedStorage:WaitForChild("State")

local function ensureValue(className: string, name: string, defaultVal)
    local v = StateFolder:FindFirstChild(name)
    if not v then
        v = Instance.new(className)
        v.Name = name
        v.Value = defaultVal
        v.Parent = StateFolder
    end
    return v
end

local activeWeatherVal = ensureValue("StringValue", "ActiveWeather", "Clear")
local activeLuckVal = ensureValue("NumberValue", "ActiveWeatherLuck", 1.0)
local weatherTimeLeftVal = ensureValue("IntValue", "WeatherTimeLeft", 90)
local exclusiveCardVal = ensureValue("StringValue", "WeatherExclusiveCard", "")

local raidActiveVal = ensureValue("BoolValue", "RaidBossActive", false)
local raidNameVal = ensureValue("StringValue", "RaidBossName", CardDatabase.WORLD_RAID_BOSS.name)
local raidHpVal = ensureValue("IntValue", "RaidBossHP", CardDatabase.WORLD_RAID_BOSS.baseHp)
local raidMaxHpVal = ensureValue("IntValue", "RaidBossMaxHP", CardDatabase.WORLD_RAID_BOSS.baseHp)
local raidCountdownVal = ensureValue("IntValue", "RaidCountdown", 45)

local atmosphere = Lighting:FindFirstChild("ApexAtmosphere") or Lighting:FindFirstChildOfClass("Atmosphere")
if not atmosphere then
    atmosphere = Instance.new("Atmosphere")
    atmosphere.Name = "ApexAtmosphere"
    atmosphere.Parent = Lighting
end

local colorCorrection = Lighting:FindFirstChild("ApexColorCorrection") or Lighting:FindFirstChildOfClass("ColorCorrectionEffect")
if not colorCorrection then
    colorCorrection = Instance.new("ColorCorrectionEffect")
    colorCorrection.Name = "ApexColorCorrection"
    colorCorrection.Parent = Lighting
end

-- Overhead weather sky emitter anchor high above courtyard
local skyAnchor = workspace:FindFirstChild("WeatherSkyAnchor")
if not skyAnchor then
    skyAnchor = Instance.new("Part")
    skyAnchor.Name = "WeatherSkyAnchor"
    skyAnchor.Anchored = true
    skyAnchor.CanCollide = false
    skyAnchor.Transparency = 1
    skyAnchor.Size = Vector3.new(120, 1, 120)
    skyAnchor.Position = Vector3.new(0, 52, 0)
    skyAnchor.Parent = workspace
end

local skyEmitter = skyAnchor:FindFirstChild("WeatherParticleEmitter")
if not skyEmitter then
    skyEmitter = Instance.new("ParticleEmitter")
    skyEmitter.Name = "WeatherParticleEmitter"
    skyEmitter.Enabled = false
    skyEmitter.Rate = 45
    skyEmitter.Speed = NumberRange.new(18, 28)
    skyEmitter.Lifetime = NumberRange.new(2.2, 3.4)
    skyEmitter.EmissionDirection = Enum.NormalId.Bottom
    skyEmitter.Size = NumberSequence.new(0.45)
    skyEmitter.Parent = skyAnchor
end

local weatherShiftBindable = ServerStorage:FindFirstChild("ApexShiftWeather")
if not weatherShiftBindable then
    weatherShiftBindable = Instance.new("BindableFunction")
    weatherShiftBindable.Name = "ApexShiftWeather"
    weatherShiftBindable.Parent = ServerStorage
end

local raidDamageBindable = ServerStorage:FindFirstChild("ApexRaidDamage")
if not raidDamageBindable then
    raidDamageBindable = Instance.new("BindableFunction")
    raidDamageBindable.Name = "ApexRaidDamage"
    raidDamageBindable.Parent = ServerStorage
end

local rewardSignal = ServerStorage:FindFirstChild("ApexRewardSignal")
if not rewardSignal then
    rewardSignal = Instance.new("BindableEvent")
    rewardSignal.Name = "ApexRewardSignal"
    rewardSignal.Parent = ServerStorage
end

local currentWeatherIdx = 1
local WEATHER_DURATION = 90
local weatherTimer = WEATHER_DURATION

local raidParticipants = {}

local function applyWeather(idx: number)
    currentWeatherIdx = ((idx - 1) % #CardDatabase.WEATHERS) + 1
    local preset = CardDatabase.WEATHERS[currentWeatherIdx]
    activeWeatherVal.Value = preset.id
    activeLuckVal.Value = preset.luckMult
    exclusiveCardVal.Value = preset.exclusiveCard or ""
    weatherTimer = WEATHER_DURATION
    weatherTimeLeftVal.Value = weatherTimer

    local info = TweenInfo.new(3.5, Enum.EasingStyle.Sine, Enum.EasingDirection.InOut)
    TweenService:Create(Lighting, info, {
        ClockTime = preset.clockTime,
        Brightness = preset.brightness,
        OutdoorAmbient = preset.ambient,
    }):Play()
    TweenService:Create(atmosphere, info, {
        Density = preset.density,
        Haze = preset.haze,
    }):Play()
    TweenService:Create(colorCorrection, info, {
        TintColor = preset.tint,
    }):Play()

    if preset.id == "Rain" or preset.id == "Snow" or preset.id == "Sandstorm" or preset.id == "Starfall" then
        skyEmitter.Enabled = true
        if preset.id == "Rain" then
            skyEmitter.Color = ColorSequence.new(Color3.fromRGB(120, 195, 255))
            skyEmitter.Speed = NumberRange.new(35, 48)
            skyEmitter.Rate = 85
        elseif preset.id == "Snow" then
            skyEmitter.Color = ColorSequence.new(Color3.fromRGB(240, 250, 255))
            skyEmitter.Speed = NumberRange.new(7, 13)
            skyEmitter.Rate = 65
        elseif preset.id == "Sandstorm" then
            skyEmitter.Color = ColorSequence.new(Color3.fromRGB(235, 185, 110))
            skyEmitter.Speed = NumberRange.new(20, 32)
            skyEmitter.Rate = 75
        else
            skyEmitter.Color = ColorSequence.new(Color3.fromRGB(255, 224, 110))
            skyEmitter.Speed = NumberRange.new(16, 26)
            skyEmitter.Rate = 55
        end
    else
        skyEmitter.Enabled = false
    end

    weatherRaidEvent:FireAllClients("weather_changed", {
        id = preset.id,
        luckMult = preset.luckMult,
        boostElement = preset.boostElement,
        exclusiveCard = preset.exclusiveCard,
        banner = preset.banner,
        timeLeft = weatherTimer,
    })
end

local function startWorldRaid()
    raidParticipants = {}
    local playerCount = math.max(1, #Players:GetPlayers())
    local maxHp = CardDatabase.WORLD_RAID_BOSS.baseHp * playerCount
    raidMaxHpVal.Value = maxHp
    raidHpVal.Value = maxHp
    raidNameVal.Value = CardDatabase.WORLD_RAID_BOSS.name
    raidActiveVal.Value = true
    raidCountdownVal.Value = CardDatabase.WORLD_RAID_BOSS.durationSec
    weatherRaidEvent:FireAllClients("raid_started", {
        name = raidNameVal.Value,
        hp = maxHp,
        maxHp = maxHp,
        duration = CardDatabase.WORLD_RAID_BOSS.durationSec,
    })
end

local function finishWorldRaid(defeated: boolean)
    raidActiveVal.Value = false
    raidCountdownVal.Value = CardDatabase.WORLD_RAID_BOSS.cooldownSec

    for player, dmg in pairs(raidParticipants) do
        if player.Parent and dmg > 0 then
            local baseShards = if defeated then 45 else 15
            local bonusShards = math.min(180, math.floor(dmg / 250))
            local coins = math.min(2500, 150 + math.floor(dmg / 15))
            rewardSignal:Fire(player, {
                coins = coins,
                shards = baseShards + bonusShards,
                xp = 140,
                reason = if defeated then "World Raid Boss Defeated!" else "World Raid Participation",
            })
        end
    end
    raidParticipants = {}

    weatherRaidEvent:FireAllClients("raid_ended", {
        defeated = defeated,
        nextRaidSec = CardDatabase.WORLD_RAID_BOSS.cooldownSec,
    })
end

weatherShiftBindable.OnInvoke = function(_player, requestedId)
    local nextIdx = (currentWeatherIdx % #CardDatabase.WEATHERS) + 1
    if typeof(requestedId) == "string" then
        for i, w in ipairs(CardDatabase.WEATHERS) do
            if w.id == requestedId then
                nextIdx = i
                break
            end
        end
    end
    applyWeather(nextIdx)
    return CardDatabase.WEATHERS[currentWeatherIdx].id
end

raidDamageBindable.OnInvoke = function(player, dmgAmount)
    if typeof(player) ~= "Instance" or not player:IsA("Player") then
        return { active = raidActiveVal.Value, hp = raidHpVal.Value }
    end
    local dmg = math.max(0, math.floor(tonumber(dmgAmount) or 0))
    if not raidActiveVal.Value then
        return { active = false, hp = 0, maxHp = raidMaxHpVal.Value }
    end
    raidParticipants[player] = (raidParticipants[player] or 0) + dmg
    local newHp = math.max(0, raidHpVal.Value - dmg)
    raidHpVal.Value = newHp
    weatherRaidEvent:FireAllClients("raid_hp", {
        hp = newHp,
        maxHp = raidMaxHpVal.Value,
        lastHitter = player.Name,
        damage = dmg,
    })
    if newHp <= 0 then
        finishWorldRaid(true)
    end
    return { active = raidActiveVal.Value, hp = newHp, maxHp = raidMaxHpVal.Value }
end

weatherRaidEvent.OnServerEvent:Connect(function(player, action)
    if typeof(action) ~= "string" then
        return
    end
    if action == "sync" then
        local preset = CardDatabase.WEATHERS[currentWeatherIdx]
        weatherRaidEvent:FireClient(player, "sync", {
            weather = {
                id = preset.id,
                luckMult = preset.luckMult,
                boostElement = preset.boostElement,
                exclusiveCard = preset.exclusiveCard,
                banner = preset.banner,
                timeLeft = weatherTimer,
            },
            raid = {
                active = raidActiveVal.Value,
                name = raidNameVal.Value,
                hp = raidHpVal.Value,
                maxHp = raidMaxHpVal.Value,
                countdown = raidCountdownVal.Value,
            },
        })
    elseif action == "trigger_raid_horn" then
        if not raidActiveVal.Value and raidCountdownVal.Value <= 30 then
            startWorldRaid()
        end
    end
end)

Players.PlayerRemoving:Connect(function(player)
    raidParticipants[player] = nil
end)

applyWeather(1)

task.spawn(function()
    while true do
        task.wait(1)
        weatherTimer = weatherTimer - 1
        if weatherTimer <= 0 then
            applyWeather(currentWeatherIdx + 1)
        else
            weatherTimeLeftVal.Value = weatherTimer
        end

        local cd = raidCountdownVal.Value - 1
        if cd <= 0 then
            if raidActiveVal.Value then
                finishWorldRaid(false)
            else
                startWorldRaid()
            end
        else
            raidCountdownVal.Value = cd
        end
    end
end)
