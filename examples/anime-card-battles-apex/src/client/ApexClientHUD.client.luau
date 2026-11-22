-- ApexClientHUD.client.luau (StarterPlayer.StarterPlayerScripts.ApexClientHUD)
-- Complete anime-arcade Client HUD: Top Status & Raid Bar, Bottom Roll/Auto/Fast Dock + Pity,
-- Animated Gacha Summon Cutscene, 4-Slot Visual Deck + 24-Card Index + Trait/Charm Forge,
-- Potions/Upgrades/Codes Drawer, and Animated 4v4 Battle Clash Board (1x/2x/3x speed).

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")
local UserInputService = game:GetService("UserInputService")

local player = Players.LocalPlayer
local playerGui = player:WaitForChild("PlayerGui")

local Modules = ReplicatedStorage:WaitForChild("Modules")
local CardDatabase = require(Modules:WaitForChild("CardDatabase"))
local CombatMath = require(Modules:WaitForChild("CombatMath"))

local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local StateFolder = ReplicatedStorage:WaitForChild("State")

local rollFunction = Remotes:WaitForChild("RollRequest")
local rollNotice = Remotes:WaitForChild("RollNotice")
local coreAction = Remotes:WaitForChild("CoreAction")
local stateSync = Remotes:WaitForChild("StateSync")
local battleEvent = Remotes:WaitForChild("BattleEvent")
local weatherRaidEvent = Remotes:WaitForChild("WeatherRaidEvent")
local openPanelEvent = Remotes:WaitForChild("OpenPanelEvent")

local function playSfx(name: string)
    local bus = player:FindFirstChild("ClientAudioBus")
    if bus and bus:IsA("BindableEvent") then
        bus:Fire(name)
    end
end

local PAL = {
    bg = Color3.fromRGB(13, 17, 29),
    panel = Color3.fromRGB(20, 26, 44),
    card = Color3.fromRGB(27, 35, 58),
    stroke = Color3.fromRGB(68, 86, 138),
    accent = Color3.fromRGB(99, 102, 241),
    cyan = Color3.fromRGB(56, 189, 248),
    gold = Color3.fromRGB(250, 204, 21),
    crimson = Color3.fromRGB(239, 68, 68),
    emerald = Color3.fromRGB(52, 211, 153),
    text = Color3.fromRGB(244, 247, 255),
    muted = Color3.fromRGB(156, 169, 198),
}

local function addCorner(inst: GuiObject, px: number)
    local c = Instance.new("UICorner")
    c.CornerRadius = UDim.new(0, px)
    c.Parent = inst
end

local function addStroke(inst: GuiObject, color: Color3, thick: number)
    local s = Instance.new("UIStroke")
    s.Color = color
    s.Thickness = thick
    s.Parent = inst
    return s
end

local function makeFrame(parent, name, size, pos, bg, trans)
    local f = Instance.new("Frame")
    f.Name = name
    f.Size = size
    f.Position = pos
    f.BackgroundColor3 = bg or PAL.panel
    f.BackgroundTransparency = trans or 0
    f.BorderSizePixel = 0
    f.Parent = parent
    return f
end

local function makeLabel(parent, name, text, size, pos, color, align)
    local l = Instance.new("TextLabel")
    l.Name = name
    l.Text = text
    l.Size = size
    l.Position = pos
    l.BackgroundTransparency = 1
    l.Font = Enum.Font.GothamBold
    l.TextColor3 = color or PAL.text
    l.TextScaled = true
    l.TextWrapped = true
    l.TextXAlignment = align or Enum.TextXAlignment.Center
    l.Parent = parent
    return l
end

local function makeButton(parent, name, text, size, pos, bg, textColor)
    local b = Instance.new("TextButton")
    b.Name = name
    b.Text = text
    b.Size = size
    b.Position = pos
    b.BackgroundColor3 = bg or PAL.accent
    b.Font = Enum.Font.GothamBold
    b.TextColor3 = textColor or PAL.text
    b.TextScaled = true
    b.AutoButtonColor = true
    b.BorderSizePixel = 0
    addCorner(b, 8)
    b.Parent = parent
    return b
end

-- Remove duplicate GUI if hot-reloaded
local oldGui = playerGui:FindFirstChild("ApexCardBattlesHUD")
if oldGui then
    oldGui:Destroy()
end

local rootGui = Instance.new("ScreenGui")
rootGui.Name = "ApexCardBattlesHUD"
rootGui.ResetOnSpawn = false
rootGui.IgnoreGuiInset = false
rootGui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling
rootGui.Parent = playerGui

local currentState = {
    coins = 450,
    shards = 35,
    raidTokens = 0,
    level = 1,
    xp = 0,
    xpMax = 80,
    pity = 0,
    pityMax = 50,
    selectedPack = "Starter",
    autoRoll = false,
    luck = 1.0,
    teamPower = 0,
    towerFloor = 1,
    storyCleared = 0,
    rebirths = 0,
    inventoryCap = 32,
    autoDelete = {},
    upgrades = { Luck = 0, Speed = 0, Cap = 0 },
    owned = {},
    deck = {},
    index = {},
    charmsOwned = {},
}

local selectedCardForForge = "eclipse-arcane-alchemist"
local battleSpeed = 1
local battlePlaying = false

-- ======================== TOP BAR & RAID BANNER ========================
local topBar = makeFrame(rootGui, "TopStatusBar", UDim2.new(0.96, 0, 0, 58), UDim2.new(0.02, 0, 0, 8), PAL.bg, 0.12)
addCorner(topBar, 12)
addStroke(topBar, PAL.stroke, 1.5)

local coinsLbl = makeLabel(topBar, "CoinsLbl", "Coins: 450", UDim2.new(0.14, 0, 0.72, 0), UDim2.new(0.01, 0, 0.14, 0), PAL.gold, Enum.TextXAlignment.Left)
local shardsLbl = makeLabel(topBar, "ShardsLbl", "Shards: 35", UDim2.new(0.12, 0, 0.72, 0), UDim2.new(0.155, 0, 0.14, 0), PAL.cyan, Enum.TextXAlignment.Left)
local luckLbl = makeLabel(topBar, "LuckLbl", "Luck: 1.00x", UDim2.new(0.12, 0, 0.72, 0), UDim2.new(0.28, 0, 0.14, 0), PAL.emerald, Enum.TextXAlignment.Left)
local levelLbl = makeLabel(topBar, "LevelLbl", "Lv.1 | Power: 0", UDim2.new(0.17, 0, 0.72, 0), UDim2.new(0.405, 0, 0.14, 0), PAL.text, Enum.TextXAlignment.Left)

