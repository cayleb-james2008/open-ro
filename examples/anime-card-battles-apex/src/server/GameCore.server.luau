-- GameCore.server.luau (ServerScriptService.GameCore)
-- Server-authoritative core loop: Gacha Rolling, Packs, Pity, XP/Upgrades,
-- Potions, Traits, Charms, Locking, Auto-Delete, 4-Slot Deck, Codes,
-- Overhead 3D Showcase Card, and Workspace Landmark Prompts.

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ServerStorage = game:GetService("ServerStorage")
local RunService = game:GetService("RunService")

local Modules = ReplicatedStorage:WaitForChild("Modules")
local CardDatabase = require(Modules:WaitForChild("CardDatabase"))
local CombatMath = require(Modules:WaitForChild("CombatMath"))

local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local StateFolder = ReplicatedStorage:WaitForChild("State")

local function ensureRemote(className: string, name: string)
    local r = Remotes:FindFirstChild(name)
    if not r then
        r = Instance.new(className)
        r.Name = name
        r.Parent = Remotes
    end
    return r
end

local rollFunction = ensureRemote("RemoteFunction", "RollRequest")
local rollNotice = ensureRemote("RemoteEvent", "RollNotice")
local coreAction = ensureRemote("RemoteEvent", "CoreAction")
local stateSync = ensureRemote("RemoteEvent", "StateSync")
local openPanelEvent = ensureRemote("RemoteEvent", "OpenPanelEvent")

local storageBridge = ServerStorage:WaitForChild("ApexStorageBridge")

local profileQuery = ServerStorage:FindFirstChild("ApexProfileQuery")
if not profileQuery then
    profileQuery = Instance.new("BindableFunction")
    profileQuery.Name = "ApexProfileQuery"
    profileQuery.Parent = ServerStorage
end

local rewardSignal = ServerStorage:FindFirstChild("ApexRewardSignal")
if not rewardSignal then
    rewardSignal = Instance.new("BindableEvent")
    rewardSignal.Name = "ApexRewardSignal"
    rewardSignal.Parent = ServerStorage
end

local shiftWeather = ServerStorage:WaitForChild("ApexShiftWeather")

local rng = Random.new()
local profiles = {}
local rateBuckets = {}
local overheadCards = {}

local PITY_THRESHOLD = 50
local BASE_INVENTORY_CAP = 32
local DEFAULT_FAST_COOLDOWN = 0.35

local function consumeToken(player: Player, cost: number): boolean
    local now = os.clock()
    local b = rateBuckets[player]
    if not b then
        b = { tokens = 12, updated = now }
        rateBuckets[player] = b
    end
    b.tokens = math.min(12, b.tokens + (now - b.updated) * 6)
    b.updated = now
    if b.tokens < cost then
        return false
    end
    b.tokens = b.tokens - cost
    return true
end

local function ensureLeaderstats(player: Player)
    local stats = player:FindFirstChild("leaderstats")
    if not stats then
        stats = Instance.new("Folder")
        stats.Name = "leaderstats"
        stats.Parent = player
    end
    local function ensureInt(name: string, val: number)
        local iv = stats:FindFirstChild(name)
        if not iv then
            iv = Instance.new("IntValue")
            iv.Name = name
            iv.Value = val
            iv.Parent = stats
        end
        return iv
    end
    local coins = ensureInt("Coins", 400)
    local floor = ensureInt("TowerFloor", 1)
    local power = ensureInt("Power", 0)
    return coins, floor, power
end

local function getInventoryCap(profile): number
    local capLvl = (profile.upgrades and profile.upgrades.Cap) or 0
    return BASE_INVENTORY_CAP + capLvl * 10
end

local function computeEffectiveLuck(player: Player, profile): number
    local luckLvl = (profile.upgrades and profile.upgrades.Luck) or 0
    local upgMult = 1 + luckLvl * 0.20
    local levelMult = 1 + math.max(0, (profile.level or 1) - 1) * 0.05
    local rebirthMult = 1 + (profile.rebirths or 0) * 0.30

    local now = os.time()
    local potionLuck = 1.0
    if profile.buffs and profile.buffs.luckUntil and profile.buffs.luckUntil > now then
        potionLuck = math.max(1.0, profile.buffs.luckMult or 1.0)
    end

    local weatherLuck = 1.0
    local wVal = StateFolder:FindFirstChild("ActiveWeatherLuck")
    if wVal and wVal:IsA("NumberValue") and wVal.Value > 0 then
        weatherLuck = wVal.Value
    end

    local charmLuck = 0.0
    for _, cardId in ipairs(profile.deck) do
        local owned = profile.owned[cardId]
        if owned and owned.charm then
            local ch = CardDatabase.CHARM_BY_ID[owned.charm]
            if ch then
                charmLuck = charmLuck + ch.luckBonus
            end
        end
    end

    local total = upgMult * levelMult * rebirthMult * potionLuck * weatherLuck * (1 + charmLuck)
    player:SetAttribute("EffectiveLuck", math.floor(total * 100) / 100)
    return total
