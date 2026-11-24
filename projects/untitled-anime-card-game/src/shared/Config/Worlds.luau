--!strict
export type EnemySpec = {
	name: string,
	cardId: string,
	hp: number,
	atk: number,
	spd: number,
	isBoss: boolean,
	skillName: string,
	skillMult: number,
}

export type StageDef = {
	id: string,
	worldId: number,
	act: number,
	isBoss: boolean,
	name: string,
	recommendedPower: number,
	yenReward: number,
	gemReward: number,
	xpReward: number,
	bossTokenReward: number,
	firstClearGems: number,
	firstClearTickets: number,
	dropCardId: string?,
	dropChance: number?,
	enemies: { EnemySpec },
}

export type BondDef = {
	id: string,
	name: string,
	worldId: number?,
	requiredShow: string?,
	minCount: number?,
	requiredCardIds: { string }?,
	atkBonus: number,
	spdBonus: number,
	hpBonus: number,
	critDmgBonus: number,
	dodgeBonus: number,
	description: string,
}

export type WorldDef = {
	id: number,
	name: string,
	bannerName: string,
	showName: string,
	rollCostYen: number,
	unlockGems: number,
	bondName: string,
	bondDescription: string,
	stages: { StageDef },
}

local function mkEnemy(
	name: string,
	cardId: string,
	hp: number,
	atk: number,
	spd: number,
	isBoss: boolean,
	skillName: string,
	skillMult: number
): EnemySpec
	return {
		name = name,
		cardId = cardId,
		hp = hp,
		atk = atk,
		spd = spd,
		isBoss = isBoss,
		skillName = skillName,
		skillMult = skillMult,
	}
end

local function mkStage(
	id: string,
	worldId: number,
	act: number,
	isBoss: boolean,
	name: string,
	recommendedPower: number,
	yenReward: number,
	gemReward: number,
	xpReward: number,
	bossTokenReward: number,
	firstClearGems: number,
	firstClearTickets: number,
	enemies: { EnemySpec },
	dropCardId: string?,
	dropChance: number?
): StageDef
	return {
		id = id,
		worldId = worldId,
		act = act,
		isBoss = isBoss,
		name = name,
		recommendedPower = recommendedPower,
		yenReward = yenReward,
		gemReward = gemReward,
		xpReward = xpReward,
		bossTokenReward = bossTokenReward,
		firstClearGems = firstClearGems,
		firstClearTickets = firstClearTickets,
		dropCardId = dropCardId,
		dropChance = dropChance,
		enemies = enemies,
	}
end

local Worlds = {}

Worlds.Tower = {
	maxFloor = 100,
	milestoneCardId = "excl_temporal_attendant",
	milestoneFloor = 100,
	baseHp = 900,
	baseAtk = 95,
	baseSpd = 105,
	hpScalePerFloor = 1.075,
	atkScalePerFloor = 1.055,
	spdPerFloor = 0.6,
}

Worlds.Raids = {
	{
		id = "raid_skull_island",
		name = "Skull Island Raid",
		minWorld = 3,
		recommendedPower = 45000,
		cardRewardId = "excl_pirate_king",
		dropChance = 0.04,
		raidTokenReward = 15,
	},
	{
		id = "raid_shibuya_breach",
		name = "Shibuya Breach Raid",
		minWorld = 4,
		recommendedPower = 140000,
		cardRewardId = "excl_disaster_flame",
		dropChance = 0.05,
		raidTokenReward = 25,
	},
}