local weatherBadge = makeFrame(topBar, "WeatherBadge", UDim2.new(0.19, 0, 0.76, 0), UDim2.new(0.585, 0, 0.12, 0), PAL.card, 0)
addCorner(weatherBadge, 8)
addStroke(weatherBadge, PAL.cyan, 1)
local weatherLbl = makeLabel(weatherBadge, "WeatherText", "Clear (1.0x)", UDim2.new(0.94, 0, 0.85, 0), UDim2.new(0.03, 0, 0.08, 0), PAL.cyan, Enum.TextXAlignment.Center)

local raidBtn = makeButton(topBar, "RaidBarBtn", "Raid in 45s", UDim2.new(0.20, 0, 0.76, 0), UDim2.new(0.79, 0, 0.12, 0), Color3.fromRGB(127, 29, 29), PAL.text)
addStroke(raidBtn, PAL.crimson, 1.5)

-- Toast Notification Banner
local toastFrame = makeFrame(rootGui, "ToastBanner", UDim2.new(0.46, 0, 0, 38), UDim2.new(0.27, 0, 0, 74), PAL.card, 0.05)
addCorner(toastFrame, 10)
addStroke(toastFrame, PAL.gold, 1.5)
toastFrame.Visible = false
local toastLbl = makeLabel(toastFrame, "ToastText", "", UDim2.new(0.94, 0, 0.85, 0), UDim2.new(0.03, 0, 0.08, 0), PAL.gold)

local function showToast(msg: string)
    toastLbl.Text = msg
    toastFrame.Visible = true
    toastFrame.Position = UDim2.new(0.27, 0, 0, 68)
    TweenService:Create(toastFrame, TweenInfo.new(0.2, Enum.EasingStyle.Back), { Position = UDim2.new(0.27, 0, 0, 76) }):Play()
    task.delay(3.2, function()
        if toastLbl.Text == msg then
            toastFrame.Visible = false
        end
    end)
end

-- ======================== BOTTOM ACTION DOCK ========================
local bottomDock = makeFrame(rootGui, "BottomDock", UDim2.new(0.86, 0, 0, 106), UDim2.new(0.07, 0, 1, -116), PAL.bg, 0.1)
addCorner(bottomDock, 14)
addStroke(bottomDock, PAL.stroke, 1.5)

-- Pity bar across top of bottom dock
local pityBg = makeFrame(bottomDock, "PityBg", UDim2.new(0.96, 0, 0, 16), UDim2.new(0.02, 0, 0, 6), Color3.fromRGB(28, 34, 54), 0)
addCorner(pityBg, 6)
local pityFill = makeFrame(pityBg, "PityFill", UDim2.new(0.02, 0, 1, 0), UDim2.new(0, 0, 0, 0), PAL.gold, 0)
addCorner(pityFill, 6)
local pityLbl = makeLabel(pityBg, "PityLbl", "Mythic+ Pity: 0 / 50", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), Color3.fromRGB(255, 255, 255))

-- Pack Switcher & Roll Controls
local packBtn = makeButton(bottomDock, "PackCycleBtn", "Pack: Starter (Free)", UDim2.new(0.22, 0, 0, 38), UDim2.new(0.02, 0, 0, 28), PAL.card)
addStroke(packBtn, PAL.cyan, 1)

local rollBtn = makeButton(bottomDock, "RollBtn", "ROLL 1", UDim2.new(0.16, 0, 0, 42), UDim2.new(0.255, 0, 0, 26), PAL.accent)
addStroke(rollBtn, PAL.cyan, 1.5)
local fastRollBtn = makeButton(bottomDock, "FastRollBtn", "FAST", UDim2.new(0.10, 0, 0, 42), UDim2.new(0.425, 0, 0, 26), Color3.fromRGB(14, 116, 144))
local autoRollBtn = makeButton(bottomDock, "AutoRollBtn", "AUTO: OFF", UDim2.new(0.12, 0, 0, 42), UDim2.new(0.535, 0, 0, 26), PAL.card)

-- Navigation Tabs
local tabDeckBtn = makeButton(bottomDock, "TabDeckBtn", "Deck & Index", UDim2.new(0.15, 0, 0, 32), UDim2.new(0.02, 0, 0, 70), PAL.card)
local tabForgeBtn = makeButton(bottomDock, "TabForgeBtn", "Traits & Charms", UDim2.new(0.16, 0, 0, 32), UDim2.new(0.18, 0, 0, 70), PAL.card)
local tabPotionsBtn = makeButton(bottomDock, "TabPotionsBtn", "Potions & Codes", UDim2.new(0.16, 0, 0, 32), UDim2.new(0.35, 0, 0, 70), PAL.card)
local tabUpgradesBtn = makeButton(bottomDock, "TabUpgradesBtn", "Upgrades", UDim2.new(0.13, 0, 0, 32), UDim2.new(0.52, 0, 0, 70), PAL.card)
local tabBattleBtn = makeButton(bottomDock, "TabBattleBtn", "4v4 BATTLE ARENA", UDim2.new(0.31, 0, 0, 44), UDim2.new(0.67, 0, 0, 56), Color3.fromRGB(185, 28, 28))
addStroke(tabBattleBtn, PAL.gold, 1.5)

-- ======================== DRAWER PANELS ========================
local activeDrawer = nil
local drawers = {}

local function makeDrawer(name: string, titleText: string)
    local panel = makeFrame(rootGui, name, UDim2.new(0.84, 0, 0.68, 0), UDim2.new(0.08, 0, 0.13, 0), PAL.bg, 0.04)
    addCorner(panel, 16)
    addStroke(panel, PAL.stroke, 2)
    panel.Visible = false

    local header = makeFrame(panel, "Header", UDim2.new(1, 0, 0, 46), UDim2.new(0, 0, 0, 0), PAL.panel, 0)
    addCorner(header, 14)
    makeLabel(header, "Title", titleText, UDim2.new(0.75, 0, 0.8, 0), UDim2.new(0.02, 0, 0.1, 0), PAL.text, Enum.TextXAlignment.Left)
    local closeBtn = makeButton(header, "CloseBtn", "X", UDim2.new(0, 38, 0, 34), UDim2.new(1, -46, 0, 6), PAL.crimson)
    closeBtn.MouseButton1Click:Connect(function()
        playSfx("ui_click")
        panel.Visible = false
        activeDrawer = nil
    end)

    local body = makeFrame(panel, "Body", UDim2.new(0.96, 0, 1, -56), UDim2.new(0.02, 0, 0, 50), PAL.bg, 1)
    drawers[name] = panel
    return panel, body
