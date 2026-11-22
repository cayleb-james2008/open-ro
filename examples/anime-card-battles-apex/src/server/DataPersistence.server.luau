-- DataPersistence.server.luau (ServerScriptService.DataPersistence)
-- Retry-hardened DataStore persistence + OrderedDataStore leaderboards + Studio offline fallback.

local Players = game:GetService("Players")
local DataStoreService = game:GetService("DataStoreService")
local ServerStorage = game:GetService("ServerStorage")
local RunService = game:GetService("RunService")

local PROFILE_STORE_NAME = "ApexAnimeCards_Profiles_v2"
local TOWER_BOARD_NAME = "ApexAnimeCards_Tower_v2"
local POWER_BOARD_NAME = "ApexAnimeCards_Power_v2"

local profileStore = nil
local towerBoard = nil
local powerBoard = nil
local datastoreAvailable = true

local okStore, errStore = pcall(function()
    profileStore = DataStoreService:GetDataStore(PROFILE_STORE_NAME)
    towerBoard = DataStoreService:GetOrderedDataStore(TOWER_BOARD_NAME)
    powerBoard = DataStoreService:GetOrderedDataStore(POWER_BOARD_NAME)
end)

if not okStore or not profileStore then
    datastoreAvailable = false
    if RunService:IsStudio() then
        warn("[DataPersistence] Studio API Services disabled; running session memory fallback: " .. tostring(errStore))
    end
end

local storageBridge = ServerStorage:FindFirstChild("ApexStorageBridge")
if not storageBridge then
    storageBridge = Instance.new("BindableFunction")
    storageBridge.Name = "ApexStorageBridge"
    storageBridge.Parent = ServerStorage
end

local sessionFallback = {}
local liveSnapshots = {}

local function withRetry(fn, attempts)
    local maxTries = attempts or 3
    for attempt = 1, maxTries do
        local ok, result = pcall(fn)
        if ok then
            return true, result
        end
        local msg = tostring(result)
        if string.find(msg, "502") or string.find(msg, "API Services") or string.find(msg, "Studio") then
            datastoreAvailable = false
            return false, msg
        end
        if attempt < maxTries then
            task.wait(2 ^ attempt)
        end
    end
    return false, "max retries exceeded"
end

local function loadProfile(userId: number)
    local key = "Player_" .. tostring(userId)
    if not datastoreAvailable or not profileStore then
        return true, sessionFallback[key]
    end
    local ok, data = withRetry(function()
        return profileStore:GetAsync(key)
    end, 3)
    if not ok then
        if not datastoreAvailable then
            return true, sessionFallback[key]
        end
        return false, nil
    end
    return true, data
end

local function saveProfile(userId: number, payload)
    if typeof(payload) ~= "table" then
        return false
    end
    local key = "Player_" .. tostring(userId)
    sessionFallback[key] = payload
    liveSnapshots[userId] = payload

    if not datastoreAvailable or not profileStore then
        return true
    end
    local ok, err = withRetry(function()
        return profileStore:UpdateAsync(key, function(_old)
            return payload
        end)
    end, 3)
    if not ok and datastoreAvailable then
        warn("[DataPersistence] UpdateAsync failed for " .. key .. ": " .. tostring(err))
        return false
    end
    return true
end

local function submitBoards(userId: number, towerFloor: number, teamPower: number)
    local key = "Player_" .. tostring(userId)
    if not datastoreAvailable then
        return
    end
    if towerBoard and towerFloor >= 1 then
        pcall(function()
            towerBoard:SetAsync(key, math.floor(towerFloor))
        end)
    end
    if powerBoard and teamPower >= 1 then
        pcall(function()
            powerBoard:SetAsync(key, math.floor(teamPower))
        end)
    end
end

local function fetchTopLeaderboard()
    local rows = {}
    if datastoreAvailable and towerBoard then
        local ok, pages = pcall(function()
            return towerBoard:GetSortedAsync(false, 8)
        end)
        if ok and pages then
            for rank, entry in ipairs(pages:GetCurrentPage()) do
                table.insert(rows, {
                    rank = rank,
                    key = entry.key,
                    floor = entry.value,
                })
            end
        end
    end
    if #rows == 0 then
        for _, p in ipairs(Players:GetPlayers()) do
            table.insert(rows, {
                rank = #rows + 1,
                key = p.Name,
                floor = p:GetAttribute("TowerFloor") or 1,
            })
        end
        table.sort(rows, function(a, b)
            return a.floor > b.floor
        end)
    end
    return rows
end

storageBridge.OnInvoke = function(op, arg1, arg2, arg3)
    if op == "load" and typeof(arg1) == "number" then
        local ok, data = loadProfile(arg1)
        return { ok = ok, data = data }
    elseif op == "save" and typeof(arg1) == "number" then
        return saveProfile(arg1, arg2)
    elseif op == "stage" and typeof(arg1) == "number" and typeof(arg2) == "table" then
        liveSnapshots[arg1] = arg2
        return true
    elseif op == "boards" and typeof(arg1) == "number" then
        submitBoards(arg1, tonumber(arg2) or 1, tonumber(arg3) or 0)
        return true
    elseif op == "top_tower" then
        return fetchTopLeaderboard()
    end
    return nil
end

Players.PlayerRemoving:Connect(function(player)
    local snap = liveSnapshots[player.UserId]
    if snap and player:GetAttribute("DataLoaded") == true then
        saveProfile(player.UserId, snap)
        local floor = player:GetAttribute("TowerFloor") or 1
        local power = player:GetAttribute("TeamPower") or 0
        submitBoards(player.UserId, floor, power)
    end
    liveSnapshots[player.UserId] = nil
end)

game:BindToClose(function()
    local active = Players:GetPlayers()
    local remaining = #active
    if remaining == 0 then
        return
    end
    for _, player in ipairs(active) do
        task.spawn(function()
            local snap = liveSnapshots[player.UserId]
            if snap and player:GetAttribute("DataLoaded") == true then
                saveProfile(player.UserId, snap)
                local floor = player:GetAttribute("TowerFloor") or 1
                local power = player:GetAttribute("TeamPower") or 0
                submitBoards(player.UserId, floor, power)
            end
            remaining = remaining - 1
        end)
    end
    local deadline = os.clock() + 20
    while remaining > 0 and os.clock() < deadline do
        task.wait(0.1)
    end
end)
