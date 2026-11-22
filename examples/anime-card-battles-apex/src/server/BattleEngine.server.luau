-- BattleEngine.server.luau (ServerScriptService.BattleEngine)
-- Turn/tick-based 4v4 Auto-Battle Clash Engine supporting:
-- 1. 5 Story Bosses (up to Impossible Overlord)
-- 2. Infinite Tower Floors (escalating rewards + milestone bosses)
-- 3. Timed World Raid Boss (live shared Raid Boss HP & Raid Shards)
-- 4. PvP Duel Challenge (live player deck vs player or Rival Champion Ghost)

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ServerStorage = game:GetService("ServerStorage")

local Modules = ReplicatedStorage:WaitForChild("Modules")
local CardDatabase = require(Modules:WaitForChild("CardDatabase"))
local CombatMath = require(Modules:WaitForChild("CombatMath"))

local Remotes = ReplicatedStorage:WaitForChild("Remotes")
local battleEvent = Remotes:FindFirstChild("BattleEvent")
if not battleEvent then
    battleEvent = Instance.new("RemoteEvent")
    battleEvent.Name = "BattleEvent"
    battleEvent.Parent = Remotes
end

local profileQuery = ServerStorage:WaitForChild("ApexProfileQuery")
local rewardSignal = ServerStorage:WaitForChild("ApexRewardSignal")
local raidDamageBindable = ServerStorage:WaitForChild("ApexRaidDamage")

local rng = Random.new()
local lastReq = {}
local activeLocks = {}
local overdriveState = {}

local function makeUnit(raw, side: string, slot: number)
    return {
        side = side,
        slot = slot,
        card_id = raw.card_id or ("enemy-" .. slot),
        title = raw.title,
        element = raw.element or "Solar",
        portrait = raw.portrait or "rbxasset://openro-assets/crimson-sea-captain.png",
        atk = raw.atk or 150,
        hp = raw.hp or 700,
        max_hp = raw.hp or 700,
        speed = raw.speed or 80,
        skillName = raw.skillName or raw.skillKind or "Strike",
        skillKind = raw.skillKind or "Burn AoE",
        maxCd = raw.cooldown or 3,
        cd = 1,
        shield = 0,
        burnTurns = 0,
        stunned = false,
        counterReady = false,
        atkBuffTurns = 0,
        critBonus = raw.critBonus or 0.12,
        lifesteal = raw.lifesteal or 0,
        regenPct = raw.regenPct or 0,
        coinMult = raw.coinMult or 1.0,
        enemySlow = raw.enemySlow or 0,
    }
end

local function buildPlayerTeam(player: Player)
    local snap = profileQuery:Invoke("deck_units", player)
    local team = {}
    if typeof(snap) == "table" and typeof(snap.units) == "table" then
        for i, u in ipairs(snap.units) do
            if i <= 4 then
                table.insert(team, makeUnit(u, "left", i))
            end
        end
    end
    if #team == 0 then
        local starter = CardDatabase.CARD_BY_ID["eclipse-arcane-alchemist"]
        table.insert(team, makeUnit(starter, "left", 1))
    end
    return team, snap or { towerFloor = 1, storyCleared = 0 }
end