end

local function toggleDrawer(name: string)
    playSfx("ui_click")
    for k, d in pairs(drawers) do
        if k == name then
            local show = not d.Visible
            d.Visible = show
            activeDrawer = if show then name else nil
        else
            d.Visible = false
        end
    end
end

-- ------------------------ 1. DECK & 24-CARD INDEX DRAWER ------------------------
local deckPanel, deckBody = makeDrawer("Index", "4-SLOT DECK BUILDER & 24-CARD COLLECTION INDEX")
local deckSlotsRow = makeFrame(deckBody, "DeckSlotsRow", UDim2.new(1, 0, 0, 118), UDim2.new(0, 0, 0, 0), PAL.panel, 0.2)
addCorner(deckSlotsRow, 10)

local autoEquipBtn = makeButton(deckSlotsRow, "AutoEquipBtn", "Auto-Equip Best", UDim2.new(0.18, 0, 0, 34), UDim2.new(0.80, 0, 0, 10), PAL.accent)
local rebirthBtn = makeButton(deckSlotsRow, "RebirthBtn", "Rebirth (F10+)", UDim2.new(0.18, 0, 0, 34), UDim2.new(0.80, 0, 0, 52), Color3.fromRGB(147, 51, 234))

autoEquipBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    coreAction:FireServer("auto_equip")
end)
rebirthBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    coreAction:FireServer("rebirth")
end)

-- Auto-delete toggles
local autoDelBar = makeFrame(deckBody, "AutoDelBar", UDim2.new(1, 0, 0, 34), UDim2.new(0, 0, 0, 124), PAL.panel, 0.3)
addCorner(autoDelBar, 8)
makeLabel(autoDelBar, "Lbl", "Auto-Delete Dupes:", UDim2.new(0.22, 0, 0.8, 0), UDim2.new(0.01, 0, 0.1, 0), PAL.muted, Enum.TextXAlignment.Left)
local autoDelBtns = {}
for i, rar in ipairs({ "Common", "Rare", "Epic" }) do
    local b = makeButton(autoDelBar, "AutoDel_" .. rar, rar .. ": OFF", UDim2.new(0.14, 0, 0.8, 0), UDim2.new(0.24 + (i - 1) * 0.15, 0, 0.1, 0), PAL.card)
    b.MouseButton1Click:Connect(function()
        playSfx("ui_click")
        coreAction:FireServer("toggle_autodelete", rar)
    end)
    autoDelBtns[rar] = b
end

local indexScroll = Instance.new("ScrollingFrame")
indexScroll.Name = "CardIndexScroll"
indexScroll.Size = UDim2.new(1, 0, 1, -166)
indexScroll.Position = UDim2.new(0, 0, 0, 164)
indexScroll.BackgroundTransparency = 1
indexScroll.ScrollBarThickness = 6
indexScroll.CanvasSize = UDim2.new(0, 0, 0, 0)
indexScroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
indexScroll.Parent = deckBody

local gridLayout = Instance.new("UIGridLayout")
gridLayout.CellSize = UDim2.new(0, 168, 0, 214)
gridLayout.CellPadding = UDim2.new(0, 10, 0, 10)
gridLayout.Parent = indexScroll

local function rebuildDeckAndIndex()
    for _, child in ipairs(deckSlotsRow:GetChildren()) do
        if child.Name:sub(1, 8) == "DeckSlot" then
            child:Destroy()
        end
    end
    for slot = 1, 4 do
        local cid = currentState.deck[slot]
        local slotBox = makeFrame(deckSlotsRow, "DeckSlot" .. slot, UDim2.new(0.18, 0, 0, 104), UDim2.new(0.01 + (slot - 1) * 0.195, 0, 0, 7), PAL.card, 0)
        addCorner(slotBox, 8)
        if cid and CardDatabase.CARD_BY_ID[cid] then
            local def = CardDatabase.CARD_BY_ID[cid]
            local entry = currentState.owned[cid]
            local rar = CardDatabase.RARITY_BY_ID[def.rarity] or CardDatabase.RARITIES[1]
            addStroke(slotBox, rar.color, 1.5)
            local img = Instance.new("ImageLabel")
            img.Size = UDim2.new(0.42, 0, 0.88, 0)
            img.Position = UDim2.new(0.04, 0, 0.06, 0)
            img.BackgroundTransparency = 1
            img.Image = def.portrait
            img.ScaleType = Enum.ScaleType.Crop
            addCorner(img, 6)
            img.Parent = slotBox
            makeLabel(slotBox, "Title", def.title, UDim2.new(0.50, 0, 0.34, 0), UDim2.new(0.48, 0, 0.05, 0), PAL.text, Enum.TextXAlignment.Left)
            local stars = (entry and entry.stars) or 1
            local traitTxt = (entry and entry.trait) or "No Trait"
            makeLabel(slotBox, "Meta", stars .. "★ | " .. traitTxt, UDim2.new(0.50, 0, 0.25, 0), UDim2.new(0.48, 0, 0.40, 0), PAL.gold, Enum.TextXAlignment.Left)
            local unBtn = makeButton(slotBox, "Unequip", "Remove", UDim2.new(0.48, 0, 0.26, 0), UDim2.new(0.48, 0, 0.68, 0), PAL.crimson)
            unBtn.MouseButton1Click:Connect(function()
                playSfx("ui_click")
                coreAction:FireServer("unequip", cid)
            end)
        else
            addStroke(slotBox, PAL.stroke, 1)
            makeLabel(slotBox, "Empty", "Slot " .. slot .. "\nEmpty", UDim2.new(0.9, 0, 0.7, 0), UDim2.new(0.05, 0, 0.15, 0), PAL.muted)
        end
    end

    for rar, btn in pairs(autoDelBtns) do
        local on = currentState.autoDelete[rar] == true
        btn.Text = rar .. (if on then ": DEL" else ": OFF")
        btn.BackgroundColor3 = if on then PAL.crimson else PAL.card
    end

    for _, child in ipairs(indexScroll:GetChildren()) do
        if child:IsA("Frame") then
            child:Destroy()
        end
    end

    for _, def in ipairs(CardDatabase.CARDS) do
        local entry = currentState.owned[def.id]
        local rar = CardDatabase.RARITY_BY_ID[def.rarity] or CardDatabase.RARITIES[1]
        local cardFrame = makeFrame(indexScroll, "Card_" .. def.id, UDim2.new(0, 168, 0, 214), UDim2.new(0, 0, 0, 0), PAL.card, 0)
        addCorner(cardFrame, 10)
        addStroke(cardFrame, if entry then rar.color else Color3.fromRGB(48, 56, 78), if entry then 2 else 1)

        local img = Instance.new("ImageLabel")
        img.Size = UDim2.new(0.90, 0, 0.50, 0)
        img.Position = UDim2.new(0.05, 0, 0.04, 0)
        img.BackgroundTransparency = 1
        img.Image = def.portrait
        img.ImageTransparency = if entry then 0 else 0.72
        img.ScaleType = Enum.ScaleType.Crop
        addCorner(img, 6)
        img.Parent = cardFrame

        makeLabel(cardFrame, "Rar", def.rarity .. " • " .. def.element, UDim2.new(0.9, 0, 0.09, 0), UDim2.new(0.05, 0, 0.55, 0), rar.color, Enum.TextXAlignment.Left)
        makeLabel(cardFrame, "Title", def.title, UDim2.new(0.9, 0, 0.11, 0), UDim2.new(0.05, 0, 0.64, 0), PAL.text, Enum.TextXAlignment.Left)

        if entry then
            local st = CombatMath.computeCardStats(def, entry)
            makeLabel(cardFrame, "Stats", "ATK " .. st.atk .. " | x" .. (entry.copies or 1) .. " | " .. (entry.stars or 1) .. "★", UDim2.new(0.9, 0, 0.09, 0), UDim2.new(0.05, 0, 0.75, 0), PAL.muted, Enum.TextXAlignment.Left)

            local isEquipped = table.find(currentState.deck, def.id) ~= nil
            local eqBtn = makeButton(cardFrame, "EqBtn", if isEquipped then "Drop" else "Equip", UDim2.new(0.44, 0, 0.12, 0), UDim2.new(0.04, 0, 0.85, 0), if isEquipped then PAL.crimson else PAL.emerald)
            eqBtn.MouseButton1Click:Connect(function()
                playSfx("ui_click")
                coreAction:FireServer(if isEquipped then "unequip" else "equip", def.id)
            end)

            local fuseBtn = makeButton(cardFrame, "FuseBtn", if (entry.copies or 1) >= 3 then "Fuse★" else "Forge", UDim2.new(0.44, 0, 0.12, 0), UDim2.new(0.52, 0, 0.85, 0), PAL.accent)
            fuseBtn.MouseButton1Click:Connect(function()
                playSfx("ui_click")
                if (entry.copies or 1) >= 3 then
                    coreAction:FireServer("fuse", def.id)
                else
                    selectedCardForForge = def.id
                    toggleDrawer("Forge")
                end
            end)
        else
            local lockInfo = if def.weatherLock then ("Weather: " .. def.weatherLock) else ("Pack: " .. def.pack)
            makeLabel(cardFrame, "Locked", lockInfo, UDim2.new(0.9, 0, 0.16, 0), UDim2.new(0.05, 0, 0.78, 0), PAL.muted, Enum.TextXAlignment.Center)
        end
    end
