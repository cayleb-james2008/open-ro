-- CombatMath.luau (ReplicatedStorage.Modules.CombatMath)
-- Shared deterministic combat, trait, charm, and upgrade math for Server & Client.

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Modules = ReplicatedStorage:WaitForChild("Modules")
local CardDatabase = require(Modules:WaitForChild("CardDatabase"))

local CombatMath = {}

function CombatMath.getElementMultiplier(attackerElem: string, defenderElem: string): number
    if CardDatabase.ELEMENT_BEATS[attackerElem] == defenderElem then
        return 1.30
    end
    if CardDatabase.ELEMENT_BEATS[defenderElem] == attackerElem then
        return 0.85
    end
    return 1.0
end

function CombatMath.getBorderMult(borderName: string?): number
    for _, b in ipairs(CardDatabase.BORDERS) do
        if b.id == borderName then
            return b.mult
        end
    end
    return 1.0
end

function CombatMath.computeCardStats(cardDef, ownedEntry)
    local stars = (ownedEntry and ownedEntry.stars) or 1
    local starMult = 1 + (stars - 1) * 0.25
    local borderMult = (ownedEntry and ownedEntry.border_mult) or CombatMath.getBorderMult(ownedEntry and ownedEntry.border)

    local trait = ownedEntry and ownedEntry.trait and CardDatabase.TRAIT_BY_ID[ownedEntry.trait] or nil
    local charm = ownedEntry and ownedEntry.charm and CardDatabase.CHARM_BY_ID[ownedEntry.charm] or nil

    local traitAtk = if trait then trait.atkMult else 1.0
    local traitHp = if trait then trait.hpMult else 1.0
    local traitSpd = if trait then trait.spdBonus else 0
    local charmAtk = if charm then charm.atkFlat else 0
    local charmHp = if charm then charm.hpFlat else 0
    local charmSpd = if charm then charm.spdFlat else 0

    local atk = math.floor(cardDef.atk * starMult * borderMult * traitAtk) + charmAtk
    local hp = math.floor(cardDef.hp * starMult * borderMult * traitHp) + charmHp
    local speed = cardDef.speed + traitSpd + charmSpd

    return {
        atk = atk,
        hp = hp,
        speed = speed,
        critBonus = (if trait then trait.critBonus else 0) + (if charm then charm.critBonus else 0),
        lifesteal = if charm then charm.lifesteal else 0,
        regenPct = if trait then trait.regenPct else 0,
        cdReduce = if trait then trait.cdReduce else 0,
        coinMult = if trait then trait.coinMult else 1.0,
        enemySlow = if charm then charm.enemySlow else 0,
        luckBonus = if charm then charm.luckBonus else 0,
    }
end

function CombatMath.cardPower(cardDef, ownedEntry): number
    local st = CombatMath.computeCardStats(cardDef, ownedEntry)
    return math.floor(st.atk * 1.45 + st.hp * 0.35 + st.speed * 2.5)
end

function CombatMath.getUpgradeCost(upgradeId: string, currentLevel: number): number
    for _, upg in ipairs(CardDatabase.UPGRADES) do
        if upg.id == upgradeId then
            return math.floor(upg.baseCost * (1.55 ^ currentLevel))
        end
    end
    return 999999
end

function CombatMath.xpForNextLevel(level: number): number
    return 80 + (level - 1) * 55
end

return CombatMath