local function buildTowerWave(floorNum: number)
    local scale = 1 + (floorNum - 1) * 0.22
    local isBoss = (floorNum % 10 == 0)
    local pool = { "crimson-sea-captain", "infinity-void-sorcerer", "thunder-flash-ronin", "shadow-sovereign", "blood-moon-gravity-ronin", "nine-tail-ember-shinobi" }
    local wave = {}
    for i = 1, 4 do
        local cid = pool[((floorNum + i - 2) % #pool) + 1]
        local def = CardDatabase.CARD_BY_ID[cid] or CardDatabase.CARDS[1]
        local bossBoost = if isBoss and i == 4 then 1.55 else 1.0
        table.insert(wave, makeUnit({
            card_id = def.id,
            title = (if isBoss and i == 4 then "Floor " .. floorNum .. " Overlord" else def.title .. " F" .. floorNum),
            element = def.element,
            portrait = def.portrait,
            atk = math.floor(def.atk * 0.42 * scale * bossBoost),
            hp = math.floor(def.hp * 0.48 * scale * bossBoost),
            speed = math.min(115, def.speed + math.floor(floorNum / 2)),
            skillKind = def.skillKind,
            skillName = def.skillName,
            cooldown = def.cooldown,
        }, "right", i))
    end
    return wave
end

local function buildRaidWave()
    local rb = CardDatabase.WORLD_RAID_BOSS
    return {
        makeUnit({
            title = "Eclipse Herald I", element = "Shadow", atk = 240, hp = 1650, speed = 86,
            skillKind = "Guard Break", skillName = "Void Rend", portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png",
        }, "right", 1),
        makeUnit({
            title = "Abyssal Guardian", element = "Arcane", atk = 260, hp = 1900, speed = 84,
            skillKind = "Mirror Shield", skillName = "Prism Wall", portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png",
        }, "right", 2),
        makeUnit({
            title = rb.name, element = rb.element, atk = rb.atk, hp = 4200, speed = rb.speed,
            skillKind = "Burn AoE", skillName = "Cataclysm Pulse", portrait = rb.portrait,
        }, "right", 3),
        makeUnit({
            title = "Eclipse Herald II", element = "Storm", atk = 250, hp = 1580, speed = 91,
            skillKind = "Stun", skillName = "Dark Thunder", portrait = "rbxasset://openro-assets/thunder-flash-ronin.png",
        }, "right", 4),
    }
end

local function buildPvpWave(challenger: Player)
    local targetPlayer = nil
    for _, p in ipairs(Players:GetPlayers()) do
        if p ~= challenger then
            targetPlayer = p
            break
        end
    end
    if targetPlayer then
        local snap = profileQuery:Invoke("deck_units", targetPlayer)
        if typeof(snap) == "table" and typeof(snap.units) == "table" and #snap.units > 0 then
            local wave = {}
            for i, u in ipairs(snap.units) do
                if i <= 4 then
                    table.insert(wave, makeUnit(u, "right", i))
                end
            end
            return wave, targetPlayer.Name
        end
    end
    -- Solo Studio fallback: Rival Champion Ghost Deck
    local ghostIds = { "thunder-flash-ronin", "crimson-sea-captain", "blood-moon-gravity-ronin", "eclipse-arcane-alchemist" }
    local wave = {}
    for i, gid in ipairs(ghostIds) do
        local def = CardDatabase.CARD_BY_ID[gid]
        table.insert(wave, makeUnit({
            card_id = def.id,
            title = "Rival " .. def.title,
            element = def.element,
            portrait = def.portrait,
            atk = math.floor(def.atk * 0.72),
            hp = math.floor(def.hp * 0.78),
            speed = def.speed,
            skillKind = def.skillKind,
            skillName = def.skillName,
            cooldown = def.cooldown,
        }, "right", i))
    end
    return wave, "Arena Grandmaster Ghost"
end

local function livingUnits(team)
    local list = {}
    for _, u in ipairs(team) do
        if u.hp > 0 then
            table.insert(list, u)
        end
    end
    return list
end

local function runMatch(player: Player, mode: string, argVal: number?)
    if activeLocks[player] then
        return
    end
    activeLocks[player] = true

    local leftTeam, snap = buildPlayerTeam(player)
    local rightTeam = {}
    local title = "Battle Clash"
    local rewardCoins = 120
    local rewardShards = 8
    local bossId = 0
    local targetFloor = snap.towerFloor or 1

    if mode == "story" then
        bossId = math.clamp(math.floor(argVal or 1), 1, #CardDatabase.STORY_BOSSES)
        local bossDef = CardDatabase.STORY_BOSSES[bossId]
        title = "Story Boss " .. bossId .. ": " .. bossDef.name
        rewardCoins = bossDef.rewardCoins
        rewardShards = bossDef.rewardShards
        for i, u in ipairs(bossDef.team) do
            table.insert(rightTeam, makeUnit(u, "right", i))
        end
    elseif mode == "tower" then
        targetFloor = math.max(1, math.floor(argVal or snap.towerFloor or 1))
        rightTeam = buildTowerWave(targetFloor)
        title = "Infinite Tower Floor " .. targetFloor
        rewardCoins = 110 + targetFloor * 45
        rewardShards = 6 + math.floor(targetFloor / 2) * 3
    elseif mode == "raid" then
        rightTeam = buildRaidWave()
        title = "World Raid: " .. CardDatabase.WORLD_RAID_BOSS.name
        rewardCoins = 320
        rewardShards = 35
    elseif mode == "pvp" then
        local rivalName
        rightTeam, rivalName = buildPvpWave(player)
        title = "PvP Duel vs " .. rivalName
        rewardCoins = 260
        rewardShards = 18
    else
        activeLocks[player] = nil
        return
    end

    -- Apply Charm enemy slow passive
    local totalSlow = 0
    for _, u in ipairs(leftTeam) do
        totalSlow = totalSlow + (u.enemySlow or 0)
    end
    if totalSlow > 0 then
        for _, e in ipairs(rightTeam) do
            e.speed = math.max(30, math.floor(e.speed * (1 - math.min(0.45, totalSlow))))
        end
    end

    local events = {}
    local function pushEvent(ev)
        table.insert(events, ev)
    end

    -- Snapshot initial units for client rendering
    local function serializeTeam(t)
        local out = {}
        for _, u in ipairs(t) do
            table.insert(out, {
                slot = u.slot, side = u.side, title = u.title, element = u.element,
                portrait = u.portrait, hp = u.hp, max_hp = u.max_hp, atk = u.atk, speed = u.speed,
                skillName = u.skillName, skillKind = u.skillKind,
            })
        end
        return out
    end

    local initialLeft = serializeTeam(leftTeam)
    local initialRight = serializeTeam(rightTeam)

    -- Round-start passives
    for _, u in ipairs(leftTeam) do
        if u.skillKind == "Mirror Shield" then
            for _, ally in ipairs(leftTeam) do
                ally.shield = ally.shield + math.floor(ally.max_hp * 0.14)
            end
            pushEvent({ type = "SkillTrigger", side = "left", slot = u.slot, title = u.title, skill = "Opening Mirror Shield" })
        elseif u.skillKind == "Domain Buff" then
            for _, ally in ipairs(leftTeam) do
                ally.atkBuffTurns = 2
            end
            pushEvent({ type = "SkillTrigger", side = "left", slot = u.slot, title = u.title, skill = "Opening Domain Buff" })
        end
    end

    local totalRaidDmg = 0
    local overdriveDone = false
    local maxRounds = 12
    for round = 1, maxRounds do
        local aliveLeft = livingUnits(leftTeam)
        local aliveRight = livingUnits(rightTeam)
        if #aliveLeft == 0 or #aliveRight == 0 then
            break
        end

        pushEvent({ type = "RoundStart", round = round })

        -- Taste Twist: CHAIN OVERDRIVE interactive timing burst before Round 2
        if round == 2 and not overdriveDone then
            overdriveDone = true
            local leadA = aliveLeft[1]
            local leadB = aliveLeft[2] or aliveLeft[1]
            overdriveState[player] = "open"
            battleEvent:FireClient(player, "overdrive_window", {
                title = title,
                duration = 1.6,
                pair = { leadA.title, leadB.title },
            })
            local deadline = os.clock() + 1.6
            while player.Parent and overdriveState[player] == "open" and os.clock() < deadline do
                task.wait(0.05)
            end
            local didTrigger = (overdriveState[player] == "triggered")
            overdriveState[player] = nil
            if didTrigger then
                local dualDmg = math.max(50, math.floor((leadA.atk + leadB.atk * 0.65) * 1.35))
                pushEvent({
                    type = "OverdriveBurst",
                    title = leadA.title .. " x " .. leadB.title,
                    bonusPct = 35,
                    slotA = leadA.slot,
                    slotB = leadB.slot,
                })
                pushEvent({ type = "Lunge", side = "left", slot = leadA.slot, targetSide = "right", targetSlot = aliveRight[1].slot })
                if leadB.slot ~= leadA.slot then
                    pushEvent({ type = "Lunge", side = "left", slot = leadB.slot, targetSide = "right", targetSlot = aliveRight[1].slot })
                end
                for _, foe in ipairs(livingUnits(rightTeam)) do
                    local actual = dualDmg
                    if foe.shield > 0 then
                        local absorbed = math.min(foe.shield, actual)
                        foe.shield = foe.shield - absorbed
                        actual = actual - absorbed
                    end
                    foe.hp = math.max(0, foe.hp - actual)
                    totalRaidDmg = totalRaidDmg + dualDmg
                    pushEvent({
                        type = "Damage",
                        side = foe.side,
                        slot = foe.slot,
                        amount = dualDmg,
                        isCrit = true,
                        tag = "OVERDRIVE +35%!",
                        hp = foe.hp,
                        max_hp = foe.max_hp,
                        shield = foe.shield,
                    })
                    if foe.hp <= 0 then
                        pushEvent({ type = "KO", side = foe.side, slot = foe.slot, title = foe.title })
                    end
                end
                aliveRight = livingUnits(rightTeam)
                if #aliveRight == 0 then
                    break
                end
            end
        end

        local order = {}
        for _, u in ipairs(aliveLeft) do table.insert(order, u) end
        for _, u in ipairs(aliveRight) do table.insert(order, u) end
        table.sort(order, function(a, b)
            return a.speed > b.speed
        end)

        for _, actor in ipairs(order) do
            if actor.hp <= 0 then
                continue
            end
            local foes = if actor.side == "left" then livingUnits(rightTeam) else livingUnits(leftTeam)
            local allies = if actor.side == "left" then livingUnits(leftTeam) else livingUnits(rightTeam)
            if #foes == 0 then
                break
            end

            if actor.regenPct > 0 then
                local regen = math.floor(actor.max_hp * actor.regenPct)
                actor.hp = math.min(actor.max_hp, actor.hp + regen)
                pushEvent({ type = "Heal", side = actor.side, slot = actor.slot, amount = regen, hp = actor.hp, max_hp = actor.max_hp })
            end

            if actor.burnTurns > 0 then
                actor.burnTurns = actor.burnTurns - 1
                local bDmg = math.max(15, math.floor(actor.max_hp * 0.06))
                actor.hp = math.max(0, actor.hp - bDmg)
                pushEvent({ type = "Damage", side = actor.side, slot = actor.slot, amount = bDmg, isCrit = false, tag = "BURN", hp = actor.hp, max_hp = actor.max_hp, shield = actor.shield })
                if actor.hp <= 0 then
                    pushEvent({ type = "KO", side = actor.side, slot = actor.slot, title = actor.title })
                    continue
                end
            end

            if actor.stunned then
                actor.stunned = false
                pushEvent({ type = "Status", side = actor.side, slot = actor.slot, text = "STUNNED!" })
                continue
            end

            actor.cd = actor.cd - 1
            local useSkill = actor.cd <= 0
            if useSkill then
                actor.cd = actor.maxCd
                pushEvent({ type = "SkillTrigger", side = actor.side, slot = actor.slot, title = actor.title, skill = actor.skillName .. " [" .. actor.skillKind .. "]" })
            end

            local target = foes[1]
            pushEvent({ type = "Lunge", side = actor.side, slot = actor.slot, targetSide = target.side, targetSlot = target.slot })

            local elemMult = CombatMath.getElementMultiplier(actor.element, target.element)
            local isCrit = rng:NextNumber() <= actor.critBonus
            local critMult = if isCrit then 1.65 else 1.0
            local buffMult = if actor.atkBuffTurns > 0 then 1.25 else 1.0
            if actor.atkBuffTurns > 0 then
                actor.atkBuffTurns = actor.atkBuffTurns - 1
            end

            local skillMult = if useSkill then 1.55 else 1.0
            local rawDmg = math.max(10, math.floor(actor.atk * elemMult * critMult * buffMult * skillMult))

            local function applyHit(victim, dmgAmount: number, ignoreShield: boolean)
                local actual = dmgAmount
                if not ignoreShield and victim.shield > 0 then
                    local absorbed = math.min(victim.shield, actual)
                    victim.shield = victim.shield - absorbed
                    actual = actual - absorbed
                end
                victim.hp = math.max(0, victim.hp - actual)
                if actor.side == "left" then
                    totalRaidDmg = totalRaidDmg + dmgAmount
                end
                pushEvent({
                    type = "Damage",
                    side = victim.side,
                    slot = victim.slot,
                    amount = dmgAmount,
                    isCrit = isCrit,
                    tag = if elemMult > 1.0 then "ADV!" else "",
                    hp = victim.hp,
                    max_hp = victim.max_hp,
                    shield = victim.shield,
                })
                if victim.counterReady and victim.hp > 0 then
                    victim.counterReady = false
                    local reflect = math.floor(dmgAmount * 0.45)
                    actor.hp = math.max(0, actor.hp - reflect)
                    pushEvent({ type = "Damage", side = actor.side, slot = actor.slot, amount = reflect, isCrit = false, tag = "COUNTER!", hp = actor.hp, max_hp = actor.max_hp, shield = actor.shield })
                    if actor.hp <= 0 then
                        pushEvent({ type = "KO", side = actor.side, slot = actor.slot, title = actor.title })
                    end
                end
                if victim.hp <= 0 then
                    pushEvent({ type = "KO", side = victim.side, slot = victim.slot, title = victim.title })
                end
            end

            if useSkill and actor.skillKind == "Burn AoE" then
                for _, foe in ipairs(foes) do
                    applyHit(foe, math.floor(rawDmg * 0.68), false)
                    foe.burnTurns = 2
                end
            elseif useSkill and actor.skillKind == "Guard Break" then
                target.shield = 0
                applyHit(target, math.floor(rawDmg * 1.15), true)
            elseif useSkill and actor.skillKind == "Counter" then
                actor.counterReady = true
                applyHit(target, rawDmg, false)
            elseif useSkill and actor.skillKind == "Mirror Shield" then
                for _, ally in ipairs(allies) do
                    ally.shield = ally.shield + math.floor(ally.max_hp * 0.20)
                    pushEvent({ type = "Shield", side = ally.side, slot = ally.slot, shield = ally.shield })
                end
                applyHit(target, math.floor(rawDmg * 0.80), false)
            elseif useSkill and actor.skillKind == "Stun" then
                applyHit(target, rawDmg, false)
                if target.hp > 0 and rng:NextNumber() <= 0.65 then
                    target.stunned = true
                    pushEvent({ type = "Status", side = target.side, slot = target.slot, text = "STUNNED!" })
                end
            elseif useSkill and actor.skillKind == "Time Stop" then
                applyHit(target, rawDmg, false)
                for _, foe in ipairs(foes) do
                    foe.cd = foe.cd + 1
                    foe.stunned = true
                end
                pushEvent({ type = "Status", side = target.side, slot = target.slot, text = "TIME FROZEN!" })
            elseif useSkill and actor.skillKind == "Domain Buff" then
                for _, ally in ipairs(allies) do
                    ally.atkBuffTurns = 2
                    local heal = math.floor(ally.max_hp * 0.10)
                    ally.hp = math.min(ally.max_hp, ally.hp + heal)
                    pushEvent({ type = "Heal", side = ally.side, slot = ally.slot, amount = heal, hp = ally.hp, max_hp = ally.max_hp })
                end
                applyHit(target, rawDmg, false)
            else
                applyHit(target, rawDmg, false)
            end

            local totalLifesteal = actor.lifesteal + (if useSkill and actor.skillKind == "Lifesteal" then 0.35 else 0)
            if totalLifesteal > 0 and actor.hp > 0 then
                local healAmt = math.floor(rawDmg * totalLifesteal)
                actor.hp = math.min(actor.max_hp, actor.hp + healAmt)
                pushEvent({ type = "Heal", side = actor.side, slot = actor.slot, amount = healAmt, hp = actor.hp, max_hp = actor.max_hp })
            end
        end
    end

    local leftWon = #livingUnits(rightTeam) == 0 and #livingUnits(leftTeam) > 0
    local coinMult = 1.0
    for _, u in ipairs(leftTeam) do
        coinMult = math.max(coinMult, u.coinMult or 1.0)
    end

    local grantedCoins = 0
    local grantedShards = 0
    local grantedTokens = 0
    if mode == "raid" then
        local raidRes = raidDamageBindable:Invoke(player, totalRaidDmg)
        grantedCoins = math.floor((120 + totalRaidDmg / 18) * coinMult)
        grantedShards = math.max(10, math.floor(totalRaidDmg / 300))
        grantedTokens = if leftWon then 5 else 2
        rewardSignal:Fire(player, {
            coins = grantedCoins,
            shards = grantedShards,
            raidTokens = grantedTokens,
            xp = 95,
            reason = "Dealt " .. totalRaidDmg .. " Raid DMG! (Boss HP: " .. tostring(raidRes.hp or 0) .. ")",
        })
    elseif leftWon then
        grantedCoins = math.floor(rewardCoins * coinMult)
        grantedShards = rewardShards
        local payload = {
            coins = grantedCoins,
            shards = grantedShards,
            xp = 75,
            reason = title .. " Victory! +" .. grantedCoins .. " Coins, +" .. grantedShards .. " Shards",
        }
        if mode == "tower" then
            payload.towerFloor = targetFloor + 1
        elseif mode == "story" then
            payload.storyCleared = bossId
        end
        rewardSignal:Fire(player, payload)
    else
        grantedCoins = 35
        rewardSignal:Fire(player, {
            coins = grantedCoins,
            xp = 25,
            reason = title .. " Defeat — +35 Training Coins",
        })
    end

    pushEvent({
        type = "Result",
        won = leftWon,
        title = title,
        coins = grantedCoins,
        shards = grantedShards,
        raidTokens = grantedTokens,
        raidDmg = totalRaidDmg,
    })

    battleEvent:FireClient(player, "clash_replay", {
        mode = mode,
        title = title,
        leftTeam = initialLeft,
        rightTeam = initialRight,
        events = events,
        won = leftWon,
        coins = grantedCoins,
        shards = grantedShards,
    })

    overdriveState[player] = nil
    activeLocks[player] = nil
end

battleEvent.OnServerEvent:Connect(function(player, mode, argVal)
    if typeof(mode) ~= "string" then
        return
    end
    if mode == "Overdrive" then
        if overdriveState[player] == "open" then
            overdriveState[player] = "triggered"
        end
        return
    end
    local now = os.clock()
    if lastReq[player] and (now - lastReq[player]) < 0.8 then
        return
    end
    lastReq[player] = now
    local numArg = if typeof(argVal) == "number" and argVal == argVal then argVal else nil
    task.spawn(runMatch, player, mode, numArg)
end)

Players.PlayerRemoving:Connect(function(player)
    lastReq[player] = nil
    activeLocks[player] = nil
    overdriveState[player] = nil
end)