end

-- ------------------------ 2. TRAIT & CHARM FORGE DRAWER ------------------------
local forgePanel, forgeBody = makeDrawer("Forge", "ALCHEMIST TRAIT REROLL & CHARM FORGE")
local forgeInfoLbl = makeLabel(forgeBody, "SelectedCardLbl", "Target Card: Eclipse Arcane Alchemist", UDim2.new(0.98, 0, 0, 34), UDim2.new(0.01, 0, 0, 6), PAL.gold, Enum.TextXAlignment.Left)
local rerollTraitBtn = makeButton(forgeBody, "RerollTraitBtn", "Reroll Trait (15 Shards)", UDim2.new(0.45, 0, 0, 44), UDim2.new(0.01, 0, 0, 46), Color3.fromRGB(124, 58, 237))
local lockCardBtn = makeButton(forgeBody, "LockCardBtn", "Toggle Card Lock", UDim2.new(0.45, 0, 0, 44), UDim2.new(0.50, 0, 0, 46), PAL.card)

rerollTraitBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    coreAction:FireServer("reroll_trait", selectedCardForForge)
end)
lockCardBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    coreAction:FireServer("toggle_lock", selectedCardForForge)
end)

makeLabel(forgeBody, "CharmHeader", "Craftable Charms (Equipped onto Target Card)", UDim2.new(0.98, 0, 0, 28), UDim2.new(0.01, 0, 0, 102), PAL.cyan, Enum.TextXAlignment.Left)
local charmButtons = {}
for idx, ch in ipairs(CardDatabase.CHARMS) do
    local row = makeFrame(forgeBody, "Charm_" .. ch.id, UDim2.new(0.96, 0, 0, 56), UDim2.new(0.02, 0, 0, 134 + (idx - 1) * 62), PAL.card, 0)
    addCorner(row, 8)
    addStroke(row, PAL.stroke, 1)
    makeLabel(row, "Name", ch.name .. " — " .. ch.desc, UDim2.new(0.68, 0, 0.8, 0), UDim2.new(0.02, 0, 0.1, 0), PAL.text, Enum.TextXAlignment.Left)
    local btn = makeButton(row, "ActionBtn", "Craft (" .. ch.shardCost .. " Shards)", UDim2.new(0.26, 0, 0.72, 0), UDim2.new(0.72, 0, 0.14, 0), PAL.emerald)
    btn.MouseButton1Click:Connect(function()
        playSfx("ui_click")
        if currentState.charmsOwned[ch.id] then
            coreAction:FireServer("equip_charm", selectedCardForForge, ch.id)
        else
            coreAction:FireServer("craft_charm", ch.id)
        end
    end)
    charmButtons[ch.id] = btn
end

local function refreshForge()
    local def = CardDatabase.CARD_BY_ID[selectedCardForForge]
    local entry = currentState.owned[selectedCardForForge]
    if not def or not entry then
        for cid in pairs(currentState.owned) do
            selectedCardForForge = cid
            def = CardDatabase.CARD_BY_ID[cid]
            entry = currentState.owned[cid]
            break
        end
    end
    if def and entry then
        forgeInfoLbl.Text = "Target: " .. def.title .. " | Trait: " .. (entry.trait or "None") .. " | Charm: " .. (entry.charm or "None") .. (if entry.locked then " [LOCKED]" else "")
    end
    for _, ch in ipairs(CardDatabase.CHARMS) do
        local btn = charmButtons[ch.id]
        if btn then
            if currentState.charmsOwned[ch.id] then
                btn.Text = if entry and entry.charm == ch.id then "Equipped" else "Equip to Card"
                btn.BackgroundColor3 = PAL.accent
            else
                btn.Text = "Craft (" .. ch.shardCost .. " Shards)"
                btn.BackgroundColor3 = PAL.emerald
            end
        end
    end
