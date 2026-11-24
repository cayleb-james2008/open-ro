--!strict
local Rarities = require(script.Parent.Config.Rarities)
local Cards = require(script.Parent.Config.Cards)
local Traits = require(script.Parent.Config.Traits)
local okBonds, loadedBonds = pcall(function()
	return require((script.Parent.Config :: any).Bonds)
end)
local Bonds: any = if okBonds and type(loadedBonds) == "table" then loadedBonds else { List = {} }
local LuckMath = require(script.Parent.LuckMath)

export type OwnedCardInput = {
	uid: string?,
	cardId: string,
	level: number?,
	mutation: string?,
	trait: string?,
	ascension: number?,
}

export type EnemySpec = {
	id: string,
	name: string,
	rarity: Rarities.RarityId?,
	role: Cards.RoleId?,
	showName: string?,
	hp: number,
	atk: number,
	spd: number,
	skill: Cards.SkillDef?,
	passiveFlags: { [string]: any }?,
	artKey: string?,
}

export type CombatUnit = {
	uid: string,
	team: "ally" | "enemy",
	slot: number,
	cardId: string,
	name: string,
	rarity: Rarities.RarityId,
	role: Cards.RoleId,
	showName: string,
	artKey: string,
	maxHp: number,
	hp: number,
	shield: number,
	atk: number,
	spd: number,
	cooldownRemaining: number,
	skill: Cards.SkillDef,
	passiveFlags: { [string]: any },
	trait: Traits.TraitDef?,
	stunned: number,
	silenced: number,
	frozen: number,
	burnTurns: number,
	burnDmg: number,
	bleedTurns: number,
	bleedDmg: number,
	defShred: number,
	atkBuff: number,
	resurrected: boolean,
	alive: boolean,
	damageDealt: number,
}

export type SquadBonuses = {
	atkPct: number?,
	hpPct: number?,
	spdPct: number?,
	critPct: number?,
	defPct: number?,
}

export type BondEvaluation = {
	activeBonds: { Bonds.BondDef },
	labels: { string },
	atkMult: number,
	hpMult: number,
	spdMult: number,
	critBonus: number,
	defBonus: number,
}

export type SynergyResult = {
	labels: { string },
	atkMult: number,
	hpMult: number,
	spdMult: number,
	critBonus: number,
	defBonus: number,
	leafCount: number,
	activeBonds: { string },
}

export type BattleEvent = {
	kind: string,
	round: number,
	actorUid: string?,
	targetUid: string?,
	skillName: string?,
	isSkill: boolean?,
	damage: number?,
	heal: number?,
	isCrit: boolean?,
	killed: boolean?,
	targetHp: number?,
	targetMaxHp: number?,
	targetShield: number?,
	note: string?,
}

export type UnitSummary = {
	uid: string,
	name: string,
	rarity: Rarities.RarityId,
	hp: number,
	maxHp: number,
	alive: boolean,
	damageDealt: number,
}

export type BattleResult = {
	won: boolean,
	rounds: number,
	events: { BattleEvent },
	alliesSummary: { UnitSummary },
	enemiesSummary: { UnitSummary },
	synergies: { string },
	mvpName: string,
}

local BattleSim = {}

local DEFAULT_ENEMY_SKILL: Cards.SkillDef = {
	name = "Shadow Strike",
	description = "145% ATK strike.",
	cooldown = 3,
	dmgMult = 1.45,
	targeting = "Single",
}

function BattleSim.buildPlayerUnit(slot: number, owned: OwnedCardInput): CombatUnit?
	local cardDef = Cards.get(owned.cardId)
	if not cardDef then
		return nil
	end
	local stats = LuckMath.computeCardStats(
		cardDef,
		owned.level or 1,
		owned.mutation or "Normal",
		owned.trait,
		owned.ascension or 0
	)
	local traitDef = Traits.get(owned.trait)
	return {
		uid = owned.uid or ("ally_" .. tostring(slot)),
		team = "ally",
		slot = slot,
		cardId = cardDef.id,
		name = cardDef.displayName,
		rarity = cardDef.rarity,
		role = cardDef.role,
		showName = cardDef.showName,
		artKey = cardDef.artKey,
		maxHp = stats.hp,
		hp = stats.hp,
		shield = 0,
		atk = stats.atk,
		spd = stats.spd,
		cooldownRemaining = 1,
		skill = cardDef.skill,
		passiveFlags = cardDef.passiveFlags or {},
		trait = traitDef,
		stunned = 0,
		silenced = 0,
		frozen = 0,
		burnTurns = 0,
		burnDmg = 0,
		bleedTurns = 0,
		bleedDmg = 0,
		defShred = 0,
		atkBuff = 0,
		resurrected = false,
		alive = true,
		damageDealt = 0,
	}
