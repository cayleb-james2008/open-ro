-- ClientAudio.client.luau (StarterPlayer.StarterPlayerScripts.ClientAudio)
-- Preloads and plays procedural WAV game SFX from assets/audio/ with graceful fallback.

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local SoundService = game:GetService("SoundService")

local player = Players.LocalPlayer
local Remotes = ReplicatedStorage:WaitForChild("Remotes")

local AUDIO_FILES = {
    summon_roll    = "rbxasset://openro-assets/audio/summon_roll.wav",
    mythic_fanfare = "rbxasset://openro-assets/audio/mythic_fanfare.wav",
    battle_crit    = "rbxasset://openro-assets/audio/battle_crit.wav",
    potion_drink   = "rbxasset://openro-assets/audio/potion_drink.wav",
    ui_click       = "rbxasset://openro-assets/audio/ui_click.wav",
}

local soundPool = {}
local soundRoot = Instance.new("Folder")
soundRoot.Name = "ApexClientSFX"
soundRoot.Parent = SoundService

for key, uri in pairs(AUDIO_FILES) do
    local s = Instance.new("Sound")
    s.Name = key
    s.SoundId = uri
    s.Volume = if key == "ui_click" then 0.45 else 0.7
    s.Parent = soundRoot
    soundPool[key] = s
end

local muted = false

local function playCue(cueName: string)
    if muted then
        return
    end
    local s = soundPool[cueName]
    if s then
        pcall(function()
            s:Play()
        end)
    end
end

local audioBus = player:FindFirstChild("ClientAudioBus")
if not audioBus then
    audioBus = Instance.new("BindableEvent")
    audioBus.Name = "ClientAudioBus"
    audioBus.Parent = player
end

audioBus.Event:Connect(function(cmd, val)
    if cmd == "mute" then
        muted = (val == true)
    elseif typeof(cmd) == "string" then
        playCue(cmd)
    end
end)

local rollNotice = Remotes:WaitForChild("RollNotice")
rollNotice.OnClientEvent:Connect(function(payload)
    if typeof(payload) ~= "table" or not payload.ok then
        return
    end
    if payload.rarity == "Mythic" or payload.rarity == "Secret" or payload.rarity == "Legendary" then
        playCue("mythic_fanfare")
    else
        playCue("summon_roll")
    end
end)

local coreAction = Remotes:WaitForChild("CoreAction")
coreAction.OnClientEvent:Connect(function(kind)
    if kind == "potion_used" then
        playCue("potion_drink")
    end
end)
