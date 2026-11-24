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
	-- World 1: Leaf Shinobi (9 banner + 1 boss)
	makeCard("w1_academy_genin", "Academy Genin", 1, "Leaf Shinobi", "Common", "Striker", true, nil, { name = "Rasengan Spark", description = "135% ATK single-target strike.", cooldown = 2, dmgMult = 1.35, targeting = "Single" }, "+10% ATK when paired with Leaf allies.", { leafAtkSynergy = 0.10 }, 0.95, 1.08, 1.02),
	makeCard("w1_tactical_chunin", "Tactical Chunin", 1, "Leaf Shinobi", "Uncommon", "Controller", true, nil, { name = "Shadow Possession", description = "110% ATK + 35% Stun (1 turn).", cooldown = 3, dmgMult = 1.10, targeting = "Single", stunChance = 0.35 }, "-10% enemy SPD passive.", { enemySpdDebuff = 0.10 }, 1.0, 0.95, 1.10),
	makeCard("w1_lotus_taijutsu", "Lotus Taijutsu", 1, "Leaf Shinobi", "Rare", "Striker", true, nil, { name = "Primary Lotus", description = "175% ATK single-target + Pierce.", cooldown = 2, dmgMult = 1.75, targeting = "Single", pierce = true }, "+15% SPD below 60% HP.", { lowHpSpdBonus = 0.15 }, 0.95, 1.12, 1.08),
	makeCard("w1_crimson_copy_ninja", "Crimson Copy Ninja", 1, "Leaf Shinobi", "Epic", "Tactician", true, nil, { name = "Lightning Blade", description = "190% ATK + Silence (1 turn).", cooldown = 3, dmgMult = 1.90, targeting = "Single", silenceTurns = 1, pierce = true }, "+10% Crit Chance & sharp tactical reflexes.", { critBonus = 0.10 }, 1.0, 1.06, 1.05),
	makeCard("w1_sage_of_toads", "Sage of Toads", 1, "Leaf Shinobi", "Legendary", "Support", true, nil, { name = "Sage Art Bath", description = "Heals team 22% Max HP + Cleanse + +20% ATK.", cooldown = 3, dmgMult = 0.0, targeting = "TeamHeal", healPct = 0.22, atkBuffPct = 0.20, cleanse = true }, "Bolsters allies with sage vitality (+10% Damage Reduction).", { damageReduction = 0.10 }, 1.15, 0.92, 1.05),
	makeCard("w1_uchiha_prodigy", "Uchiha Prodigy", 1, "Leaf Shinobi", "Mythic", "Controller", true, nil, { name = "Tsukuyomi Gaze", description = "240% ATK AoE + 65% Stun + Burn.", cooldown = 3, dmgMult = 2.40, targeting = "AoE", stunChance = 0.65, burnPct = 0.10 }, "25% Dodge chance against incoming hits.", { dodgeChance = 0.25 }, 1.0, 1.05, 1.12),
	makeCard("w1_ghost_uchiha", "Ghost of the Uchiha", 1, "Leaf Shinobi", "Secret", "Vanguard", true, nil, { name = "Perfect Susanoo Cleave", description = "260% ATK AoE + 35% Shield.", cooldown = 3, dmgMult = 2.60, targeting = "AoE", shieldPct = 0.35 }, "Reflects 20% incoming damage.", { reflectPct = 0.20, damageReduction = 0.15 }, 1.25, 1.0, 0.96),
	makeCard("w1_seventh_hokage", "Seventh Hokage", 1, "Leaf Shinobi", "Universal", "Striker", true, nil, { name = "Baryon Rasenshuriken", description = "340% ATK + Burn + Pierce + drains 15% energy.", cooldown = 3, dmgMult = 3.40, targeting = "Single", burnPct = 0.12, pierce = true, drainEnergy = 15 }, "Relentless chakra surge (+15% Crit & +10% Lifesteal).", { critBonus = 0.15, lifesteal = 0.10 }, 1.05, 1.18, 1.08),
	makeCard("w1_rinnegan_shadow", "Rinnegan Shadow", 1, "Leaf Shinobi", "Nexus", "Tactician", true, nil, { name = "Indra Arrow", description = "420% ATK AoE + Silence + 40% Def Shred.", cooldown = 3, dmgMult = 4.20, targeting = "AoE", silenceTurns = 1, defShredPct = 0.40 }, "+15% ATK per turn elapsed.", { turnAtkRamp = 0.15, critBonus = 0.15 }, 1.05, 1.15, 1.10),
	makeCard("boss_ten_tails_vessel", "Ten-Tails Vessel", 1, "Leaf Shinobi", "Legendary", "Vanguard", false, "World 1 Boss", { name = "Truth-Seeking Barrier", description = "210% ATK AoE + 30% Max HP Shield.", cooldown = 3, dmgMult = 2.10, targeting = "AoE", shieldPct = 0.30 }, "Truth-Seeking Orb shield (-25% incoming damage).", { damageReduction = 0.25 }, 1.22, 0.98, 0.96),

	-- World 2: Z-Fighters (9 banner)
	makeCard("w2_desert_bandit", "Desert Bandit", 2, "Z-Fighters", "Common", "Striker", true, nil, { name = "Wolf Fang Fist", description = "140% ATK rapid combo strike.", cooldown = 2, dmgMult = 1.40, targeting = "Single" }, "+8% Crit Chance passive.", { critBonus = 0.08 }),
	makeCard("w2_turtle_hermit_disciple", "Turtle Hermit Disciple", 2, "Z-Fighters", "Uncommon", "Support", true, nil, { name = "Destructo Disc", description = "145% ATK Pierce + 10% Heal.", cooldown = 2, dmgMult = 1.45, targeting = "Single", pierce = true, healPct = 0.10 }, "+8% Crit Chance.", { critBonus = 0.08 }),
	makeCard("w2_namekian_guardian", "Namekian Guardian", 2, "Z-Fighters", "Rare", "Vanguard", true, nil, { name = "Special Beam Cannon", description = "165% ATK Pierce + 20% Shield.", cooldown = 3, dmgMult = 1.65, targeting = "Single", pierce = true, shieldPct = 0.20 }, "Namekian endurance (+12% Damage Reduction).", { damageReduction = 0.12 }),
	makeCard("w2_mystic_prodigy", "Mystic Prodigy", 2, "Z-Fighters", "Epic", "Tactician", true, nil, { name = "Masenko Burst", description = "200% ATK AoE + 15% ATK Buff.", cooldown = 3, dmgMult = 2.00, targeting = "AoE", atkBuffPct = 0.15 }, "+12% Crit Damage.", { critDmgBonus = 0.12 }),
	makeCard("w2_saiyan_prince", "Saiyan Prince", 2, "Z-Fighters", "Legendary", "Striker", true, nil, { name = "Final Flash", description = "255% ATK devastating energy blast.", cooldown = 3, dmgMult = 2.55, targeting = "Single", pierce = true }, "Prideful surge (+15% Crit & +10% ATK/turn).", { critBonus = 0.15, turnAtkRamp = 0.10 }),
	makeCard("w2_future_savior", "Future Savior", 2, "Z-Fighters", "Mythic", "Striker", true, nil, { name = "Burning Slash", description = "280% ATK + Burn + Pierce.", cooldown = 3, dmgMult = 2.80, targeting = "Single", burnPct = 0.12, pierce = true }, "First-round priority strike.", { firstRoundPriority = true, critBonus = 0.12 }),
	makeCard("w2_legendary_berserker", "Legendary Berserker", 2, "Z-Fighters", "Secret", "Vanguard", true, nil, { name = "Omega Blaster", description = "290% ATK AoE + 25% Shield.", cooldown = 3, dmgMult = 2.90, targeting = "AoE", shieldPct = 0.25 }, "+15% ATK/turn ramp & 15% DR.", { turnAtkRamp = 0.15, damageReduction = 0.15 }),
	makeCard("w2_fused_god_warrior", "Fused God Warrior", 2, "Z-Fighters", "Universal", "Tactician", true, nil, { name = "Stardust Breaker", description = "360% ATK Pierce + 35% Def Shred.", cooldown = 3, dmgMult = 3.60, targeting = "Single", pierce = true, defShredPct = 0.35 }, "+20% Crit & +30% Crit DMG.", { critBonus = 0.20, critDmgBonus = 0.30 }),
	makeCard("w2_silver_autonomous_master", "Silver Autonomous Master", 2, "Z-Fighters", "Nexus", "Controller", true, nil, { name = "Ultra Instinct Sign", description = "440% ATK strike with unmatched reflexes.", cooldown = 3, dmgMult = 4.40, targeting = "Single", pierce = true }, "55% Auto-Dodge & guaranteed counter-hit.", { dodgeChance = 0.55, counterHit = true }),

	-- World 3: Grand Line (9 banner)
	makeCard("w3_sniper_king", "Sniper King", 3, "Grand Line", "Common", "Controller", true, nil, { name = "Fire Bird Star", description = "125% ATK + Burn.", cooldown = 2, dmgMult = 1.25, targeting = "Single", burnPct = 0.08 }, "Keen long-range aim (+10% Crit Chance).", { critBonus = 0.10 }),
	makeCard("w3_cotton_candy_doctor", "Cotton Candy Doctor", 3, "Grand Line", "Uncommon", "Support", true, nil, { name = "Rumble Guard", description = "18% Team Heal + Cleanse.", cooldown = 3, dmgMult = 0.0, targeting = "TeamHeal", healPct = 0.18, cleanse = true }, "+8% Damage Reduction.", { damageReduction = 0.08 }),
	makeCard("w3_black_leg_chef", "Black Leg Chef", 3, "Grand Line", "Rare", "Striker", true, nil, { name = "Diable Jambe", description = "180% ATK + Burn.", cooldown = 2, dmgMult = 1.80, targeting = "Single", burnPct = 0.10 }, "+15% SPD below 60% HP.", { lowHpSpdBonus = 0.15 }),
	makeCard("w3_surgeon_of_death", "Surgeon of Death", 3, "Grand Line", "Epic", "Tactician", true, nil, { name = "Gamma Knife", description = "205% ATK Ignore Shield + 25% Def Shred.", cooldown = 3, dmgMult = 2.05, targeting = "Single", ignoreShield = true, defShredPct = 0.25 }, "+10% Lifesteal.", { lifesteal = 0.10 }),
	makeCard("w3_pirate_hunter", "Pirate Hunter", 3, "Grand Line", "Legendary", "Striker", true, nil, { name = "King of Hell", description = "265% ATK + Bleed + Pierce.", cooldown = 3, dmgMult = 2.65, targeting = "Single", bleedPct = 0.10, pierce = true }, "+50% Crit Damage passive.", { critDmgBonus = 0.50, critBonus = 0.12 }),
	makeCard("w3_flame_emperor", "Flame Emperor", 3, "Grand Line", "Mythic", "Tactician", true, nil, { name = "Entei Inferno", description = "260% ATK AoE + Burn.", cooldown = 3, dmgMult = 2.60, targeting = "AoE", burnPct = 0.15 }, "20% Dodge & +12% Crit.", { dodgeChance = 0.20, critBonus = 0.12 }),
	makeCard("w3_red_haired_emperor", "Red-Haired Emperor", 3, "Grand Line", "Secret", "Controller", true, nil, { name = "Divine Departure", description = "310% ATK Pierce + 55% Stun.", cooldown = 3, dmgMult = 3.10, targeting = "Single", pierce = true, stunChance = 0.55 }, "Conqueror pressure (-15% enemy SPD).", { enemySpdDebuff = 0.15, firstRoundPriority = true }),
	makeCard("w3_darkness_quake", "Darkness Quake", 3, "Grand Line", "Universal", "Vanguard", true, nil, { name = "Black Vortex Quake", description = "320% ATK AoE + Silence + 30% Shield.", cooldown = 3, dmgMult = 3.20, targeting = "AoE", silenceTurns = 1, shieldPct = 0.30 }, "20% DR & 15% Reflect.", { damageReduction = 0.20, reflectPct = 0.15 }),
	makeCard("w3_warrior_of_liberation", "Warrior of Liberation", 3, "Grand Line", "Nexus", "Striker", true, nil, { name = "Bajrang Gun", description = "450% ATK AoE + Stun.", cooldown = 3, dmgMult = 4.50, targeting = "AoE", stunChance = 0.50 }, "1x Resurrect at 50% HP.", { resurrectPct = 0.50 }),

	-- World 4: Cursed Sorcerers (9 banner)
	makeCard("w4_cursed_speech_student", "Cursed Speech Student", 4, "Cursed Sorcerers", "Common", "Controller", true, nil, { name = "Dont Move", description = "115% ATK + 30% Stun.", cooldown = 2, dmgMult = 1.15, targeting = "Single", stunChance = 0.30 }, "Suppressing cursed aura.", { enemySpdDebuff = 0.08 }),
	makeCard("w4_ten_shadows_initiate", "Ten Shadows Initiate", 4, "Cursed Sorcerers", "Uncommon", "Tactician", true, nil, { name = "Divine Dogs", description = "145% ATK + Bleed.", cooldown = 2, dmgMult = 1.45, targeting = "Single", bleedPct = 0.08 }, "+8% Crit Chance.", { critBonus = 0.08 }),
	makeCard("w4_black_flash_vessel", "Black Flash Vessel", 4, "Cursed Sorcerers", "Rare", "Striker", true, nil, { name = "Black Flash", description = "185% ATK + Pierce.", cooldown = 2, dmgMult = 1.85, targeting = "Single", pierce = true }, "+25% Crit Damage.", { critDmgBonus = 0.25, critBonus = 0.10 }),
	makeCard("w4_thunder_god_sorcerer", "Thunder God Sorcerer", 4, "Cursed Sorcerers", "Epic", "Striker", true, nil, { name = "Mythic Amber Beast", description = "215% ATK + 35% Stun.", cooldown = 3, dmgMult = 2.15, targeting = "Single", stunChance = 0.35 }, "First-round priority.", { firstRoundPriority = true }),
	makeCard("w4_queen_of_curses", "Queen of Curses", 4, "Cursed Sorcerers", "Legendary", "Support", true, nil, { name = "Rika Manifestation", description = "220% ATK AoE + Cleanse & 20% Heal.", cooldown = 3, dmgMult = 2.20, targeting = "AoE", healPct = 0.20, cleanse = true }, "Boundless cursed energy pool.", { damageReduction = 0.10 }),
	makeCard("w4_sorcerer_killer", "Sorcerer Killer", 4, "Cursed Sorcerers", "Mythic", "Striker", true, nil, { name = "Inverted Spear", description = "295% ATK Ignore Shield + Pierce.", cooldown = 3, dmgMult = 2.95, targeting = "Single", ignoreShield = true, pierce = true }, "+40% Execute below 40% HP.", { executeBonus = 0.40, critBonus = 0.15 }),
	makeCard("w4_brain_of_curses", "Brain of Curses", 4, "Cursed Sorcerers", "Secret", "Controller", true, nil, { name = "Womb Profusion", description = "280% ATK AoE + Silence + 30% Def Shred.", cooldown = 3, dmgMult = 2.80, targeting = "AoE", silenceTurns = 1, defShredPct = 0.30 }, "-15% Enemy SPD & 15% DR.", { enemySpdDebuff = 0.15, damageReduction = 0.15 }),
	makeCard("w4_king_of_curses", "King of Curses", 4, "Cursed Sorcerers", "Universal", "Striker", true, nil, { name = "Malevolent Shrine", description = "380% ATK AoE + Bleed + Pierce.", cooldown = 3, dmgMult = 3.80, targeting = "AoE", bleedPct = 0.15, pierce = true }, "15% Lifesteal & +20% Crit.", { lifesteal = 0.15, critBonus = 0.20 }),
	makeCard("w4_honored_one", "Honored One", 4, "Cursed Sorcerers", "Nexus", "Controller", true, nil, { name = "Hollow Purple", description = "460% ATK True DMG + Freeze counter.", cooldown = 3, dmgMult = 4.60, targeting = "Single", pierce = true, ignoreShield = true, freezeChance = 0.55 }, "Limitless barrier (40% Damage Reduction).", { damageReduction = 0.40, counterHit = true }),

	-- World 5: Soul Society (9 banner)
	makeCard("w5_eleventh_lieutenant", "Eleventh Squad Lieutenant", 5, "Soul Society", "Common", "Vanguard", true, nil, { name = "Zabimaru Whip", description = "135% ATK + Shield.", cooldown = 2, dmgMult = 1.35, targeting = "Single", shieldPct = 0.15 }, "Frontline Soul Reaper tenacity.", { damageReduction = 0.08 }),
	makeCard("w5_ice_prodigy_captain", "Ice Prodigy Captain", 5, "Soul Society", "Uncommon", "Controller", true, nil, { name = "Hyoten Hyakkaso", description = "130% ATK + 40% Freeze.", cooldown = 3, dmgMult = 1.30, targeting = "Single", freezeChance = 0.40 }, "-10% enemy SPD.", { enemySpdDebuff = 0.10 }),
	makeCard("w5_thunder_flash_goddess", "Thunder Flash Goddess", 5, "Soul Society", "Rare", "Striker", true, nil, { name = "Shunko Raijin", description = "180% ATK + 25% Stun.", cooldown = 2, dmgMult = 1.80, targeting = "Single", stunChance = 0.25 }, "20% Dodge & first-round priority.", { dodgeChance = 0.20, firstRoundPriority = true }),
	makeCard("w5_noble_senbonzakura", "Noble Senbonzakura", 5, "Soul Society", "Epic", "Tactician", true, nil, { name = "Senkei Scatter", description = "210% ATK AoE + Pierce.", cooldown = 3, dmgMult = 2.10, targeting = "AoE", pierce = true }, "+15% Crit Chance.", { critBonus = 0.15 }),
	makeCard("w5_kenpachi", "Battle-Crazed Kenpachi", 5, "Soul Society", "Legendary", "Vanguard", true, nil, { name = "Nozarashi Cleave", description = "270% ATK + 20% Lifesteal.", cooldown = 3, dmgMult = 2.70, targeting = "Single" }, "20% Lifesteal & +15% ATK below 50% HP.", { lifesteal = 0.20 }),
	makeCard("w5_clever_scientist_captain", "Clever Scientist Captain", 5, "Soul Society", "Mythic", "Support", true, nil, { name = "Konjiki Ashisogi", description = "230% ATK AoE + Cleanse + +25% ATK Buff.", cooldown = 3, dmgMult = 2.30, targeting = "AoE", atkBuffPct = 0.25, cleanse = true }, "15% DR & -12% enemy SPD.", { damageReduction = 0.15, enemySpdDebuff = 0.12 }),
	makeCard("w5_head_captain_inferno", "Head Captain Inferno", 5, "Soul Society", "Secret", "Striker", true, nil, { name = "Zanka no Tachi", description = "330% ATK AoE + Burn + Pierce.", cooldown = 3, dmgMult = 3.30, targeting = "AoE", burnPct = 0.18, pierce = true }, "+20% ATK/turn ramp.", { turnAtkRamp = 0.20, critBonus = 0.15 }),
	makeCard("w5_transcendent_strategist", "Transcendent Strategist", 5, "Soul Society", "Universal", "Tactician", true, nil, { name = "Kyoka Suigetsu", description = "370% ATK AoE + Silence + 40% Def Shred.", cooldown = 3, dmgMult = 3.70, targeting = "AoE", silenceTurns = 1, defShredPct = 0.40 }, "35% Dodge & Counter-hit.", { dodgeChance = 0.35, counterHit = true }),
	makeCard("w5_true_bankai_substitute", "True Bankai Substitute", 5, "Soul Society", "Nexus", "Striker", true, nil, { name = "True Getsuga Tensho", description = "470% ATK + Execute + Pierce.", cooldown = 3, dmgMult = 4.70, targeting = "Single", pierce = true }, "+50% Execute damage vs enemies below 45% HP.", { executeBonus = 0.50, critBonus = 0.15 }),

	-- Remaining 8 Exclusives (bannerPool = false)
	makeCard("boss_golden_tyrant", "Golden Tyrant", 2, "Z-Fighters", "Mythic", "Striker", false, "W2 Boss", { name = "Emperor Death Beam", description = "275% ATK Pierce + 25% Def Shred.", cooldown = 3, dmgMult = 2.75, targeting = "Single", pierce = true, defShredPct = 0.25 }, "+20% Crit & +15% ATK/turn.", { critBonus = 0.20, turnAtkRamp = 0.15 }),
	makeCard("boss_strongest_creature", "Strongest Creature", 3, "Grand Line", "Mythic", "Vanguard", false, "W3 Boss", { name = "Thunder Bagua", description = "260% ATK AoE + 30% Shield + Burn.", cooldown = 3, dmgMult = 2.60, targeting = "AoE", shieldPct = 0.30, burnPct = 0.10 }, "25% DR & 15% Reflect.", { damageReduction = 0.25, reflectPct = 0.15 }),
	makeCard("boss_divine_general", "Divine General", 4, "Cursed Sorcerers", "Secret", "Vanguard", false, "W4 Boss", { name = "Eight-Handled Adaptation", description = "310% ATK AoE + Cleanse + 35% Shield.", cooldown = 3, dmgMult = 3.10, targeting = "AoE", cleanse = true, shieldPct = 0.35 }, "30% DR & Counter-hit.", { damageReduction = 0.30, counterHit = true }),
	makeCard("boss_almighty_king", "Almighty King", 5, "Soul Society", "Universal", "Controller", false, "W5 Boss", { name = "The Almighty", description = "385% ATK AoE + Silence + Drain 20 Energy.", cooldown = 3, dmgMult = 3.85, targeting = "AoE", silenceTurns = 1, drainEnergy = 20 }, "30% Dodge & 25% DR.", { dodgeChance = 0.30, damageReduction = 0.25 }),
	makeCard("excl_temporal_attendant", "Temporal Attendant", 2, "Z-Fighters", "Universal", "Support", false, "Tower F100", { name = "Temporal Rewind", description = "30% Team Heal + Cleanse + +30% ATK Buff.", cooldown = 3, dmgMult = 0.0, targeting = "TeamHeal", healPct = 0.30, cleanse = true, atkBuffPct = 0.30 }, "40% Dodge & 1x Resurrect at 60% HP.", { dodgeChance = 0.40, resurrectPct = 0.60 }),
	makeCard("excl_pirate_king", "Pirate King", 3, "Grand Line", "Universal", "Striker", false, "Raid 1 Skull Island", { name = "Kamusari", description = "390% ATK Pierce + Ignore Shield + 50% Stun.", cooldown = 3, dmgMult = 3.90, targeting = "Single", pierce = true, ignoreShield = true, stunChance = 0.50 }, "+25% Crit & +40% Crit DMG.", { critBonus = 0.25, critDmgBonus = 0.40 }),
	makeCard("excl_disaster_flame", "Disaster Flame", 4, "Cursed Sorcerers", "Mythic", "Tactician", false, "Raid 2 Shibuya Breach", { name = "Coffin of the Iron Mountain", description = "275% ATK AoE + 18% Burn.", cooldown = 3, dmgMult = 2.75, targeting = "AoE", burnPct = 0.18 }, "+15% Crit & +12% ATK/turn.", { critBonus = 0.15, turnAtkRamp = 0.12 }),
	makeCard("excl_hokage_flash_kcm", "Hokage Flash KCM", 1, "Leaf Shinobi", "Secret", "Striker", false, "Ranked Grandmaster+", { name = "Flying Raijin Jiku", description = "340% ATK Pierce + 40% Stun.", cooldown = 2, dmgMult = 3.40, targeting = "Single", pierce = true, stunChance = 0.40 }, "First-round priority & 30% Dodge.", { firstRoundPriority = true, dodgeChance = 0.30 }),
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

function Cards.getBannerPool(worldId: number): { CardDef }
	return Cards.getPoolForWorld(worldId)
end

return Cards