end

function BattleSim.buildEnemyUnit(slot: number, spec: EnemySpec): CombatUnit
	local rarity: Rarities.RarityId = spec.rarity or "Common"
	local role: Cards.RoleId = spec.role or "Striker"
	return {
		uid = "enemy_" .. tostring(slot),
		team = "enemy",
		slot = slot,
		cardId = spec.id,
		name = spec.name,
		rarity = rarity,
		role = role,
		showName = spec.showName or "World Foe",
		artKey = spec.artKey or ("cards/" .. spec.id),
		maxHp = math.max(1, math.floor(spec.hp)),
		hp = math.max(1, math.floor(spec.hp)),
		shield = 0,
		atk = math.max(1, math.floor(spec.atk)),
		spd = math.max(1, math.floor(spec.spd)),
		cooldownRemaining = 1,
		skill = spec.skill or DEFAULT_ENEMY_SKILL,
		passiveFlags = spec.passiveFlags or {},
		trait = nil,
		stunned = 0,
		silenced = 0,
		frozen = 0,
		burnTurns = 0,
		burnDmg = 0,
		bleedTurns = 0,
		bleedDmg = 0,
		defShred = 0,
		atkBuff = 0,
		resurrected = false,
		alive = true,
		damageDealt = 0,
	}
end

function BattleSim.evaluateBonds(cardIds: { string }): BondEvaluation
	local showCounts: { [string]: number } = {}
	local cardSet: { [string]: boolean } = {}
	for _, cardId in ipairs(cardIds) do
		cardSet[cardId] = true
		local cardDef = Cards.get(cardId)
		if cardDef then
			showCounts[cardDef.showName] = (showCounts[cardDef.showName] or 0) + 1
		end
	end

	local activeBonds: { any } = {}
	local labels: { string } = {}
	local atkMult = 1.0
	local hpMult = 1.0
	local spdMult = 1.0
	local critBonus = 0.0
	local defBonus = 0.0

	for _, bond in ipairs(Bonds.List) do
		local matched = false
		if bond.kind == "Show" then
			local show = bond.show or ""
			local minCount = bond.minCount or 2
			if (showCounts[show] or 0) >= minCount then
				matched = true
			end
		elseif bond.kind == "CrossCard" then
			local reqs = bond.requiredCards
			if reqs and #reqs > 0 then
				matched = true
				for _, reqId in ipairs(reqs) do
					if not cardSet[reqId] then
						matched = false
						break
					end
				end
			end
		end
		if matched then
			table.insert(activeBonds, bond)
			table.insert(labels, bond.name .. ": " .. bond.description)
			atkMult += bond.bonus.atkPct or 0
			hpMult += bond.bonus.hpPct or 0
			spdMult += bond.bonus.spdPct or 0
			critBonus += bond.bonus.critPct or 0
			defBonus += bond.bonus.defPct or 0
		end
	end

	return {
		activeBonds = activeBonds,
		labels = labels,
		atkMult = atkMult,
		hpMult = hpMult,
		spdMult = spdMult,
		critBonus = critBonus,
		defBonus = defBonus,
	}
end

