--!strict
local Rarities = require(script.Parent.Rarities)

export type RoleId = "Vanguard" | "Striker" | "Tactician" | "Controller" | "Support"

export type SkillDef = {
	name: string,
	description: string,
	cooldown: number,
	dmgMult: number,
	targeting: "Single" | "AoE" | "TeamHeal",
	shieldPct: number?,
	healPct: number?,
	stunChance: number?,
	silenceTurns: number?,
	burnPct: number?,
	bleedPct: number?,
	freezeChance: number?,
	pierce: boolean?,
	ignoreShield: boolean?,
	defShredPct: number?,
	atkBuffPct: number?,
	cleanse: boolean?,
	drainEnergy: number?,
}

export type CardDef = {
	id: string,
	displayName: string,
	worldId: number,
	showName: string,
	rarity: Rarities.RarityId,
	role: RoleId,
	bannerPool: boolean,
	exclusiveSource: string?,
	baseHP: number,
	baseATK: number,
	baseSPD: number,
	skill: SkillDef,
	passiveText: string,
	passiveFlags: {
		critBonus: number?,
		dodgeChance: number?,
		lifesteal: number?,
		reflectPct: number?,
		damageReduction: number?,
		enemySpdDebuff: number?,
		lowHpSpdBonus: number?,
		leafAtkSynergy: number?,
		turnAtkRamp: number?,
		critDmgBonus: number?,
		firstRoundPriority: boolean?,
		executeBonus: number?,
		counterHit: boolean?,
		resurrectPct: number?,
	},
	artKey: string,
}

local Cards = {}

local function makeCard(
	id: string,
	displayName: string,
	worldId: number,
	showName: string,
	rarity: Rarities.RarityId,
	role: RoleId,
	bannerPool: boolean,
	exclusiveSource: string?,
	skill: SkillDef,
	passiveText: string,
	passiveFlags: { [string]: any }?,
	hpScale: number?,
	atkScale: number?,
	spdScale: number?
): CardDef
	local r = Rarities.get(rarity)
	return {
		id = id,
		displayName = displayName,
		worldId = worldId,
		showName = showName,
		rarity = rarity,
		role = role,
		bannerPool = bannerPool,
		exclusiveSource = exclusiveSource,
		baseHP = math.floor(r.baseHP * (hpScale or 1.0)),
		baseATK = math.floor(r.baseATK * (atkScale or 1.0)),
		baseSPD = math.floor(r.baseSPD * (spdScale or 1.0)),
		skill = skill,
		passiveText = passiveText,
		passiveFlags = (passiveFlags or {}) :: any,
		artKey = "cards/" .. id,
	}
end