end

local function computeRollSpeed(profile): number
    local spdLvl = (profile.upgrades and profile.upgrades.Speed) or 0
    local upgMult = 1 + spdLvl * 0.12
    local now = os.time()
    local potionSpd = 1.0
    if profile.buffs and profile.buffs.speedUntil and profile.buffs.speedUntil > now then
        potionSpd = math.max(1.0, profile.buffs.speedMult or 1.0)
    end
    return upgMult * potionSpd
end

local function computeTeamPower(player: Player, profile): number
    local total = 0
    for _, cardId in ipairs(profile.deck) do
        local cardDef = CardDatabase.CARD_BY_ID[cardId]
        local entry = profile.owned[cardId]
        if cardDef and entry then
            total = total + CombatMath.cardPower(cardDef, entry)
        end
    end
    local uniqueCount = 0
    for _ in pairs(profile.index) do
        uniqueCount = uniqueCount + 1
    end
    local indexMult = 1 + uniqueCount * 0.02
    local rebirthMult = 1 + (profile.rebirths or 0) * 0.20
    local finalPower = math.floor(total * indexMult * rebirthMult)
    player:SetAttribute("TeamPower", finalPower)
    local stats = player:FindFirstChild("leaderstats")
    local pStat = stats and stats:FindFirstChild("Power")
    if pStat and pStat:IsA("IntValue") then
        pStat.Value = finalPower
    end
    return finalPower
end

local function updateOverheadCard(player: Player, profile)
    local char = player.Character
    if not char then
        return
    end
    local head = char:FindFirstChild("Head") or char:FindFirstChild("HumanoidRootPart")
    if not head or not head:IsA("BasePart") then
        return
    end

    local old = overheadCards[player]
    if old then
        old:Destroy()
        overheadCards[player] = nil
    end

    local leadId = profile.deck[1]
    if not leadId then
        return
    end
    local cardDef = CardDatabase.CARD_BY_ID[leadId]
    if not cardDef then
        return
    end
    local rarityInfo = CardDatabase.RARITY_BY_ID[cardDef.rarity] or CardDatabase.RARITIES[1]

    local model = Instance.new("Model")
    model.Name = "EquippedShowcaseCard"

    local board = Instance.new("Part")
    board.Name = "CardBoard"
    board.Size = Vector3.new(2.2, 3.0, 0.22)
    board.Anchored = false
    board.CanCollide = false
    board.Massless = true
    board.Material = Enum.Material.SmoothPlastic
    board.Color = rarityInfo.color
    board.CFrame = head.CFrame * CFrame.new(2.2, 2.1, 0)
    board.Parent = model

    local decal = Instance.new("Decal")
    decal.Name = "ArtDecal"
    decal.Face = Enum.NormalId.Front
    decal.Texture = cardDef.portrait
    decal.Parent = board

    local sgui = Instance.new("SurfaceGui")
    sgui.Name = "ArtSurfaceGui"
    sgui.Face = Enum.NormalId.Front
    local img = Instance.new("ImageLabel")
    img.Size = UDim2.new(1, 0, 1, 0)
    img.BackgroundTransparency = 1
    img.Image = cardDef.portrait
    img.ScaleType = Enum.ScaleType.Crop
    img.Parent = sgui
    sgui.Parent = board

    local hl = Instance.new("Highlight")
    hl.FillTransparency = 0.85
    hl.OutlineColor = rarityInfo.color
    hl.OutlineTransparency = 0.1
    hl.Parent = board

    local att0 = Instance.new("Attachment")
    att0.Position = Vector3.new(2.2, 2.2, 0)
    att0.Parent = head

    local att1 = Instance.new("Attachment")
    att1.Parent = board

    local alignPos = Instance.new("AlignPosition")
    alignPos.Attachment0 = att1
    alignPos.Attachment1 = att0
    alignPos.MaxForce = 10000
    alignPos.Responsiveness = 18
    alignPos.Parent = board

    local alignOri = Instance.new("AlignOrientation")
    alignOri.Attachment0 = att1
    alignOri.Attachment1 = att0
    alignOri.MaxTorque = 10000
    alignOri.Responsiveness = 15
    alignOri.Parent = board

    model.Parent = char
    overheadCards[player] = model
end

local function makeSnapshot(profile)
    return {
        coins = profile.coins,
        shards = profile.shards,
        raidTokens = profile.raidTokens,
        level = profile.level,
        xp = profile.xp,
        pity = profile.pity,
        totalRolls = profile.totalRolls,
        rebirths = profile.rebirths,
        towerFloor = profile.towerFloor,
        storyCleared = profile.storyCleared,
        selectedPack = profile.selectedPack,
        autoDelete = profile.autoDelete,
        upgrades = profile.upgrades,
        owned = profile.owned,
        deck = profile.deck,
        index = profile.index,
        charmsOwned = profile.charmsOwned,
        codes = profile.codes,
    }