end

-- ------------------------ 3. POTIONS & CODES DRAWER ------------------------
local potionsPanel, potionsBody = makeDrawer("Potions", "ALCHEMIST POTIONS & REWARD CODES")
for idx, pot in ipairs(CardDatabase.POTIONS) do
    local col = (idx - 1) % 2
    local row = math.floor((idx - 1) / 2)
    local box = makeFrame(potionsBody, "Pot_" .. pot.id, UDim2.new(0.48, 0, 0, 62), UDim2.new(0.01 + col * 0.50, 0, 0, 8 + row * 68), PAL.card, 0)
    addCorner(box, 8)
    addStroke(box, PAL.emerald, 1)
    makeLabel(box, "Info", pot.name .. "\n" .. pot.desc, UDim2.new(0.65, 0, 0.85, 0), UDim2.new(0.02, 0, 0.08, 0), PAL.text, Enum.TextXAlignment.Left)
    local buy = makeButton(box, "Buy", pot.cost .. " C", UDim2.new(0.28, 0, 0.72, 0), UDim2.new(0.70, 0, 0.14, 0), PAL.emerald)
    buy.MouseButton1Click:Connect(function()
        playSfx("ui_click")
        coreAction:FireServer("buy_potion", pot.id)
    end)
end

local codeBar = makeFrame(potionsBody, "CodeBar", UDim2.new(0.98, 0, 0, 62), UDim2.new(0.01, 0, 1, -70), PAL.panel, 0)
addCorner(codeBar, 10)
local codeBox = Instance.new("TextBox")
codeBox.Name = "CodeInput"
codeBox.PlaceholderText = "Enter code (REBIRTH / APEX2026 / LUCKBOOST)..."
codeBox.Text = ""
codeBox.Size = UDim2.new(0.66, 0, 0.72, 0)
codeBox.Position = UDim2.new(0.02, 0, 0.14, 0)
codeBox.BackgroundColor3 = PAL.card
codeBox.TextColor3 = PAL.text
codeBox.Font = Enum.Font.GothamBold
codeBox.TextScaled = true
addCorner(codeBox, 8)
codeBox.Parent = codeBar

local redeemBtn = makeButton(codeBar, "RedeemBtn", "Redeem Code", UDim2.new(0.28, 0, 0.72, 0), UDim2.new(0.70, 0, 0.14, 0), PAL.gold, Color3.fromRGB(20, 20, 30))
redeemBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    if #codeBox.Text > 0 then
        coreAction:FireServer("redeem_code", codeBox.Text)
        codeBox.Text = ""
    end
end)

-- ------------------------ 4. PERMANENT UPGRADES DRAWER ------------------------
local upgPanel, upgBody = makeDrawer("Upgrades", "PERMANENT ROLL-LEVEL UPGRADES")
local upgButtons = {}
for idx, upg in ipairs(CardDatabase.UPGRADES) do
    local row = makeFrame(upgBody, "Upg_" .. upg.id, UDim2.new(0.96, 0, 0, 76), UDim2.new(0.02, 0, 0, 14 + (idx - 1) * 86), PAL.card, 0)
    addCorner(row, 10)
    addStroke(row, PAL.accent, 1.5)
    local info = makeLabel(row, "Info", upg.name .. "\n" .. upg.perLevelText, UDim2.new(0.66, 0, 0.8, 0), UDim2.new(0.02, 0, 0.1, 0), PAL.text, Enum.TextXAlignment.Left)
    local btn = makeButton(row, "BuyBtn", "Upgrade", UDim2.new(0.28, 0, 0.7, 0), UDim2.new(0.70, 0, 0.15, 0), PAL.accent)
    btn.MouseButton1Click:Connect(function()
        playSfx("ui_click")
        coreAction:FireServer("buy_upgrade", upg.id)
    end)
    upgButtons[upg.id] = { btn = btn, info = info, def = upg }
end

local function refreshUpgrades()
    for uid, ref in pairs(upgButtons) do
        local cur = (currentState.upgrades and currentState.upgrades[uid]) or 0
        local cost = CombatMath.getUpgradeCost(uid, cur)
        ref.info.Text = ref.def.name .. " (Lv." .. cur .. "/" .. ref.def.maxLevel .. ")\n" .. ref.def.perLevelText
        ref.btn.Text = if cur >= ref.def.maxLevel then "MAX" else ("Upgrade (" .. cost .. " C)")
    end
end

-- ------------------------ 5. ANIMATED 4v4 BATTLE CLASH BOARD ------------------------
local battlePanel, battleBody = makeDrawer("Battle", "4v4 ANIME BATTLE CLASH ARENA")

local modeBar = makeFrame(battleBody, "ModeBar", UDim2.new(1, 0, 0, 42), UDim2.new(0, 0, 0, 0), PAL.panel, 0)
addCorner(modeBar, 8)
local speedBtn = makeButton(modeBar, "SpeedBtn", "Speed: 1x", UDim2.new(0.14, 0, 0.78, 0), UDim2.new(0.85, 0, 0.11, 0), PAL.card)
speedBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    battleSpeed = if battleSpeed == 1 then 2 elseif battleSpeed == 2 then 3 else 1
    speedBtn.Text = "Speed: " .. battleSpeed .. "x"
end)

local towerFightBtn = makeButton(modeBar, "TowerFightBtn", "Infinite Tower", UDim2.new(0.18, 0, 0.78, 0), UDim2.new(0.01, 0, 0.11, 0), PAL.cyan, Color3.fromRGB(15, 23, 42))
local raidFightBtn = makeButton(modeBar, "RaidFightBtn", "World Raid Boss", UDim2.new(0.18, 0, 0.78, 0), UDim2.new(0.20, 0, 0.11, 0), PAL.crimson)
local pvpFightBtn = makeButton(modeBar, "PvpFightBtn", "PvP Duel", UDim2.new(0.14, 0, 0.78, 0), UDim2.new(0.39, 0, 0.11, 0), Color3.fromRGB(147, 51, 234))

for bIdx = 1, 5 do
    local sBtn = makeButton(modeBar, "Boss" .. bIdx, "S" .. bIdx, UDim2.new(0.055, 0, 0.78, 0), UDim2.new(0.545 + (bIdx - 1) * 0.06, 0, 0.11, 0), PAL.card)
    sBtn.MouseButton1Click:Connect(function()
        if not battlePlaying then
            playSfx("ui_click")
            battleEvent:FireServer("story", bIdx)
        end
    end)
end