function BattleSim.computeSynergies(units: { CombatUnit }): SynergyResult
	local showCounts: { [string]: number } = {}
	local cardIds: { string } = {}
	local roleSet: { [string]: boolean } = {}
	local hasVanguard = false
	local hasStriker = false
	local roleCount = 0
	local maxShowCount = 0
	local topShowName = ""

	for _, unit in ipairs(units) do
		table.insert(cardIds, unit.cardId)
		local sCount = (showCounts[unit.showName] or 0) + 1
		showCounts[unit.showName] = sCount
		if sCount > maxShowCount then
			maxShowCount = sCount
			topShowName = unit.showName
		end
		if not roleSet[unit.role] then
			roleSet[unit.role] = true
			roleCount += 1
		end
		if unit.role == "Vanguard" then
			hasVanguard = true
		elseif unit.role == "Striker" then
			hasStriker = true
		end
	end

	local labels: { string } = {}
	local atkMult = 1.0
	local hpMult = 1.0
	local spdMult = 1.0
	local critBonus = 0.0
	local defBonus = 0.0

	if maxShowCount >= 4 then
		atkMult += 0.25
		hpMult += 0.15
		table.insert(labels, topShowName .. " x" .. tostring(maxShowCount) .. " (+25% ATK, +15% HP)")
	elseif maxShowCount == 3 then
		atkMult += 0.15
		hpMult += 0.10
		table.insert(labels, topShowName .. " x3 (+15% ATK, +10% HP)")
	elseif maxShowCount == 2 then
		atkMult += 0.08
		table.insert(labels, topShowName .. " x2 (+8% ATK)")
	end

	if roleCount >= 3 then
		atkMult += 0.10
		hpMult += 0.10
		spdMult += 0.06
		table.insert(labels, "Balanced Squad (+10% ATK/HP, +6% SPD)")
	end

	if hasVanguard and hasStriker then
		critBonus += 0.08
		table.insert(labels, "Spear & Shield (+8% Crit)")
	end

	local bondEval = BattleSim.evaluateBonds(cardIds)
	local activeBonds: { string } = {}
	for _, bond in ipairs(bondEval.activeBonds) do
		table.insert(activeBonds, bond.id)
		table.insert(labels, "Bond: " .. bond.name)
	end
	atkMult += bondEval.atkMult - 1.0
	hpMult += bondEval.hpMult - 1.0
	spdMult += bondEval.spdMult - 1.0
	critBonus += bondEval.critBonus
	defBonus += bondEval.defBonus

	return {
		labels = labels,
		atkMult = atkMult,
		hpMult = hpMult,
		spdMult = spdMult,
		critBonus = critBonus,
		defBonus = defBonus,
		leafCount = showCounts["Leaf Shinobi"] or 0,
		activeBonds = activeBonds,
	}
end

local function countAlive(units: { CombatUnit }): number
	local total = 0
	for _, u in ipairs(units) do
		if u.alive then
			total += 1
		end
	end
	return total
end

local function getLiving(units: { CombatUnit }): { CombatUnit }
	local out: { CombatUnit } = {}
	for _, u in ipairs(units) do
		if u.alive then
			table.insert(out, u)
		end
	end
	return out
end

local function applyDamage(
	target: CombatUnit,
	rawDmg: number,
	ignoreShield: boolean,
	roundNum: number,
	events: { BattleEvent }
): (number, boolean)
	local dmg = math.max(1, math.floor(rawDmg))
	local remaining = dmg
	if not ignoreShield and target.shield > 0 then
		local absorbed = math.min(target.shield, remaining)
		target.shield -= absorbed
		remaining -= absorbed
	end
	target.hp = math.max(0, target.hp - remaining)
	local killed = false
	if target.hp <= 0 then
		local resPct = target.passiveFlags.resurrectPct or 0
		if resPct > 0 and not target.resurrected then
			target.resurrected = true
			target.hp = math.max(1, math.floor(target.maxHp * resPct))
			table.insert(events, {
				kind = "resurrect",
				round = roundNum,
				actorUid = target.uid,
				targetUid = target.uid,
				targetHp = target.hp,
				targetMaxHp = target.maxHp,
				note = target.name .. " revived!",
			})
		else
			target.alive = false
			killed = true
		end
	end
	return dmg, killed
end

local function applyTeamBuffs(
	allies: { CombatUnit },
	enemies: { CombatUnit },
	synergy: SynergyResult,
	squadBonuses: SquadBonuses?
)
	local extraHp = if squadBonuses and squadBonuses.hpPct then math.max(0, squadBonuses.hpPct) else 0
	local extraAtk = if squadBonuses and squadBonuses.atkPct then math.max(0, squadBonuses.atkPct) else 0
	local extraSpd = if squadBonuses and squadBonuses.spdPct then math.max(0, squadBonuses.spdPct) else 0
	local enemySpdDebuff = 0
	for _, ally in ipairs(allies) do
		local hpBoost = synergy.hpMult + extraHp
		local atkBoost = synergy.atkMult + extraAtk
		if (ally.passiveFlags.leafAtkSynergy or 0) > 0 and synergy.leafCount >= 2 then
			atkBoost += ally.passiveFlags.leafAtkSynergy
		end
		ally.maxHp = math.max(1, math.floor(ally.maxHp * hpBoost))
		ally.hp = ally.maxHp
		ally.atk = math.max(1, math.floor(ally.atk * atkBoost))
		ally.spd = math.max(1, math.floor(ally.spd * (synergy.spdMult + extraSpd)))
		enemySpdDebuff = math.max(enemySpdDebuff, ally.passiveFlags.enemySpdDebuff or 0)
	end
	if enemySpdDebuff > 0 then
		for _, enemy in ipairs(enemies) do
			enemy.spd = math.max(1, math.floor(enemy.spd * (1 - enemySpdDebuff)))
		end
	end