end

local function syncPlayer(player: Player)
    local profile = profiles[player]
    if not profile then
        return
    end
    local coinsStat, floorStat = ensureLeaderstats(player)
    coinsStat.Value = profile.coins
    floorStat.Value = profile.towerFloor

    player:SetAttribute("TowerFloor", profile.towerFloor)
    player:SetAttribute("Rebirths", profile.rebirths)
    player:SetAttribute("RollLevel", profile.level)
    player:SetAttribute("Shards", profile.shards)
    player:SetAttribute("PityCount", profile.pity)

    local luck = computeEffectiveLuck(player, profile)
    local power = computeTeamPower(player, profile)

    pcall(function()
        storageBridge:Invoke("stage", player.UserId, makeSnapshot(profile))
    end)

    stateSync:FireClient(player, {
        coins = profile.coins,
        shards = profile.shards,
        raidTokens = profile.raidTokens,
        level = profile.level,
        xp = profile.xp,
        xpMax = CombatMath.xpForNextLevel(profile.level),
        pity = profile.pity,
        pityMax = PITY_THRESHOLD,
        totalRolls = profile.totalRolls,
        rebirths = profile.rebirths,
        towerFloor = profile.towerFloor,
        storyCleared = profile.storyCleared,
        selectedPack = profile.selectedPack,
        autoDelete = profile.autoDelete,
        upgrades = profile.upgrades,
        owned = profile.owned,
        deck = profile.deck,
        index = profile.index,
        charmsOwned = profile.charmsOwned,
        buffs = profile.buffs,
        luck = luck,
        speedMult = computeRollSpeed(profile),
        teamPower = power,
        inventoryCap = getInventoryCap(profile),
        autoRoll = profile.autoRoll,
    })
end

local function defaultProfile()
    return {
        coins = 450,
        shards = 35,
        raidTokens = 0,
        level = 1,
        xp = 0,
        pity = 0,
        totalRolls = 0,
        rebirths = 0,
        towerFloor = 1,
        storyCleared = 0,
        selectedPack = "Starter",
        autoDelete = {},
        upgrades = { Luck = 0, Speed = 0, Cap = 0 },
        owned = {
            ["eclipse-arcane-alchemist"] = {
                card_id = "eclipse-arcane-alchemist",
                border = "Normal",
                border_mult = 1.0,
                stars = 1,
                copies = 1,
                locked = true,
                trait = nil,
                charm = nil,
            },
            ["ember-acolyte"] = {
                card_id = "ember-acolyte",
                border = "Normal",
                border_mult = 1.0,
                stars = 1,
                copies = 1,
                locked = false,
                trait = nil,
                charm = nil,
            },
        },
        deck = { "eclipse-arcane-alchemist", "ember-acolyte" },
        index = {
            ["eclipse-arcane-alchemist"] = true,
            ["ember-acolyte"] = true,
        },
        charmsOwned = {},
        codes = {},
        buffs = { luckMult = 1.0, luckUntil = 0, speedMult = 1.0, speedUntil = 0 },
        autoRoll = false,
        lastRollAt = 0,
    }
end

local function addXp(profile, amount: number)
    profile.xp = profile.xp + amount
    local leveled = false
    while profile.xp >= CombatMath.xpForNextLevel(profile.level) do
        profile.xp = profile.xp - CombatMath.xpForNextLevel(profile.level)
        profile.level = profile.level + 1
        profile.coins = profile.coins + 120 + profile.level * 25
        profile.shards = profile.shards + 12
        leveled = true
    end
    return leveled
end

local function rollBorder(luck: number)
    local draw = rng:NextNumber()
    for i = #CardDatabase.BORDERS, 1, -1 do
        local b = CardDatabase.BORDERS[i]
        local threshold = math.min(0.95, b.chance * math.sqrt(luck))
        if b.id == "Normal" or draw <= threshold then
            return b.id, b.mult
        end
    end
    return "Normal", 1.0
end

local function rollRarity(profile, luck: number): string
    if profile.pity >= PITY_THRESHOLD * 2 then
        profile.pity = 0
        return "Secret"
    end
    if profile.pity >= PITY_THRESHOLD then
        profile.pity = 0
        return "Mythic"
    end
    for i = #CardDatabase.RARITIES, 1, -1 do
        local r = CardDatabase.RARITIES[i]
        if r.id == "Common" then
            return "Common"
        end
        local odds = math.min(0.65, luck / r.denom)
        if rng:NextNumber() <= odds then
            if r.id == "Mythic" or r.id == "Secret" then
                profile.pity = 0
            end
            return r.id
        end
    end
    return "Common"
end