towerFightBtn.MouseButton1Click:Connect(function()
    if not battlePlaying then
        playSfx("ui_click")
        battleEvent:FireServer("tower", currentState.towerFloor)
    end
end)
raidFightBtn.MouseButton1Click:Connect(function()
    if not battlePlaying then
        playSfx("ui_click")
        battleEvent:FireServer("raid", 1)
    end
end)
pvpFightBtn.MouseButton1Click:Connect(function()
    if not battlePlaying then
        playSfx("ui_click")
        battleEvent:FireServer("pvp", 1)
    end
end)

local arenaStage = makeFrame(battleBody, "ArenaStage", UDim2.new(1, 0, 1, -50), UDim2.new(0, 0, 0, 48), Color3.fromRGB(10, 14, 24), 0)
addCorner(arenaStage, 12)
addStroke(arenaStage, PAL.stroke, 1.5)

local roundTitleLbl = makeLabel(arenaStage, "RoundTitle", "Select Story Boss, Infinite Tower, World Raid, or PvP Duel above!", UDim2.new(0.96, 0, 0, 28), UDim2.new(0.02, 0, 0, 6), PAL.gold)
local cutInBanner = makeFrame(arenaStage, "SkillCutInBanner", UDim2.new(0.92, 0, 0, 42), UDim2.new(0.04, 0, 0.44, 0), PAL.accent, 0.08)
addCorner(cutInBanner, 10)
addStroke(cutInBanner, PAL.gold, 2)
cutInBanner.ZIndex = 12
cutInBanner.Visible = false
local cutInLbl = makeLabel(cutInBanner, "CutInText", "", UDim2.new(0.94, 0, 0.85, 0), UDim2.new(0.03, 0, 0.08, 0), PAL.gold)
cutInLbl.ZIndex = 13

local slotWidgets = { left = {}, right = {} }

local function buildArenaSlots(side: string, teamData)
    for _, old in pairs(slotWidgets[side]) do
        if old.frame then old.frame:Destroy() end
    end
    slotWidgets[side] = {}
    for _, u in ipairs(teamData) do
        local xPos = if side == "left" then 0.03 else 0.56
        local yPos = 0.14 + (u.slot - 1) * 0.21
        local card = makeFrame(arenaStage, side .. "_Slot" .. u.slot, UDim2.new(0.41, 0, 0.19, 0), UDim2.new(xPos, 0, yPos, 0), PAL.card, 0)
        addCorner(card, 8)
        local elemColor = CardDatabase.ELEMENT_COLORS[u.element] or PAL.cyan
        addStroke(card, elemColor, 1.5)

        local img = Instance.new("ImageLabel")
        img.Size = UDim2.new(0.18, 0, 0.86, 0)
        img.Position = UDim2.new(0.02, 0, 0.07, 0)
        img.BackgroundTransparency = 1
        img.Image = u.portrait
        img.ScaleType = Enum.ScaleType.Crop
        addCorner(img, 6)
        img.Parent = card

        makeLabel(card, "Name", u.title, UDim2.new(0.55, 0, 0.36, 0), UDim2.new(0.22, 0, 0.06, 0), PAL.text, Enum.TextXAlignment.Left)
        makeLabel(card, "Elem", u.element .. " • " .. u.skillKind, UDim2.new(0.55, 0, 0.25, 0), UDim2.new(0.22, 0, 0.40, 0), elemColor, Enum.TextXAlignment.Left)

        local hpBg = makeFrame(card, "HpBg", UDim2.new(0.74, 0, 0.22, 0), UDim2.new(0.22, 0, 0.70, 0), Color3.fromRGB(30, 35, 50), 0)
        addCorner(hpBg, 5)
        local hpFill = makeFrame(hpBg, "HpFill", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), PAL.emerald, 0)
        addCorner(hpFill, 5)
        local hpTxt = makeLabel(hpBg, "HpTxt", u.hp .. "/" .. u.max_hp, UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), Color3.fromRGB(255, 255, 255))

        slotWidgets[side][u.slot] = {
            frame = card,
            basePos = UDim2.new(xPos, 0, yPos, 0),
            hpFill = hpFill,
            hpTxt = hpTxt,
            maxHp = u.max_hp,
        }
    end
end

local function spawnBattlePopup(targetSlot, text: string, color: Color3)
    if not targetSlot or not targetSlot.frame then
        return
    end
    local pop = makeLabel(arenaStage, "Popup", text, UDim2.new(0.22, 0, 0.07, 0), targetSlot.basePos + UDim2.new(0.10, 0, -0.02, 0), color)
    pop.ZIndex = 15
    TweenService:Create(pop, TweenInfo.new(0.55 / battleSpeed, Enum.EasingStyle.Quad), {
        Position = pop.Position + UDim2.new(0, 0, -0.06, 0),
        TextTransparency = 1,
    }):Play()
    task.delay(0.58 / battleSpeed, function()
        pop:Destroy()
    end)
end

-- ======================== GACHA SUMMON CUTSCENE OVERLAY ========================
local cutscene = makeFrame(rootGui, "GachaCutscene", UDim2.new(1, 0, 1, 0), UDim2.new(0, 0, 0, 0), Color3.fromRGB(6, 8, 16), 0.18)
cutscene.ZIndex = 20
cutscene.Visible = false

local shockwave = makeFrame(cutscene, "Shockwave", UDim2.new(0, 80, 0, 80), UDim2.new(0.5, -40, 0.5, -40), PAL.gold, 0.3)
addCorner(shockwave, 999)
shockwave.ZIndex = 21

local revealCard = makeFrame(cutscene, "RevealCard", UDim2.new(0, 290, 0, 420), UDim2.new(0.5, -145, 0.5, -210), PAL.card, 0)
addCorner(revealCard, 16)
local revealStroke = addStroke(revealCard, PAL.gold, 3)
revealCard.ZIndex = 22

local revealImg = Instance.new("ImageLabel")
revealImg.Size = UDim2.new(0.90, 0, 0.62, 0)
revealImg.Position = UDim2.new(0.05, 0, 0.04, 0)
revealImg.BackgroundTransparency = 1
revealImg.ScaleType = Enum.ScaleType.Crop
revealImg.ZIndex = 23
addCorner(revealImg, 10)
revealImg.Parent = revealCard