Worlds.List = {
	{
		id = 1,
		name = "Hidden Leaf Village",
		bannerName = "Leaf Village Summons",
		showName = "Leaf Shinobi",
		rollCostYen = 100,
		unlockGems = 0,
		bondName = "Will of Fire",
		bondDescription = "2+ Leaf Shinobi: +10% ATK, +8% SPD",
		stages = {
			mkStage("w1_act1", 1, 1, false, "Act 1: Academy Courtyard", 220, 350, 45, 120, 0, 150, 3, {
				mkEnemy("Rogue Genin A", "w1_academy_genin", 320, 48, 92, false, "Kunai Flurry", 1.25),
				mkEnemy("Rogue Genin B", "w1_academy_genin", 340, 50, 95, false, "Spark Strike", 1.25),
			}),
			mkStage("w1_act2", 1, 2, false, "Act 2: Forest of Traps", 480, 650, 65, 180, 0, 200, 4, {
				mkEnemy("Shadow Tracker", "w1_tactical_chunin", 680, 92, 104, false, "Shadow Bind", 1.30),
				mkEnemy("Leaf Striker", "w1_academy_genin", 620, 105, 102, false, "Chakra Dash", 1.35),
			}),
			mkStage("w1_act3", 1, 3, false, "Act 3: Chunin Arena", 950, 1100, 90, 260, 0, 250, 5, {
				mkEnemy("Gate Taijutsu", "w1_lotus_taijutsu", 1250, 175, 114, false, "Leaf Hurricane", 1.45),
				mkEnemy("Chunin Strategist", "w1_tactical_chunin", 1100, 150, 110, false, "Trap Seal", 1.30),
			}),
			mkStage("w1_act4", 1, 4, false, "Act 4: Uchiha Shrine", 2100, 1850, 130, 380, 0, 320, 6, {
				mkEnemy("Copy Jonin", "w1_crimson_copy_ninja", 2400, 340, 124, false, "Lightning Cutter", 1.60),
				mkEnemy("Shrine Guard", "w1_lotus_taijutsu", 2000, 290, 118, false, "Gate Strike", 1.45),
			}),
			mkStage("w1_act5", 1, 5, false, "Act 5: Valley of the End", 3200, 2500, 170, 500, 0, 400, 7, {
				mkEnemy("Uchiha Phantom", "w1_uchiha_prodigy", 3800, 460, 128, false, "Amaterasu", 1.65),
				mkEnemy("Sage Sentinel", "w1_sage_of_toads", 3400, 380, 122, false, "Frog Kata", 1.50),
			}),
			mkStage("w1_boss", 1, 6, true, "Boss: Ten-Tails Jinchuriki", 4200, 3200, 220, 650, 2, 500, 10, {
				mkEnemy("Ten-Tails Vessel", "boss_ten_tails_vessel", 6800, 580, 132, true, "Truth-Seeking Burst", 1.75),
			}, "boss_ten_tails_vessel", 0.08),
		},
	},
	{
		id = 2,
		name = "West City & Namek",
		bannerName = "West City & Namek Capsule",
		showName = "Z-Fighters",
		rollCostYen = 500,
		unlockGems = 1200,
		bondName = "Saiyan Pride",
		bondDescription = "2+ Z-Fighters: +12% ATK, +5% CRIT",
		stages = {
			mkStage("w2_act1", 2, 1, false, "Act 1: Wasteland Outpost", 8500, 5200, 260, 900, 0, 450, 6, {
				mkEnemy("Bandit Scout", "w2_desert_bandit", 9500, 820, 132, false, "Wolf Fang", 1.45),
				mkEnemy("Hermit Pupil", "w2_turtle_hermit_disciple", 9000, 760, 130, false, "Kame Blast", 1.40),
			}),
			mkStage("w2_act2", 2, 2, false, "Act 2: Capsule Gravity Room", 12500, 7000, 310, 1150, 0, 520, 7, {
				mkEnemy("Namek Defender", "w2_namekian_guardian", 14000, 1100, 136, false, "Ki Cannon", 1.55),
				mkEnemy("Mystic Trainee", "w2_mystic_prodigy", 12000, 1200, 138, false, "Masenko", 1.55),
			}),
			mkStage("w2_act3", 2, 3, false, "Act 3: Namekian Plateau", 18000, 9500, 360, 1450, 0, 600, 8, {
				mkEnemy("Elite Prince", "w2_saiyan_prince", 19500, 1650, 144, false, "Galick Gun", 1.65),
				mkEnemy("Namek Guard", "w2_namekian_guardian", 17000, 1300, 138, false, "Beam Burst", 1.50),
			}),
			mkStage("w2_act4", 2, 4, false, "Act 4: Cell Arena", 25000, 12500, 420, 1800, 0, 700, 9, {
				mkEnemy("Future Blade", "w2_future_savior", 26000, 2100, 150, false, "Shining Sword", 1.70),
				mkEnemy("Mystic Ace", "w2_mystic_prodigy", 22000, 1850, 146, false, "Burst Rush", 1.60),
			}),
			mkStage("w2_act5", 2, 5, false, "Act 5: Tournament of Power", 34000, 16000, 480, 2200, 0, 850, 10, {
				mkEnemy("Berserk Saiyan", "w2_legendary_berserker", 36000, 2600, 148, false, "Gigantic Meteor", 1.75),
				mkEnemy("God Fusion", "w2_fused_god_warrior", 31000, 2400, 154, false, "Soul Punisher", 1.75),
			}),
			mkStage(
				"w2_boss",
				2,
				6,
				true,
				"Boss: Golden Tyrant",
				46000,
				22000,
				600,
				3000,
				3,
				1100,
				14,
				{ mkEnemy("Golden Tyrant", "boss_golden_tyrant", 62000, 3600, 158, true, "Emperor Death Beam", 1.85) },
				"boss_golden_tyrant",
				0.06
			),
		},
	},
	{
		id = 3,
		name = "Grand Line Seas",
		bannerName = "Grand Line Bounty Pack",
		showName = "Grand Line",
		rollCostYen = 2500,
		unlockGems = 4500,
		bondName = "Grand Fleet",
		bondDescription = "2+ Grand Line: +12% HP, +8% ATK",
		stages = {
			mkStage("w3_act1", 3, 1, false, "Act 1: Syrup Harbor", 55000, 28000, 650, 3600, 0, 1200, 10, {
				mkEnemy("Sniper Lookout", "w3_sniper_king", 52000, 3800, 155, false, "Kabuto Shot", 1.50),
				mkEnemy("Reindeer Medic", "w3_cotton_candy_doctor", 58000, 3200, 152, false, "Heavy Point", 1.45),
			}),
			mkStage("w3_act2", 3, 2, false, "Act 2: Baratie Deck", 72000, 35000, 740, 4400, 0, 1350, 11, {
				mkEnemy("Black Leg Striker", "w3_black_leg_chef", 68000, 4800, 164, false, "Concasse", 1.60),
				mkEnemy("Heart Surgeon", "w3_surgeon_of_death", 66000, 4500, 160, false, "Room Slash", 1.60),
			}),
			mkStage("w3_act3", 3, 3, false, "Act 3: Dressrosa Colosseum", 95000, 44000, 840, 5400, 0, 1500, 12, {
				mkEnemy("Three-Sword Ronin", "w3_pirate_hunter", 88000, 6000, 168, false, "Oni Giri", 1.70),
				mkEnemy("Flame Commander", "w3_flame_emperor", 82000, 5600, 166, false, "Fire Fist", 1.65),
			}),
			mkStage("w3_act4", 3, 4, false, "Act 4: Marineford Bay", 125000, 56000, 960, 6600, 0, 1750, 14, {
				mkEnemy("Red Hair Duelist", "w3_red_haired_emperor", 118000, 7400, 174, false, "Haki Slash", 1.75),
				mkEnemy("Quake Admiral", "w3_darkness_quake", 125000, 7000, 162, false, "Seaquake", 1.75),
			}),
			mkStage("w3_act5", 3, 5, false, "Act 5: Onigashima Roof", 160000, 72000, 1100, 8000, 0, 2000, 16, {
				mkEnemy("Sun Drum", "w3_warrior_of_liberation", 155000, 8800, 178, false, "Red Roc", 1.80),
				mkEnemy("First Mate", "w3_pirate_hunter", 140000, 8200, 172, false, "Asura", 1.75),
			}),
			mkStage("w3_boss", 3, 6, true, "Boss: Strongest Creature", 210000, 95000, 1400, 10500, 4, 2500, 20, {
				mkEnemy(
					"Strongest Creature",
					"boss_strongest_creature",
					260000,
					11500,
					176,
					true,
					"Thunder Bagua",
					1.90
				),
			}, "boss_strongest_creature", 0.05),
		},
	},
	{
		id = 4,
		name = "Tokyo Jujutsu High",
		bannerName = "Tokyo Jujutsu Archive",
		showName = "Cursed Sorcerers",
		rollCostYen = 10000,
		unlockGems = 12000,
		bondName = "Jujutsu Tech",
		bondDescription = "2+ Cursed Sorcerers: +11% ATK, +8% DEF",
		stages = {
			mkStage("w4_act1", 4, 1, false, "Act 1: Cursed Corridor", 260000, 120000, 1500, 12500, 0, 2700, 16, {
				mkEnemy("Speech Student", "w4_cursed_speech_student", 240000, 13000, 178, false, "Blast Away", 1.55),
				mkEnemy("Shadow User", "w4_ten_shadows_initiate", 250000, 13500, 180, false, "Nue Strike", 1.55),
			}),
			mkStage("w4_act2", 4, 2, false, "Act 2: Goodwill Arena", 330000, 150000, 1750, 15000, 0, 3000, 18, {
				mkEnemy("Flash Brawler", "w4_black_flash_vessel", 310000, 16500, 184, false, "Divergent Fist", 1.65),
				mkEnemy(
					"Thunder Sorcerer",
					"w4_thunder_god_sorcerer",
					300000,
					17200,
					188,
					false,
					"Lightning Bolt",
					1.70
				),
			}),
			mkStage("w4_act3", 4, 3, false, "Act 3: Sendai Colony", 420000, 190000, 2000, 18500, 0, 3400, 20, {
				mkEnemy("Curse Queen", "w4_queen_of_curses", 400000, 20000, 190, false, "Love Beam", 1.75),
				mkEnemy("Zen Assassin", "w4_sorcerer_killer", 380000, 22000, 194, false, "Split Soul", 1.75),
			}),
			mkStage("w4_act4", 4, 4, false, "Act 4: Tomb of the Star", 540000, 240000, 2300, 22500, 0, 3900, 22, {
				mkEnemy("Ancient Brain", "w4_brain_of_curses", 520000, 25000, 196, false, "Uzumaki", 1.80),
				mkEnemy("King Vessel", "w4_king_of_curses", 540000, 27000, 198, false, "Cleave", 1.85),
			}),
			mkStage("w4_act5", 4, 5, false, "Act 5: Shinjuku Showdown", 700000, 310000, 2700, 28000, 0, 4500, 25, {
				mkEnemy("Limitless Master", "w4_honored_one", 680000, 31000, 204, false, "Hollow Purple", 1.90),
				mkEnemy("Curse King", "w4_king_of_curses", 660000, 32000, 202, false, "Fuga", 1.90),
			}),
			mkStage("w4_boss", 4, 6, true, "Boss: Divine General", 900000, 420000, 3400, 36000, 5, 5500, 30, {
				mkEnemy(
					"Divine General",
					"boss_divine_general",
					1150000,
					40000,
					206,
					true,
					"Eight-Handled Adaptation",
					2.00
				),
			}, "boss_divine_general", 0.04),
		},
	},
	{
		id = 5,
		name = "Soul Society",
		bannerName = "Soul Society Seireitei Pack",
		showName = "Soul Society",
		rollCostYen = 35000,
		unlockGems = 35000,
		bondName = "Gotei 13",
		bondDescription = "2+ Soul Society: +10% SPD, +10% HP",
		stages = {
			mkStage("w5_act1", 5, 1, false, "Act 1: Rukongai Gate", 1150000, 520000, 3800, 42000, 0, 6000, 26, {
				mkEnemy(
					"Eleventh Vanguard",
					"w5_eleventh_lieutenant",
					1100000,
					46000,
					208,
					false,
					"Howl Zabimaru",
					1.65
				),
				mkEnemy("Ice Prodigy", "w5_ice_prodigy_captain", 1050000, 44000, 212, false, "Frost Dragon", 1.65),
			}),
			mkStage("w5_act2", 5, 2, false, "Act 2: Sokyoku Hill", 1500000, 660000, 4400, 52000, 0, 6800, 28, {
				mkEnemy("Flash Goddess", "w5_thunder_flash_goddess", 1400000, 56000, 220, false, "Raijin Senkei", 1.75),
				mkEnemy("Noble Captain", "w5_noble_senbonzakura", 1450000, 58000, 216, false, "Kageyoshi", 1.75),
			}),
			mkStage("w5_act3", 5, 3, false, "Act 3: Squad 11 Barracks", 1950000, 840000, 5100, 65000, 0, 7800, 30, {
				mkEnemy("Kenpachi", "w5_kenpachi", 1900000, 72000, 218, false, "Nozarashi", 1.85),
				mkEnemy(
					"Squad Scientist",
					"w5_clever_scientist_captain",
					1750000,
					64000,
					220,
					false,
					"Poison Mist",
					1.75
				),
			}),
			mkStage(
				"w5_act4",
				5,
				4,
				false,
				"Act 4: First Division Sanctum",
				2500000,
				1080000,
				6000,
				82000,
				0,
				9000,
				34,
				{
					mkEnemy(
						"Head Captain",
						"w5_head_captain_inferno",
						2450000,
						88000,
						224,
						false,
						"Zanka no Tachi",
						1.90
					),
					mkEnemy(
						"Hogyoku Master",
						"w5_transcendent_strategist",
						2350000,
						86000,
						228,
						false,
						"Kurohitsugi",
						1.90
					),
				}
			),
			mkStage("w5_act5", 5, 5, false, "Act 5: Royal Palace", 3200000, 1400000, 7200, 105000, 0, 11000, 38, {
				mkEnemy("True Bankai", "w5_true_bankai_substitute", 3100000, 108000, 232, false, "True Getsuga", 2.00),
				mkEnemy("Transcendent", "w5_transcendent_strategist", 2900000, 102000, 230, false, "Fragor", 1.95),
			}),
			mkStage(
				"w5_boss",
				5,
				6,
				true,
				"Boss: Almighty King",
				4200000,
				1900000,
				9000,
				140000,
				8,
				15000,
				45,
				{ mkEnemy("Almighty King", "boss_almighty_king", 5200000, 138000, 236, true, "The Almighty", 2.10) },
				"boss_almighty_king",
				0.03
			),
		},
	},
} :: { WorldDef }