Cards.List = {
	-- World 1: Hidden Leaf Village (Full 9-tier banner)
	makeCard("w1_academy_genin", "Academy Genin", 1, "Leaf Shinobi", "Common", "Striker", true, nil, {
		name = "Rasengan Spark",
		description = "135% ATK single-target strike.",
		cooldown = 2,
		dmgMult = 1.35,
		targeting = "Single",
	}, "+10% ATK when paired with Leaf allies.", { leafAtkSynergy = 0.10 }, 0.95, 1.08, 1.02),
	makeCard("w1_tactical_chunin", "Tactical Chunin", 1, "Leaf Shinobi", "Uncommon", "Controller", true, nil, {
		name = "Shadow Possession",
		description = "110% ATK + 35% Stun (1 turn).",
		cooldown = 3,
		dmgMult = 1.10,
		targeting = "Single",
		stunChance = 0.35,
	}, "-10% enemy SPD passive.", { enemySpdDebuff = 0.10 }, 1.0, 0.95, 1.10),
	makeCard("w1_lotus_taijutsu", "Lotus Taijutsu", 1, "Leaf Shinobi", "Rare", "Striker", true, nil, {
		name = "Primary Lotus",
		description = "175% ATK single-target + Pierce.",
		cooldown = 2,
		dmgMult = 1.75,
		targeting = "Single",
		pierce = true,
	}, "+15% SPD below 60% HP.", { lowHpSpdBonus = 0.15 }, 0.95, 1.12, 1.08),
	makeCard("w1_crimson_copy_ninja", "Crimson Copy Ninja", 1, "Leaf Shinobi", "Epic", "Tactician", true, nil, {
		name = "Lightning Blade",
		description = "190% ATK + Silence (1 turn).",
		cooldown = 3,
		dmgMult = 1.90,
		targeting = "Single",
		silenceTurns = 1,
		pierce = true,
	}, "+10% Crit Chance & sharp tactical reflexes.", { critBonus = 0.10 }, 1.0, 1.06, 1.05),
	makeCard("w1_sage_of_toads", "Sage of Toads", 1, "Leaf Shinobi", "Legendary", "Support", true, nil, {
		name = "Sage Art Bath",
		description = "Heals team 22% Max HP + Cleanse + +20% ATK.",
		cooldown = 3,
		dmgMult = 0.0,
		targeting = "TeamHeal",
		healPct = 0.22,
		atkBuffPct = 0.20,
		cleanse = true,
	}, "Bolsters allies with sage vitality (+10% Damage Reduction).", { damageReduction = 0.10 }, 1.15, 0.92, 1.05),
	makeCard("w1_uchiha_prodigy", "Uchiha Prodigy", 1, "Leaf Shinobi", "Mythic", "Controller", true, nil, {
		name = "Tsukuyomi Gaze",
		description = "240% ATK AoE + 65% Stun + Burn.",
		cooldown = 3,
		dmgMult = 2.40,
		targeting = "AoE",
		stunChance = 0.65,
		burnPct = 0.10,
	}, "25% Dodge chance against incoming hits.", { dodgeChance = 0.25 }, 1.0, 1.05, 1.12),
	makeCard("w1_ghost_uchiha", "Ghost of the Uchiha", 1, "Leaf Shinobi", "Secret", "Vanguard", true, nil, {
		name = "Perfect Susanoo Cleave",
		description = "260% ATK AoE + 35% Shield.",
		cooldown = 3,
		dmgMult = 2.60,
		targeting = "AoE",
		shieldPct = 0.35,
	}, "Reflects 20% incoming damage.", { reflectPct = 0.20, damageReduction = 0.15 }, 1.25, 1.0, 0.96),
	makeCard(
		"w1_seventh_hokage",
		"Seventh Hokage",
		1,
		"Leaf Shinobi",
		"Universal",
		"Striker",
		true,
		nil,
		{
			name = "Baryon Rasenshuriken",
			description = "340% ATK + Burn + Pierce + drains 15% energy.",
			cooldown = 3,
			dmgMult = 3.40,
			targeting = "Single",
			burnPct = 0.12,
			pierce = true,
			drainEnergy = 15,
		},
		"Relentless chakra surge (+15% Crit & +10% Lifesteal).",
		{ critBonus = 0.15, lifesteal = 0.10 },
		1.05,
		1.18,
		1.08
	),
	makeCard("w1_rinnegan_shadow", "Rinnegan Shadow", 1, "Leaf Shinobi", "Nexus", "Tactician", true, nil, {
		name = "Indra Arrow",
		description = "420% ATK AoE + Silence + 40% Def Shred.",
		cooldown = 3,
		dmgMult = 4.20,
		targeting = "AoE",
		silenceTurns = 1,
		defShredPct = 0.40,
	}, "+15% ATK per turn elapsed.", { turnAtkRamp = 0.15, critBonus = 0.15 }, 1.05, 1.15, 1.10),

	-- World 1 Boss Exclusive Drop
	makeCard(
		"boss_ten_tails_vessel",
		"Ten-Tails Vessel",
		1,
		"Leaf Shinobi",
		"Legendary",
		"Vanguard",
		false,
		"World 1 Boss",
		{
			name = "Truth-Seeking Barrier",
			description = "210% ATK AoE + 30% Max HP Shield.",
			cooldown = 3,
			dmgMult = 2.10,
			targeting = "AoE",
			shieldPct = 0.30,
		},
		"Truth-Seeking Orb shield (-25% incoming damage).",
		{ damageReduction = 0.25 },
		1.22,
		0.98,
		0.96
	),

	-- World 2 Sample Roster (West City & Namek)
	makeCard("w2_desert_bandit", "Desert Bandit", 2, "Z-Warriors", "Common", "Striker", true, nil, {
		name = "Wolf Fang Fist",
		description = "140% ATK rapid combo strike.",
		cooldown = 2,
		dmgMult = 1.40,
		targeting = "Single",
	}, "+8% Crit Chance passive.", { critBonus = 0.08 }),
	makeCard("w2_namekian_guardian", "Namekian Guardian", 2, "Z-Warriors", "Rare", "Vanguard", true, nil, {
		name = "Special Beam Cannon",
		description = "165% ATK Pierce + 20% Shield.",
		cooldown = 3,
		dmgMult = 1.65,
		targeting = "Single",
		pierce = true,
		shieldPct = 0.20,
	}, "Namekian endurance (+12% Damage Reduction).", { damageReduction = 0.12 }),
	makeCard("w2_saiyan_prince", "Saiyan Prince", 2, "Z-Warriors", "Legendary", "Striker", true, nil, {
		name = "Final Flash",
		description = "255% ATK devastating energy blast.",
		cooldown = 3,
		dmgMult = 2.55,
		targeting = "Single",
		pierce = true,
	}, "Prideful surge (+15% Crit & +10% ATK/turn).", { critBonus = 0.15, turnAtkRamp = 0.10 }),
	makeCard(
		"w2_silver_autonomous_master",
		"Silver Autonomous Master",
		2,
		"Z-Warriors",
		"Nexus",
		"Controller",
		true,
		nil,
		{
			name = "Ultra Instinct Sign",
			description = "440% ATK strike with unmatched reflexes.",
			cooldown = 3,
			dmgMult = 4.40,
			targeting = "Single",
			pierce = true,
		},
		"55% Auto-Dodge & guaranteed counter-hit.",
		{ dodgeChance = 0.55, counterHit = true }
	),

	-- World 3 Sample Roster (Grand Line Seas)
	makeCard("w3_sniper_king", "Sniper King", 3, "Grand Fleet", "Common", "Controller", true, nil, {
		name = "Fire Bird Star",
		description = "125% ATK + Burn.",
		cooldown = 2,
		dmgMult = 1.25,
		targeting = "Single",
		burnPct = 0.08,
	}, "Keen long-range aim (+10% Crit Chance).", { critBonus = 0.10 }),
	makeCard("w3_pirate_hunter", "Pirate Hunter", 3, "Grand Fleet", "Legendary", "Striker", true, nil, {
		name = "King of Hell",
		description = "265% ATK + Bleed + Pierce.",
		cooldown = 3,
		dmgMult = 2.65,
		targeting = "Single",
		bleedPct = 0.10,
		pierce = true,
	}, "+50% Crit Damage passive.", { critDmgBonus = 0.50, critBonus = 0.12 }),
	makeCard("w3_warrior_of_liberation", "Warrior of Liberation", 3, "Grand Fleet", "Nexus", "Striker", true, nil, {
		name = "Bajrang Gun",
		description = "450% ATK AoE + Stun.",
		cooldown = 3,
		dmgMult = 4.50,
		targeting = "AoE",
		stunChance = 0.50,
	}, "1x Resurrect at 50% HP.", { resurrectPct = 0.50 }),

	-- World 4 Sample Roster (Tokyo Jujutsu High)
	makeCard(
		"w4_cursed_speech_student",
		"Cursed Speech Student",
		4,
		"Jujutsu High",
		"Common",
		"Controller",
		true,
		nil,
		{
			name = "Dont Move",
			description = "115% ATK + 30% Stun.",
			cooldown = 2,
			dmgMult = 1.15,
			targeting = "Single",
			stunChance = 0.30,
		},
		"Suppressing cursed aura.",
		{ enemySpdDebuff = 0.08 }
	),
	makeCard("w4_queen_of_curses", "Queen of Curses", 4, "Jujutsu High", "Legendary", "Support", true, nil, {
		name = "Rika Manifestation",
		description = "220% ATK AoE + Cleanse & 20% Heal.",
		cooldown = 3,
		dmgMult = 2.20,
		targeting = "AoE",
		healPct = 0.20,
		cleanse = true,
	}, "Boundless cursed energy pool.", { damageReduction = 0.10 }),
	makeCard("w4_honored_one", "Honored One", 4, "Jujutsu High", "Nexus", "Controller", true, nil, {
		name = "Hollow Purple",
		description = "460% ATK True DMG + Freeze counter.",
		cooldown = 3,
		dmgMult = 4.60,
		targeting = "Single",
		pierce = true,
		ignoreShield = true,
		freezeChance = 0.55,
	}, "Limitless barrier (40% Damage Reduction).", { damageReduction = 0.40, counterHit = true }),

	-- World 5 Sample Roster (Soul Society)
	makeCard(
		"w5_eleventh_lieutenant",
		"Eleventh Squad Lieutenant",
		5,
		"Soul Society",
		"Common",
		"Vanguard",
		true,
		nil,
		{
			name = "Zabimaru Whip",
			description = "135% ATK + Shield.",
			cooldown = 2,
			dmgMult = 1.35,
			targeting = "Single",
			shieldPct = 0.15,
		},
		"Frontline Soul Reaper tenacity.",
		{ damageReduction = 0.08 }
	),
	makeCard("w5_kenpachi", "Battle-Crazed Kenpachi", 5, "Soul Society", "Legendary", "Vanguard", true, nil, {
		name = "Nozarashi Cleave",
		description = "270% ATK + 20% Lifesteal.",
		cooldown = 3,
		dmgMult = 2.70,
		targeting = "Single",
	}, "20% Lifesteal & +15% ATK below 50% HP.", { lifesteal = 0.20 }),
	makeCard("w5_true_bankai_substitute", "True Bankai Substitute", 5, "Soul Society", "Nexus", "Striker", true, nil, {
		name = "True Getsuga Tensho",
		description = "470% ATK + Execute + Pierce.",
		cooldown = 3,
		dmgMult = 4.70,
		targeting = "Single",
		pierce = true,
	}, "+50% Execute damage vs enemies below 45% HP.", { executeBonus = 0.50, critBonus = 0.15 }),
} :: { CardDef }

Cards.ById = {} :: { [string]: CardDef }
for _, card in ipairs(Cards.List) do
	Cards.ById[card.id] = card
end

function Cards.get(id: string): CardDef?
	return Cards.ById[id]
end

function Cards.getPoolForWorld(worldId: number): { CardDef }
	local exact: { CardDef } = {}
	for _, card in ipairs(Cards.List) do
		if card.bannerPool and card.worldId == worldId then
			table.insert(exact, card)
		end
	end
	return exact
end

return Cards