local revealRarLbl = makeLabel(revealCard, "RarLbl", "MYTHIC", UDim2.new(0.9, 0, 0.07, 0), UDim2.new(0.05, 0, 0.67, 0), PAL.gold)
revealRarLbl.ZIndex = 23
local revealTitleLbl = makeLabel(revealCard, "TitleLbl", "Card Name", UDim2.new(0.9, 0, 0.09, 0), UDim2.new(0.05, 0, 0.74, 0), PAL.text)
revealTitleLbl.ZIndex = 23
local revealStatsLbl = makeLabel(revealCard, "StatsLbl", "ATK 500 | HP 1600 | SPD 94", UDim2.new(0.9, 0, 0.07, 0), UDim2.new(0.05, 0, 0.83, 0), PAL.muted)
revealStatsLbl.ZIndex = 23
local keepBtn = makeButton(revealCard, "KeepBtn", "KEEP & CONTINUE", UDim2.new(0.44, 0, 0.08, 0), UDim2.new(0.04, 0, 0.91, 0), PAL.emerald)
keepBtn.ZIndex = 24
local equipRevealedBtn = makeButton(revealCard, "EquipBtn", "EQUIP NOW", UDim2.new(0.44, 0, 0.08, 0), UDim2.new(0.52, 0, 0.91, 0), PAL.accent)
equipRevealedBtn.ZIndex = 24

local lastRevealedId = nil
keepBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    cutscene.Visible = false
end)
equipRevealedBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    if lastRevealedId then
        coreAction:FireServer("equip", lastRevealedId)
    end
    cutscene.Visible = false
end)

local function playSummonCutscene(payload)
    if currentState.autoRoll and payload.rarity ~= "Mythic" and payload.rarity ~= "Secret" then
        showToast("Auto-Rolled: [" .. payload.rarity .. "] " .. payload.title .. (if payload.autoDeleted then " (Auto-Sharded)" else ""))
        return
    end
    lastRevealedId = payload.card_id
    local rar = CardDatabase.RARITY_BY_ID[payload.rarity] or CardDatabase.RARITIES[1]
    revealStroke.Color = rar.color
    revealImg.Image = payload.portrait
    revealRarLbl.Text = string.upper(payload.rarity) .. " • " .. payload.border .. " (" .. payload.odds .. ")"
    revealRarLbl.TextColor3 = rar.color
    revealTitleLbl.Text = payload.title .. (if payload.isNew then " [NEW!]" else "")
    revealStatsLbl.Text = "ATK " .. payload.atk .. " | HP " .. payload.hp .. " | " .. payload.skillName

    cutscene.Visible = true
    shockwave.Size = UDim2.new(0, 60, 0, 60)
    shockwave.Position = UDim2.new(0.5, -30, 0.5, -30)
    shockwave.BackgroundColor3 = rar.color
    shockwave.BackgroundTransparency = 0.25

    revealCard.Size = UDim2.new(0, 40, 0, 60)
    revealCard.Position = UDim2.new(0.5, -20, 0.5, -30)
    revealCard.Rotation = -180

    TweenService:Create(shockwave, TweenInfo.new(0.55, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
        Size = UDim2.new(0, 560, 0, 560),
        Position = UDim2.new(0.5, -280, 0.5, -280),
        BackgroundTransparency = 1,
    }):Play()

    TweenService:Create(revealCard, TweenInfo.new(0.45, Enum.EasingStyle.Back, Enum.EasingDirection.Out), {
        Size = UDim2.new(0, 290, 0, 420),
        Position = UDim2.new(0.5, -145, 0.5, -210),
        Rotation = 0,
    }):Play()
end

-- ======================== INTERACTIONS & EVENT WIRING ========================
packBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    local idx = 1
    for i, p in ipairs(CardDatabase.PACKS) do
        if p.id == currentState.selectedPack then
            idx = (i % #CardDatabase.PACKS) + 1
            break
        end
    end
    coreAction:FireServer("select_pack", CardDatabase.PACKS[idx].id)
end)

rollBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    local res = rollFunction:InvokeServer("roll", currentState.selectedPack)
    if typeof(res) == "table" and not res.ok and res.reason == "insufficient_coins" then
        showToast("Need " .. tostring(res.needed) .. " Coins for this pack! Switch to Starter (Free).")
    end
end)

fastRollBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    local res = rollFunction:InvokeServer("fast_roll", currentState.selectedPack)
    if typeof(res) == "table" and not res.ok and res.reason == "insufficient_coins" then
        showToast("Need " .. tostring(res.needed) .. " Coins for this pack!")
    end
end)

autoRollBtn.MouseButton1Click:Connect(function()
    playSfx("ui_click")
    rollFunction:InvokeServer("auto_toggle")
end)

tabDeckBtn.MouseButton1Click:Connect(function() toggleDrawer("Index") end)
tabForgeBtn.MouseButton1Click:Connect(function() toggleDrawer("Forge") end)
tabPotionsBtn.MouseButton1Click:Connect(function() toggleDrawer("Potions") end)
tabUpgradesBtn.MouseButton1Click:Connect(function() toggleDrawer("Upgrades") end)
tabBattleBtn.MouseButton1Click:Connect(function() toggleDrawer("Battle") end)
raidBtn.MouseButton1Click:Connect(function()
    if activeDrawer ~= "Battle" then
        toggleDrawer("Battle")
    end
    if not battlePlaying then
        battleEvent:FireServer("raid", 1)
    end
end)

UserInputService.InputBegan:Connect(function(input, gp)
    if gp then return end
    if input.KeyCode == Enum.KeyCode.R then
        rollFunction:InvokeServer("fast_roll", currentState.selectedPack)
    end
end)

rollNotice.OnClientEvent:Connect(function(payload)
    if typeof(payload) == "table" and payload.ok then
        playSummonCutscene(payload)
    end
end)

coreAction.OnClientEvent:Connect(function(kind, msg)
    if kind == "toast" and typeof(msg) == "string" then
        showToast(msg)
    elseif kind == "potion_used" and typeof(msg) == "string" then
        showToast("Drank " .. msg .. "!")
    end
end)

openPanelEvent.OnClientEvent:Connect(function(panelName)
    if typeof(panelName) == "string" and drawers[panelName] then
        if activeDrawer ~= panelName then
            toggleDrawer(panelName)
        end
    end
end)

weatherRaidEvent.OnClientEvent:Connect(function(kind, payload)
    if kind == "weather_changed" and typeof(payload) == "table" then
        weatherLbl.Text = payload.id .. " (" .. payload.luckMult .. "x)"
        showToast(payload.banner)
    elseif kind == "raid_started" and typeof(payload) == "table" then
        showToast("WORLD RAID SPAWNED: " .. payload.name .. "!")
    elseif kind == "sync" and typeof(payload) == "table" and payload.weather then
        weatherLbl.Text = payload.weather.id .. " (" .. payload.weather.luckMult .. "x)"
    end
end)