end

function BattleSim.run(
	alliesInput: { CombatUnit },
	enemiesInput: { CombatUnit },
	opts: { seed: number?, maxRounds: number?, squadBonuses: SquadBonuses? }?
): BattleResult
	local cfg = opts or {}
	local extraCrit = if cfg.squadBonuses and cfg.squadBonuses.critPct then math.max(0, cfg.squadBonuses.critPct) else 0
	local extraDef = if cfg.squadBonuses and cfg.squadBonuses.defPct then math.max(0, cfg.squadBonuses.defPct) else 0
	local randFn = LuckMath.makeSeededRng(cfg.seed or 42)
	local maxRounds = cfg.maxRounds or 30

	local allies = alliesInput
	local enemies = enemiesInput
	local synergy = BattleSim.computeSynergies(allies)
	applyTeamBuffs(allies, enemies, synergy, cfg.squadBonuses)

	local events: { BattleEvent } = {}
	table.insert(events, {
		kind = "start",
		round = 0,
		note = "Battle Started",
	})

	local roundsPlayed = 0
	for roundNum = 1, maxRounds do
		if countAlive(allies) == 0 or countAlive(enemies) == 0 then
			break
		end
		roundsPlayed = roundNum

		local allUnits: { CombatUnit } = {}
		for _, aUnit in ipairs(allies) do
			if aUnit.alive then
				table.insert(allUnits, aUnit)
			end
		end
		for _, eUnit in ipairs(enemies) do
			if eUnit.alive then
				table.insert(allUnits, eUnit)
			end
		end

		for _, dUnit in ipairs(allUnits) do
			if dUnit.alive and dUnit.burnTurns > 0 then
				dUnit.burnTurns -= 1
				local bDmg, bKilled = applyDamage(dUnit, dUnit.burnDmg, false, roundNum, events)
				table.insert(events, {
					kind = "dot",
					round = roundNum,
					targetUid = dUnit.uid,
					damage = bDmg,
					killed = bKilled,
					targetHp = dUnit.hp,
					targetMaxHp = dUnit.maxHp,
					targetShield = dUnit.shield,
					note = "Burn",
				})
			end
			if dUnit.alive and dUnit.bleedTurns > 0 then
				dUnit.bleedTurns -= 1
				local blDmg, blKilled = applyDamage(dUnit, dUnit.bleedDmg, true, roundNum, events)
				table.insert(events, {
					kind = "dot",
					round = roundNum,
					targetUid = dUnit.uid,
					damage = blDmg,
					killed = blKilled,
					targetHp = dUnit.hp,
					targetMaxHp = dUnit.maxHp,
					targetShield = dUnit.shield,
					note = "Bleed",
				})
			end
		end

		table.sort(allUnits, function(a: CombatUnit, b: CombatUnit): boolean
			if roundNum == 1 then
				local aPrio = a.passiveFlags.firstRoundPriority == true
				local bPrio = b.passiveFlags.firstRoundPriority == true
				if aPrio ~= bPrio then
					return aPrio
				end
			end
			local aSpd = a.spd
			if a.hp < a.maxHp * 0.6 and (a.passiveFlags.lowHpSpdBonus or 0) > 0 then
				aSpd *= (1 + a.passiveFlags.lowHpSpdBonus)
			end
			local bSpd = b.spd
			if b.hp < b.maxHp * 0.6 and (b.passiveFlags.lowHpSpdBonus or 0) > 0 then
				bSpd *= (1 + b.passiveFlags.lowHpSpdBonus)
			end
			if aSpd ~= bSpd then
				return aSpd > bSpd
			end
			if a.team ~= b.team then
				return a.team == "ally"
			end
			return a.slot < b.slot
		end)

		for _, actor in ipairs(allUnits) do
			if countAlive(allies) == 0 or countAlive(enemies) == 0 then
				break
			end
			if not actor.alive then
				continue
			end

			if actor.stunned > 0 or actor.frozen > 0 then
				if actor.stunned > 0 then
					actor.stunned -= 1
				end
				if actor.frozen > 0 then
					actor.frozen -= 1
				end
				table.insert(events, {
					kind = "skip",
					round = roundNum,
					actorUid = actor.uid,
					note = "Stunned",
				})
				continue
			end

			local friendlyTeam = if actor.team == "ally" then allies else enemies
			local opposingTeam = if actor.team == "ally" then enemies else allies

			local useSkill = actor.cooldownRemaining <= 0 and actor.silenced <= 0
			if actor.silenced > 0 then
				actor.silenced -= 1
			end
			if useSkill then
				local cdDelta = if actor.trait then actor.trait.cooldownDelta else 0
				actor.cooldownRemaining = math.max(1, actor.skill.cooldown + cdDelta)
			else
				actor.cooldownRemaining = math.max(0, actor.cooldownRemaining - 1)
			end

			local turnRamp = (actor.passiveFlags.turnAtkRamp or 0) * (roundNum - 1)
			local effAtk = actor.atk * (1 + actor.atkBuff + turnRamp)

			if useSkill and actor.skill.targeting == "TeamHeal" then
				local livingAllies = getLiving(friendlyTeam)
				for _, mate in ipairs(livingAllies) do
					local healAmt = math.floor(mate.maxHp * (actor.skill.healPct or 0.15))
					mate.hp = math.min(mate.maxHp, mate.hp + healAmt)
					if actor.skill.atkBuffPct then
						mate.atkBuff = math.min(0.8, mate.atkBuff + actor.skill.atkBuffPct)
					end
					if actor.skill.cleanse then
						mate.burnTurns = 0
						mate.bleedTurns = 0
						mate.stunned = 0
						mate.frozen = 0
					end
					table.insert(events, {
						kind = "heal",
						round = roundNum,
						actorUid = actor.uid,
						targetUid = mate.uid,
						skillName = actor.skill.name,
						isSkill = true,
						heal = healAmt,
						targetHp = mate.hp,
						targetMaxHp = mate.maxHp,
					})
				end
				continue
			end

			local livingFoes = getLiving(opposingTeam)
			if #livingFoes == 0 then
				break
			end

			local targets: { CombatUnit } = {}
			if useSkill and actor.skill.targeting == "AoE" then
				targets = livingFoes
			else
				table.insert(targets, livingFoes[1])
			end

			local skillMult = if useSkill then actor.skill.dmgMult else 1.0
			if useSkill and actor.trait then
				skillMult *= actor.trait.skillDmgMult
			end
			local actionName = if useSkill then actor.skill.name else "Basic Strike"

			for _, targetUnit in ipairs(targets) do
				if not targetUnit.alive then
					continue
				end
				local dodge = (targetUnit.passiveFlags.dodgeChance or 0)
					+ (if targetUnit.trait then targetUnit.trait.dodgeChance else 0)
				if dodge > 0 and randFn() < dodge then
					table.insert(events, {
						kind = "dodge",
						round = roundNum,
						actorUid = actor.uid,
						targetUid = targetUnit.uid,
						skillName = actionName,
						isSkill = useSkill,
					})
					continue
				end

				local dmg = effAtk * skillMult
				local execBonus = (actor.passiveFlags.executeBonus or 0)
					+ (if actor.trait then actor.trait.executeBonus else 0)
				if execBonus > 0 and targetUnit.hp < targetUnit.maxHp * 0.45 then
					dmg *= (1 + execBonus)
				end

				local critChance = 0.10
					+ (actor.passiveFlags.critBonus or 0)
					+ (if actor.trait then actor.trait.critBonus else 0)
					+ (if actor.team == "ally" then (synergy.critBonus + extraCrit) else 0)
				local isCrit = randFn() < critChance
				if isCrit then
					dmg *= (1.5 + (actor.passiveFlags.critDmgBonus or 0))
				end

				local dmgRed = (targetUnit.passiveFlags.damageReduction or 0)
					+ (if targetUnit.trait then targetUnit.trait.damageReduction else 0)
					+ (if targetUnit.team == "ally" then (synergy.defBonus + extraDef) else 0)
				local defIgnore = if actor.trait then actor.trait.defIgnore else 0
				if useSkill and actor.skill.pierce then
					defIgnore = math.max(defIgnore, 0.5)
				end
				local effRed = math.max(0, dmgRed * (1 - defIgnore))
				dmg *= (1 - math.min(0.75, effRed)) * (1 + targetUnit.defShred)

				local ignoreShield = useSkill and actor.skill.ignoreShield == true
				local dealt, killed = applyDamage(targetUnit, dmg, ignoreShield, roundNum, events)
				actor.damageDealt += dealt

				table.insert(events, {
					kind = "hit",
					round = roundNum,
					actorUid = actor.uid,
					targetUid = targetUnit.uid,
					skillName = actionName,
					isSkill = useSkill,
					damage = dealt,
					isCrit = isCrit,
					killed = killed,
					targetHp = targetUnit.hp,
					targetMaxHp = targetUnit.maxHp,
					targetShield = targetUnit.shield,
				})

				local lifesteal = (actor.passiveFlags.lifesteal or 0)
					+ (if actor.trait then actor.trait.lifesteal else 0)
				if lifesteal > 0 and actor.alive then
					local lHeal = math.floor(dealt * lifesteal)
					actor.hp = math.min(actor.maxHp, actor.hp + lHeal)
				end

				local reflectPct = targetUnit.passiveFlags.reflectPct or 0
				if reflectPct > 0 and actor.alive then
					applyDamage(actor, dealt * reflectPct, false, roundNum, events)
				end

				if useSkill and targetUnit.alive then
					if actor.skill.stunChance and randFn() < actor.skill.stunChance then
						targetUnit.stunned = math.max(targetUnit.stunned, 1)
					end
					if actor.skill.freezeChance and randFn() < actor.skill.freezeChance then
						targetUnit.frozen = math.max(targetUnit.frozen, 1)
					end
					if actor.skill.silenceTurns then
						targetUnit.silenced = math.max(targetUnit.silenced, actor.skill.silenceTurns)
					end
					if actor.skill.burnPct then
						targetUnit.burnTurns = 2
						targetUnit.burnDmg = math.max(1, math.floor(targetUnit.maxHp * actor.skill.burnPct))
					end
					if actor.skill.bleedPct then
						targetUnit.bleedTurns = 2
						targetUnit.bleedDmg = math.max(1, math.floor(targetUnit.maxHp * actor.skill.bleedPct))
					end
					if actor.skill.defShredPct then
						targetUnit.defShred = math.min(0.6, targetUnit.defShred + actor.skill.defShredPct)
					end
				end
			end

			if useSkill and actor.alive and actor.skill.shieldPct then
				actor.shield += math.floor(actor.maxHp * actor.skill.shieldPct)
			end
			if useSkill and actor.alive and actor.skill.healPct then
				local selfHeal = math.floor(actor.maxHp * actor.skill.healPct)
				actor.hp = math.min(actor.maxHp, actor.hp + selfHeal)
			end
		end
	end

	local won = countAlive(enemies) == 0
	table.insert(events, {
		kind = "end",
		round = roundsPlayed,
		note = if won then "Victory" else "Defeat",
	})

	local alliesSummary: { UnitSummary } = {}
	local mvpName = "None"
	local bestDmg = -1
	for _, aUnit in ipairs(allies) do
		if aUnit.damageDealt > bestDmg then
			bestDmg = aUnit.damageDealt
			mvpName = aUnit.name
		end
		table.insert(alliesSummary, {
			uid = aUnit.uid,
			name = aUnit.name,
			rarity = aUnit.rarity,
			hp = aUnit.hp,
			maxHp = aUnit.maxHp,
			alive = aUnit.alive,
			damageDealt = aUnit.damageDealt,
		})
	end

	local enemiesSummary: { UnitSummary } = {}
	for _, eUnit in ipairs(enemies) do
		table.insert(enemiesSummary, {
			uid = eUnit.uid,
			name = eUnit.name,
			rarity = eUnit.rarity,
			hp = eUnit.hp,
			maxHp = eUnit.maxHp,
			alive = eUnit.alive,
			damageDealt = eUnit.damageDealt,
		})
	end

	return {
		won = won,
		rounds = roundsPlayed,
		events = events,
		alliesSummary = alliesSummary,
		enemiesSummary = enemiesSummary,
		mvpName = mvpName,
	}
end

return BattleSim