Worlds.Bonds = {
	{
		id = "bond_will_of_fire_2",
		name = "Will of Fire (II)",
		requiredShow = "Leaf Shinobi",
		minCount = 2,
		atkBonus = 0.12,
		spdBonus = 0.12,
		hpBonus = 0,
		critDmgBonus = 0,
		dodgeBonus = 0,
		description = "2+ Leaf Shinobi: +12% ATK & SPD",
	},
	{
		id = "bond_will_of_fire_4",
		name = "Will of Fire (IV)",
		requiredShow = "Leaf Shinobi",
		minCount = 4,
		atkBonus = 0.28,
		spdBonus = 0.28,
		hpBonus = 0,
		critDmgBonus = 0,
		dodgeBonus = 0.10,
		description = "4+ Leaf Shinobi: +28% ATK & SPD, +10% Dodge",
	},
	{
		id = "bond_team_7",
		name = "Team 7 Reunion",
		requiredCardIds = { "w1_seventh_hokage", "w1_rinnegan_shadow", "w1_crimson_copy_ninja" },
		atkBonus = 0.25,
		spdBonus = 0.15,
		hpBonus = 0,
		critDmgBonus = 0,
		dodgeBonus = 0,
		description = "Seventh Hokage + Rinnegan Shadow + Crimson Copy Ninja: +25% ATK & +15% SPD",
	},
	{
		id = "bond_z_warrior_2",
		name = "Z-Warrior Spirit (II)",
		requiredShow = "Z-Fighters",
		minCount = 2,
		atkBonus = 0.15,
		spdBonus = 0,
		hpBonus = 0,
		critDmgBonus = 0,
		dodgeBonus = 0,
		description = "2+ Z-Fighters: +15% ATK",
	},
} :: { BondDef }

Worlds.ById = {} :: { [number]: WorldDef }
Worlds.StageById = {} :: { [string]: StageDef }

for _, world in ipairs(Worlds.List) do
	Worlds.ById[world.id] = world
	for _, stage in ipairs(world.stages) do
		Worlds.StageById[stage.id] = stage
	end
end

function Worlds.get(id: number): WorldDef?
	return Worlds.ById[id]
end

function Worlds.getWorld(id: number): WorldDef?
	return Worlds.ById[id]
end

function Worlds.getStage(stageId: string): StageDef?
	return Worlds.StageById[stageId]
end

return Worlds