-- Live StateFolder observers for Raid Countdown bar
local function updateRaidCountdownLabel()
    local rActive = StateFolder:FindFirstChild("RaidBossActive")
    local rCd = StateFolder:FindFirstChild("RaidCountdown")
    local rHp = StateFolder:FindFirstChild("RaidBossHP")
    local rMax = StateFolder:FindFirstChild("RaidBossMaxHP")
    if rActive and rActive:IsA("BoolValue") and rActive.Value then
        local hp = if rHp and rHp:IsA("IntValue") then rHp.Value else 0
        local mx = if rMax and rMax:IsA("IntValue") then math.max(1, rMax.Value) else 1
        raidBtn.Text = "RAID LIVE: " .. hp .. "/" .. mx
        raidBtn.BackgroundColor3 = PAL.crimson
    else
        local sec = if rCd and rCd:IsA("IntValue") then rCd.Value else 45
        raidBtn.Text = "World Raid in " .. sec .. "s"
        raidBtn.BackgroundColor3 = Color3.fromRGB(90, 28, 36)
    end
end

for _, vName in ipairs({ "RaidBossActive", "RaidCountdown", "RaidBossHP" }) do
    local valObj = StateFolder:FindFirstChild(vName)
    if valObj and (valObj:IsA("BoolValue") or valObj:IsA("IntValue")) then
        valObj.Changed:Connect(updateRaidCountdownLabel)
    end
end
updateRaidCountdownLabel()

stateSync.OnClientEvent:Connect(function(st)
    if typeof(st) ~= "table" then
        return
    end
    currentState = st
    coinsLbl.Text = "Coins: " .. tostring(st.coins)
    shardsLbl.Text = "Shards: " .. tostring(st.shards)
    luckLbl.Text = "Luck: " .. string.format("%.2fx", st.luck or 1.0)
    levelLbl.Text = "Lv." .. tostring(st.level) .. " | Power: " .. tostring(st.teamPower) .. " | F" .. tostring(st.towerFloor)

    local pityRatio = math.clamp((st.pity or 0) / math.max(1, st.pityMax or 50), 0.02, 1.0)
    pityFill.Size = UDim2.new(pityRatio, 0, 1, 0)
    pityLbl.Text = "Mythic+ Pity: " .. tostring(st.pity) .. " / " .. tostring(st.pityMax) .. "  (Rolls: " .. tostring(st.totalRolls) .. ")"

    local packDef = CardDatabase.PACK_BY_ID[st.selectedPack] or CardDatabase.PACKS[1]
    packBtn.Text = "Pack: " .. packDef.id .. (if packDef.cost > 0 then (" (" .. packDef.cost .. " C)") else " (Free)")

    autoRollBtn.Text = if st.autoRoll then "AUTO: ON" else "AUTO: OFF"
    autoRollBtn.BackgroundColor3 = if st.autoRoll then PAL.emerald else PAL.card

    rebuildDeckAndIndex()
    refreshForge()
    refreshUpgrades()
end)

battleEvent.OnClientEvent:Connect(function(kind, data)
    if kind ~= "clash_replay" or typeof(data) ~= "table" then
        return
    end
    battlePlaying = true
    if activeDrawer ~= "Battle" then
        toggleDrawer("Battle")
    end
    roundTitleLbl.Text = data.title
    buildArenaSlots("left", data.leftTeam or {})
    buildArenaSlots("right", data.rightTeam or {})

    task.spawn(function()
        local stepDelay = 0.34 / battleSpeed
        for _, ev in ipairs(data.events or {}) do
            if ev.type == "RoundStart" then
                roundTitleLbl.Text = data.title .. " — Round " .. tostring(ev.round)
                task.wait(stepDelay * 0.7)
            elseif ev.type == "SkillTrigger" then
                cutInLbl.Text = ev.title .. ": " .. ev.skill .. "!"
                cutInBanner.Visible = true
                task.wait(stepDelay * 1.1)
                cutInBanner.Visible = false
            elseif ev.type == "Lunge" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    local offset = if ev.side == "left" then UDim2.new(0.08, 0, 0, 0) else UDim2.new(-0.08, 0, 0, 0)
                    TweenService:Create(w.frame, TweenInfo.new(0.12 / battleSpeed), { Position = w.basePos + offset }):Play()
                    task.wait(0.13 / battleSpeed)
                    TweenService:Create(w.frame, TweenInfo.new(0.12 / battleSpeed), { Position = w.basePos }):Play()
                end
            elseif ev.type == "Damage" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    local ratio = math.clamp(ev.hp / math.max(1, ev.max_hp), 0, 1)
                    w.hpFill.Size = UDim2.new(ratio, 0, 1, 0)
                    w.hpTxt.Text = ev.hp .. "/" .. ev.max_hp .. (if (ev.shield or 0) > 0 then (" [+" .. ev.shield .. "]") else "")
                    local tag = if ev.isCrit then " CRIT!" elseif ev.tag and #ev.tag > 0 then (" " .. ev.tag) else ""
                    spawnBattlePopup(w, "-" .. tostring(ev.amount) .. tag, if ev.isCrit then PAL.gold else PAL.crimson)
                    if ev.isCrit then
                        playSfx("battle_crit")
                    end
                end
                task.wait(stepDelay * 0.65)
            elseif ev.type == "Heal" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    local ratio = math.clamp(ev.hp / math.max(1, ev.max_hp), 0, 1)
                    w.hpFill.Size = UDim2.new(ratio, 0, 1, 0)
                    w.hpTxt.Text = ev.hp .. "/" .. ev.max_hp
                    spawnBattlePopup(w, "+" .. tostring(ev.amount) .. " HEAL", PAL.emerald)
                end
            elseif ev.type == "Shield" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    spawnBattlePopup(w, "+" .. tostring(ev.shield) .. " SHIELD", PAL.cyan)
                end
            elseif ev.type == "Status" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    spawnBattlePopup(w, ev.text, PAL.gold)
                end
            elseif ev.type == "KO" then
                local w = slotWidgets[ev.side] and slotWidgets[ev.side][ev.slot]
                if w then
                    w.frame.BackgroundTransparency = 0.65
                end
            elseif ev.type == "Result" then
                local msg = (if ev.won then "VICTORY! " else "DEFEAT — ") .. "+" .. tostring(ev.coins) .. " Coins, +" .. tostring(ev.shards) .. " Shards"
                roundTitleLbl.Text = msg
                showToast(msg)
            end
        end
        battlePlaying = false
    end)
end)

-- Initial handshake
coreAction:FireServer("sync")
weatherRaidEvent:FireServer("sync")
