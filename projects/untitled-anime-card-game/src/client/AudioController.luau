--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local SoundService = game:GetService("SoundService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local AssetIds = require(Shared:WaitForChild("AssetIds"))
local Theme = require(script.Parent:WaitForChild("Theme"))

local AudioController = {}

local CUE_MAP: { [string]: string } = {
	ui_click = "assets/audio/ui-click.wav",
	["ui-click"] = "assets/audio/ui-click.wav",
	card_flip = "assets/audio/card-flip.wav",
	["card-flip"] = "assets/audio/card-flip.wav",
	summon_roll = "assets/audio/summon-roll.wav",
	["summon-roll"] = "assets/audio/summon-roll.wav",
	legendary_drop = "assets/audio/legendary-drop.wav",
	["legendary-drop"] = "assets/audio/legendary-drop.wav",
	battle_crit = "assets/audio/battle-crit.wav",
	["battle-crit"] = "assets/audio/battle-crit.wav",
	level_up = "assets/audio/level-up.wav",
	["level-up"] = "assets/audio/level-up.wav",
}

local muted = false

local function isValidAssetId(assetId: string): boolean
	return string.match(assetId, "^rbxassetid://[1-9]%d*$") ~= nil
end

function AudioController.setMuted(isMuted: boolean)
	muted = isMuted == true
end

function AudioController.isMuted(): boolean
	return muted
end

function AudioController.play(cue: string, volume: number?)
	if muted or cue == "" then
		return
	end
	local key = CUE_MAP[cue] or cue
	local resolvedId = if isValidAssetId(key) then key else AssetIds.get(key)
	if not isValidAssetId(resolvedId) then
		return
	end
	pcall(function()
		local sound = Instance.new("Sound")
		sound.Name = "UACG_Sfx"
		sound.SoundId = resolvedId
		sound.Volume = math.clamp(volume or 0.4, 0, 1)
		sound.Parent = SoundService
		sound:Play()
		task.delay(3, function()
			if sound.Parent then
				sound:Destroy()
			end
		end)
	end)
end

function AudioController.playRollReveal(rarity: string)
	local style = Theme.getRarity(rarity)
	if style.order >= 5 then
		AudioController.play("legendary_drop", 0.55)
	else
		AudioController.play("summon_roll", 0.42)
	end
end

function AudioController.playBattleCue(kind: string)
	local lower = string.lower(kind)
	if string.find(lower, "crit", 1, true) or string.find(lower, "ult", 1, true) then
		AudioController.play("battle_crit", 0.48)
	elseif
		string.find(lower, "level", 1, true)
		or string.find(lower, "win", 1, true)
		or string.find(lower, "heal", 1, true)
	then
		AudioController.play("level_up", 0.45)
	else
		AudioController.play("ui_click", 0.32)
	end
end

return AudioController