local function pickCardFromPack(packId: string, rarityId: string, activeWeather: string)
    local exact = {}
    local packPool = {}
    local rarityPool = {}
    for _, c in ipairs(CardDatabase.CARDS) do
        local weatherOk = (c.weatherLock == nil) or (c.weatherLock == activeWeather)
        if weatherOk then
            if c.pack == packId and c.rarity == rarityId then
                table.insert(exact, c)
            end
            if c.pack == packId then
                table.insert(packPool, c)
            end
            if c.rarity == rarityId then
                table.insert(rarityPool, c)
            end
        end
    end

    -- Give weather-exclusive cards a 25% spotlight chance when their biome is active
    local wDef = CardDatabase.WEATHER_BY_ID[activeWeather]
    if wDef and wDef.exclusiveCard and rng:NextNumber() <= 0.22 then
        local ex = CardDatabase.CARD_BY_ID[wDef.exclusiveCard]
        if ex and (ex.rarity == rarityId or rarityId == "Common" or rarityId == "Rare") then
            return ex
        end
    end

    if #exact > 0 then
        return exact[rng:NextInteger(1, #exact)]
    end
    if #rarityPool > 0 then
        return rarityPool[rng:NextInteger(1, #rarityPool)]
    end
    if #packPool > 0 then
        return packPool[rng:NextInteger(1, #packPool)]
    end
    return CardDatabase.CARDS[1]
end

local function countOwnedUnique(profile): number
    local n = 0
    for _ in pairs(profile.owned) do
        n = n + 1
    end
    return n
end

local function performRoll(player: Player, fastMode: boolean, requestedPack: string?)
    local profile = profiles[player]
    if not profile then
        return { ok = false, reason = "loading" }
    end

    local packId = profile.selectedPack or "Starter"
    if typeof(requestedPack) == "string" and CardDatabase.PACK_BY_ID[requestedPack] then
        packId = requestedPack
        profile.selectedPack = packId
    end
    local pack = CardDatabase.PACK_BY_ID[packId] or CardDatabase.PACKS[1]

    local now = os.clock()
    local spd = computeRollSpeed(profile)
    local minDelay = (if fastMode then DEFAULT_FAST_COOLDOWN else DEFAULT_FAST_COOLDOWN * 1.45) / spd
    if (now - profile.lastRollAt) < minDelay then
        return { ok = false, reason = "cooldown" }
    end

    if profile.coins < pack.cost then
        -- Fall back cleanly to free Starter Pack if auto-rolling without enough coins
        if pack.cost > 0 then
            return { ok = false, reason = "insufficient_coins", needed = pack.cost }
        end
    end

    profile.lastRollAt = now
    profile.coins = profile.coins - pack.cost
    profile.totalRolls = profile.totalRolls + 1
    profile.pity = profile.pity + 1

    local wVal = StateFolder:FindFirstChild("ActiveWeather")
    local activeWeather = if wVal and wVal:IsA("StringValue") then wVal.Value else "Clear"

    local luck = computeEffectiveLuck(player, profile) * pack.luckBonus
    local rarityId = rollRarity(profile, luck)
    local card = pickCardFromPack(pack.id, rarityId, activeWeather)
    local borderId, borderMult = rollBorder(luck)
    local rarityMeta = CardDatabase.RARITY_BY_ID[card.rarity] or CardDatabase.RARITIES[1]

    local isNew = not profile.index[card.id]
    profile.index[card.id] = true

    local existing = profile.owned[card.id]
    local autoDeleted = false
    if profile.autoDelete[card.rarity] and not (existing and existing.locked) and borderId == "Normal" then
        autoDeleted = true
        profile.shards = profile.shards + rarityMeta.shards
        profile.coins = profile.coins + rarityMeta.shards * 4
    elseif existing then
        existing.copies = (existing.copies or 1) + 1
        if borderMult > (existing.border_mult or 1.0) then
            existing.border = borderId
            existing.border_mult = borderMult
        end
    else
        if countOwnedUnique(profile) < getInventoryCap(profile) then
            profile.owned[card.id] = {
                card_id = card.id,
                border = borderId,
                border_mult = borderMult,
                stars = 1,
                copies = 1,
                locked = (card.rarity == "Mythic" or card.rarity == "Secret"),
                trait = nil,
                charm = nil,
            }
            if #profile.deck < 4 then
                table.insert(profile.deck, card.id)
                updateOverheadCard(player, profile)
            end
        else
            autoDeleted = true
            profile.shards = profile.shards + rarityMeta.shards
        end
    end

    local leveledUp = addXp(profile, rarityMeta.xp)
    local globalVal = StateFolder:FindFirstChild("GlobalRolls")
    if globalVal and globalVal:IsA("IntValue") then
        globalVal.Value = globalVal.Value + 1
    end

    local entry = profile.owned[card.id]
    local stats = CombatMath.computeCardStats(card, entry)

    local payload = {
        ok = true,
        card_id = card.id,
        title = card.title,
        rarity = card.rarity,
        odds = rarityMeta.oddsText,
        element = card.element,
        pack = card.pack,
        portrait = card.portrait,
        skillName = card.skillName,
        skillKind = card.skillKind,
        passive = card.passive,
        border = borderId,
        border_mult = borderMult,
        atk = stats.atk,
        hp = stats.hp,
        speed = stats.speed,
        isNew = isNew,
        autoDeleted = autoDeleted,
        leveledUp = leveledUp,
        level = profile.level,
        pity = profile.pity,
        pityMax = PITY_THRESHOLD,
        luck = luck,
    }

    syncPlayer(player)
    rollNotice:FireClient(player, payload)
    return payload
end

local function autoEquipBest(player: Player, profile)
    local list = {}
    for cardId, entry in pairs(profile.owned) do
        local def = CardDatabase.CARD_BY_ID[cardId]
        if def then
            table.insert(list, { id = cardId, power = CombatMath.cardPower(def, entry) })
        end
    end
    table.sort(list, function(a, b)
        return a.power > b.power
    end)
    profile.deck = {}
    for i = 1, math.min(4, #list) do
        table.insert(profile.deck, list[i].id)
    end
    updateOverheadCard(player, profile)
end

local function pickRandomTrait()
    local total = 0
    for _, t in ipairs(CardDatabase.TRAITS) do
        total = total + t.weight
    end
    local r = rng:NextNumber() * total
    local acc = 0
    for _, t in ipairs(CardDatabase.TRAITS) do
        acc = acc + t.weight
        if r <= acc then
            return t.id
        end
    end
    return CardDatabase.TRAITS[1].id
end

rollFunction.OnServerInvoke = function(player, action, arg)
    if not consumeToken(player, 1) then
        return { ok = false, reason = "rate_limit" }
    end
    if typeof(action) ~= "string" then
        return { ok = false, reason = "invalid_action" }
    end
    local packArg = if typeof(arg) == "string" then arg else nil

    local profile = profiles[player]
    if not profile then
        return { ok = false, reason = "loading" }
    end

    if action == "roll" then
        return performRoll(player, false, packArg)
    elseif action == "fast_roll" then
        return performRoll(player, true, packArg)
    elseif action == "auto_toggle" then
        profile.autoRoll = not profile.autoRoll
        syncPlayer(player)
        return { ok = true, autoRoll = profile.autoRoll }
    elseif action == "select_pack" and packArg and CardDatabase.PACK_BY_ID[packArg] then
        profile.selectedPack = packArg
        syncPlayer(player)
        return { ok = true, selectedPack = packArg }
    elseif action == "status" then
        syncPlayer(player)
        return { ok = true, pity = profile.pity, pityMax = PITY_THRESHOLD }
    end
    return { ok = false, reason = "unknown_action" }
end

coreAction.OnServerEvent:Connect(function(player, action, arg1, arg2)
    if not consumeToken(player, 1) then
        return
    end
    if typeof(action) ~= "string" or #action > 40 then
        return
    end
    local profile = profiles[player]
    if not profile then
        return
    end

    if action == "sync" then
        syncPlayer(player)
        return
    elseif action == "auto_equip" then
        autoEquipBest(player, profile)
        syncPlayer(player)
        return
    elseif action == "rebirth" then
        if profile.towerFloor >= 10 then
            profile.rebirths = profile.rebirths + 1
            profile.towerFloor = 1
            profile.coins = profile.coins + 600
            profile.shards = profile.shards + 30
            syncPlayer(player)
            coreAction:FireClient(player, "toast", "Rebirth #" .. profile.rebirths .. " Ascended! (+30% Luck & +20% Power)")
        else
            coreAction:FireClient(player, "toast", "Reach Tower Floor 10 to Rebirth!")
        end
        return
    elseif action == "select_pack" then
        if typeof(arg1) == "string" and CardDatabase.PACK_BY_ID[arg1] then
            profile.selectedPack = arg1
            syncPlayer(player)
        end
        return
    elseif action == "toggle_autodelete" then
        if typeof(arg1) == "string" and CardDatabase.RARITY_BY_ID[arg1] and arg1 ~= "Mythic" and arg1 ~= "Secret" then
            profile.autoDelete[arg1] = not profile.autoDelete[arg1]
            syncPlayer(player)
        end
        return
    elseif action == "buy_upgrade" then
        if typeof(arg1) ~= "string" then
            return
        end
        for _, upg in ipairs(CardDatabase.UPGRADES) do
            if upg.id == arg1 then
                local cur = profile.upgrades[arg1] or 0
                if cur >= upg.maxLevel then
                    coreAction:FireClient(player, "toast", upg.name .. " is MAX level!")
                    return
                end
                local cost = CombatMath.getUpgradeCost(arg1, cur)
                if profile.coins < cost then
                    coreAction:FireClient(player, "toast", "Need " .. cost .. " Coins for " .. upg.name)
                    return
                end
                profile.coins = profile.coins - cost
                profile.upgrades[arg1] = cur + 1
                syncPlayer(player)
                coreAction:FireClient(player, "toast", upg.name .. " upgraded to Lv." .. (cur + 1) .. "!")
                return
            end
        end
        return
    elseif action == "buy_potion" then
        if typeof(arg1) ~= "string" then
            return
        end
        for _, pot in ipairs(CardDatabase.POTIONS) do
            if pot.id == arg1 then
                if profile.coins < pot.cost then
                    coreAction:FireClient(player, "toast", "Need " .. pot.cost .. " Coins for " .. pot.name)
                    return
                end
                profile.coins = profile.coins - pot.cost
                if pot.id == "WeatherReroll" then
                    local nextW = shiftWeather:Invoke(player, nil)
                    coreAction:FireClient(player, "toast", "Weather shifted to " .. tostring(nextW) .. "!")
                else
                    local now = os.time()
                    if pot.luckMult > 1.0 then
                        profile.buffs.luckMult = math.max(profile.buffs.luckMult or 1.0, pot.luckMult)
                        profile.buffs.luckUntil = math.max(profile.buffs.luckUntil or now, now) + pot.duration
                    end
                    if pot.speedMult > 1.0 then
                        profile.buffs.speedMult = math.max(profile.buffs.speedMult or 1.0, pot.speedMult)
                        profile.buffs.speedUntil = math.max(profile.buffs.speedUntil or now, now) + pot.duration
                    end
                    coreAction:FireClient(player, "potion_used", pot.name)
                end
                syncPlayer(player)
                return
            end
        end
        return
    elseif action == "redeem_code" then
        if typeof(arg1) ~= "string" or #arg1 == 0 or #arg1 > 32 then
            return
        end
        local code = string.upper(arg1)
        local reward = CardDatabase.CODES[code]
        if not reward then
            coreAction:FireClient(player, "toast", "Invalid code: " .. code)
            return
        end
        if profile.codes[code] then
            coreAction:FireClient(player, "toast", "Code already redeemed: " .. code)
            return
        end
        profile.codes[code] = true
        profile.coins = profile.coins + reward.coins
        profile.shards = profile.shards + reward.shards
        local now = os.time()
        if reward.luckSec > 0 then
            profile.buffs.luckMult = math.max(profile.buffs.luckMult or 1.0, reward.luckMult)
            profile.buffs.luckUntil = math.max(profile.buffs.luckUntil or now, now) + reward.luckSec
        end
        if reward.speedSec > 0 then
            profile.buffs.speedMult = math.max(profile.buffs.speedMult or 1.0, reward.speedMult)
            profile.buffs.speedUntil = math.max(profile.buffs.speedUntil or now, now) + reward.speedSec
        end
        syncPlayer(player)
        coreAction:FireClient(player, "toast", "Redeemed " .. code .. "! +" .. reward.coins .. " Coins & +" .. reward.shards .. " Shards")
        return
    elseif action == "craft_charm" then
        if typeof(arg1) ~= "string" then
            return
        end
        local ch = CardDatabase.CHARM_BY_ID[arg1]
        if not ch then
            return
        end
        if profile.charmsOwned[ch.id] then
            coreAction:FireClient(player, "toast", ch.name .. " already crafted!")
            return
        end
        if profile.shards < ch.shardCost then
            coreAction:FireClient(player, "toast", "Need " .. ch.shardCost .. " Shards for " .. ch.name)
            return
        end
        profile.shards = profile.shards - ch.shardCost
        profile.charmsOwned[ch.id] = true
        syncPlayer(player)
        coreAction:FireClient(player, "toast", "Crafted Charm: " .. ch.name .. "!")
        return
    end

    -- Actions below target a specific owned cardId
    if typeof(arg1) ~= "string" or #arg1 == 0 or #arg1 > 64 then
        return
    end
    local entry = profile.owned[arg1]
    if not entry then
        return
    end

    if action == "equip" then
        for _, id in ipairs(profile.deck) do
            if id == arg1 then
                return
            end
        end
        if #profile.deck < 4 then
            table.insert(profile.deck, arg1)
            updateOverheadCard(player, profile)
            syncPlayer(player)
        else
            coreAction:FireClient(player, "toast", "4-Slot Deck full! Unequip a card first.")
        end
    elseif action == "unequip" then
        for idx, id in ipairs(profile.deck) do
            if id == arg1 then
                table.remove(profile.deck, idx)
                updateOverheadCard(player, profile)
                syncPlayer(player)
                return
            end
        end
    elseif action == "toggle_lock" then
        entry.locked = not entry.locked
        syncPlayer(player)
    elseif action == "fuse" then
        if (entry.stars or 1) < 5 and (entry.copies or 1) >= 3 then
            entry.copies = entry.copies - 2
            entry.stars = (entry.stars or 1) + 1
            syncPlayer(player)
            coreAction:FireClient(player, "toast", "Star Fused to " .. entry.stars .. "★!")
        else
            coreAction:FireClient(player, "toast", "Need 3 total copies to Star Fuse (max 5★).")
        end
    elseif action == "reroll_trait" then
        local cost = 15
        if profile.shards < cost then
            coreAction:FireClient(player, "toast", "Need " .. cost .. " Shards to reroll Trait!")
            return
        end
        profile.shards = profile.shards - cost
        entry.trait = pickRandomTrait()
        syncPlayer(player)
        coreAction:FireClient(player, "toast", "Rolled Trait: " .. entry.trait .. "!")
    elseif action == "equip_charm" then
        if typeof(arg2) ~= "string" then
            return
        end
        if arg2 == "None" then
            entry.charm = nil
            syncPlayer(player)
            return
        end
        if profile.charmsOwned[arg2] then
            for _, other in pairs(profile.owned) do
                if other.charm == arg2 then
                    other.charm = nil
                end
            end
            entry.charm = arg2
            syncPlayer(player)
            coreAction:FireClient(player, "toast", "Equipped Charm: " .. arg2)
        end
    end
end)

profileQuery.OnInvoke = function(op, player)
    if typeof(player) ~= "Instance" or not player:IsA("Player") then
        return nil
    end
    local profile = profiles[player]
    if not profile then
        return nil
    end
    if op == "deck_units" then
        local units = {}
        for _, cardId in ipairs(profile.deck) do
            local def = CardDatabase.CARD_BY_ID[cardId]
            local entry = profile.owned[cardId]
            if def and entry then
                local st = CombatMath.computeCardStats(def, entry)
                table.insert(units, {
                    card_id = def.id,
                    title = def.title,
                    rarity = def.rarity,
                    element = def.element,
                    portrait = def.portrait,
                    skillName = def.skillName,
                    skillKind = def.skillKind,
                    cooldown = math.max(1, def.cooldown - st.cdReduce),
                    atk = st.atk,
                    hp = st.hp,
                    max_hp = st.hp,
                    speed = st.speed,
                    critBonus = st.critBonus,
                    lifesteal = st.lifesteal,
                    regenPct = st.regenPct,
                    coinMult = st.coinMult,
                    enemySlow = st.enemySlow,
                    trait = entry.trait,
                    charm = entry.charm,
                    stars = entry.stars,
                })
            end
        end
        return {
            units = units,
            towerFloor = profile.towerFloor,
            storyCleared = profile.storyCleared,
            rebirths = profile.rebirths,
            teamPower = computeTeamPower(player, profile),
        }
    end
    return profile
end

rewardSignal.Event:Connect(function(player, data)
    if typeof(player) ~= "Instance" or not player:IsA("Player") or typeof(data) ~= "table" then
        return
    end
    local profile = profiles[player]
    if not profile then
        return
    end
    if typeof(data.coins) == "number" and data.coins > 0 then
        profile.coins = profile.coins + math.floor(data.coins)
    end
    if typeof(data.shards) == "number" and data.shards > 0 then
        profile.shards = profile.shards + math.floor(data.shards)
    end
    if typeof(data.raidTokens) == "number" and data.raidTokens > 0 then
        profile.raidTokens = profile.raidTokens + math.floor(data.raidTokens)
    end
    if typeof(data.xp) == "number" and data.xp > 0 then
        addXp(profile, math.floor(data.xp))
    end
    if typeof(data.towerFloor) == "number" and data.towerFloor > profile.towerFloor then
        profile.towerFloor = math.floor(data.towerFloor)
    end
    if typeof(data.storyCleared) == "number" and data.storyCleared > profile.storyCleared then
        profile.storyCleared = math.floor(data.storyCleared)
    end
    syncPlayer(player)
    if typeof(data.reason) == "string" and #data.reason > 0 then
        coreAction:FireClient(player, "toast", data.reason)
    end
end)

local PROMPT_PANEL_MAP = {
    SummonPrompt = "Index",
    InspectPrompt = "Index",
    BossPrompt = "Battle",
    TowerPrompt = "Battle",
    PvpPrompt = "Battle",
    PotionPrompt = "Potions",
    ForgePrompt = "Forge",
}

-- Wire Workspace ProximityPrompts & animated Torii halo rings + Leaderboard Wall
local function wireWorldLandmarks()
    local lobby = workspace:FindFirstChild("ApexLobby")
    if lobby then
        for _, desc in ipairs(lobby:GetDescendants()) do
            if desc:IsA("ProximityPrompt") then
                desc.Triggered:Connect(function(player)
                    local panel = desc:GetAttribute("OpenPanel") or PROMPT_PANEL_MAP[desc.Name]
                    if typeof(panel) == "string" then
                        openPanelEvent:FireClient(player, panel)
                    end
                end)
            end
        end

        local haloOuter = lobby:FindFirstChild("ToriiHaloOuter", true)
        local haloInner = lobby:FindFirstChild("ToriiHaloInner", true)
        if haloOuter and haloOuter:IsA("BasePart") and haloInner and haloInner:IsA("BasePart") then
            local baseOuter = haloOuter.Position
            local baseInner = haloInner.Position
            local t = 0
            RunService.Heartbeat:Connect(function(dt)
                t = t + dt
                local bob = math.sin(t * 2.0) * 0.45
                haloOuter.CFrame = CFrame.new(baseOuter + Vector3.new(0, bob, 0)) * CFrame.Angles(0, t * 1.1, math.rad(15))
                haloInner.CFrame = CFrame.new(baseInner + Vector3.new(0, -bob * 0.8, 0)) * CFrame.Angles(math.rad(12), -t * 1.6, 0)
            end)
        end
    end

    local gallery = workspace:FindFirstChild("CardGallery")
    if gallery then
        for _, desc in ipairs(gallery:GetDescendants()) do
            if desc:IsA("ProximityPrompt") then
                desc.Triggered:Connect(function(player)
                    openPanelEvent:FireClient(player, "Index")
                end)
            end
        end
    end

    task.spawn(function()
        while true do
            task.wait(3)
            if lobby then
                local lbText = lobby:FindFirstChild("Rankings", true)
                if lbText and lbText:IsA("TextLabel") then
                    local lines = { "APEX TOWER & POWER LEADERBOARD" }
                    for _, p in ipairs(Players:GetPlayers()) do
                        local fl = p:GetAttribute("TowerFloor") or 1
                        local pw = p:GetAttribute("TeamPower") or 0
                        table.insert(lines, p.Name .. "  |  Floor " .. tostring(fl) .. "  |  Power " .. tostring(pw))
                    end
                    lbText.Text = table.concat(lines, "\n")
                end
                local raidText = lobby:FindFirstChild("RaidStats", true)
                if raidText and raidText:IsA("TextLabel") then
                    local rAct = StateFolder:FindFirstChild("RaidBossActive")
                    local rCd = StateFolder:FindFirstChild("RaidCountdown")
                    local rHp = StateFolder:FindFirstChild("RaidBossHP")
                    local rMax = StateFolder:FindFirstChild("RaidBossMaxHP")
                    if rAct and rAct:IsA("BoolValue") and rAct.Value and rHp and rMax then
                        raidText.Text = "WORLD RAID ACTIVE!\nAbyssal Eclipse Leviathan\nHP: " .. tostring(rHp.Value) .. " / " .. tostring(rMax.Value)
                    elseif rCd then
                        raidText.Text = "NEXT WORLD RAID\nSpawning in " .. tostring(rCd.Value) .. "s\nBring your 4-card team!"
                    end
                end
            end
        end
    end)
end
task.spawn(wireWorldLandmarks)

local function onPlayerAdded(player: Player)
    ensureLeaderstats(player)
    local res = storageBridge:Invoke("load", player.UserId)
    local prof = defaultProfile()
    if typeof(res) == "table" and res.ok then
        player:SetAttribute("DataLoaded", true)
        if typeof(res.data) == "table" and typeof(res.data.owned) == "table" then
            for k, v in pairs(res.data) do
                prof[k] = v
            end
            prof.autoRoll = false
            prof.lastRollAt = 0
            prof.buffs = prof.buffs or { luckMult = 1.0, luckUntil = 0, speedMult = 1.0, speedUntil = 0 }
        end
    else
        player:SetAttribute("DataLoaded", false)
    end
    profiles[player] = prof

    player.CharacterAdded:Connect(function()
        task.wait(0.25)
        if profiles[player] then
            updateOverheadCard(player, profiles[player])
        end
    end)

    syncPlayer(player)
    if player.Character then
        task.spawn(function()
            task.wait(0.25)
            updateOverheadCard(player, prof)
        end)
    end

    task.spawn(function()
        while player.Parent do
            local st = profiles[player]
            local spd = if st then computeRollSpeed(st) else 1.0
            task.wait(math.max(0.22, DEFAULT_FAST_COOLDOWN / spd))
            if st and st.autoRoll then
                local r = performRoll(player, true, st.selectedPack)
                if not r.ok and r.reason == "insufficient_coins" then
                    st.selectedPack = "Starter"
                end
            end
        end
    end)
end

Players.PlayerAdded:Connect(onPlayerAdded)
for _, p in ipairs(Players:GetPlayers()) do
    task.spawn(onPlayerAdded, p)
end

Players.PlayerRemoving:Connect(function(player)
    local card = overheadCards[player]
    if card then
        card:Destroy()
        overheadCards[player] = nil
    end
    profiles[player] = nil
    rateBuckets[player] = nil
end)
