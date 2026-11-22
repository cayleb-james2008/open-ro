-- CardDatabase.luau (ReplicatedStorage.Modules.CardDatabase)
-- Canonical catalog for Untitled Anime Card Game
-- 24 anime-homage cards, 6 packs, 6 rarity tiers, 5 traits, 4 craftable charms,
-- 9 weather states (7 standard + 2 Fusion Weather Storms), 5 story bosses, and World Raid Boss.

local CardDatabase = {}

CardDatabase.RARITIES = {
    { id = "Common",    order = 1, oddsText = "1 in 2",      denom = 2,     color = Color3.fromRGB(148, 163, 184), hex = "#94A3B8", xp = 10,  shards = 2 },
    { id = "Rare",      order = 2, oddsText = "1 in 12",     denom = 12,    color = Color3.fromRGB(52, 211, 153),  hex = "#34D399", xp = 24,  shards = 6 },
    { id = "Epic",      order = 3, oddsText = "1 in 60",     denom = 60,    color = Color3.fromRGB(192, 38, 211),  hex = "#C026D3", xp = 65,  shards = 18 },
    { id = "Legendary", order = 4, oddsText = "1 in 400",    denom = 400,   color = Color3.fromRGB(56, 189, 248),  hex = "#38BDF8", xp = 180, shards = 55 },
    { id = "Mythic",    order = 5, oddsText = "1 in 3,500",  denom = 3500,  color = Color3.fromRGB(235, 83, 42),   hex = "#EB532A", xp = 520, shards = 160 },
    { id = "Secret",    order = 6, oddsText = "1 in 50,000", denom = 50000, color = Color3.fromRGB(255, 196, 60),  hex = "#FFC43C", xp = 1800, shards = 500 },
}

CardDatabase.RARITY_BY_ID = {}
for _, r in ipairs(CardDatabase.RARITIES) do
    CardDatabase.RARITY_BY_ID[r.id] = r
end

CardDatabase.BORDERS = {
    { id = "Normal",  chance = 1.0,   mult = 1.00, color = Color3.fromRGB(203, 213, 225) },
    { id = "Shiny",   chance = 0.18,  mult = 1.25, color = Color3.fromRGB(56, 189, 248) },
    { id = "Gold",    chance = 0.075, mult = 1.50, color = Color3.fromRGB(250, 204, 21) },
    { id = "Rainbow", chance = 0.025, mult = 2.00, color = Color3.fromRGB(244, 114, 182) },
    { id = "Void",    chance = 0.008, mult = 2.75, color = Color3.fromRGB(168, 85, 247) },
}

-- Element advantage wheel: Solar > Shadow > Storm > Arcane > Crimson > Solar
CardDatabase.ELEMENT_BEATS = {
    Solar   = "Shadow",
    Shadow  = "Storm",
    Storm   = "Arcane",
    Arcane  = "Crimson",
    Crimson = "Solar",
}

CardDatabase.ELEMENT_COLORS = {
    Solar   = Color3.fromRGB(251, 191, 36),
    Shadow  = Color3.fromRGB(139, 92, 246),
    Storm   = Color3.fromRGB(56, 189, 248),
    Arcane  = Color3.fromRGB(52, 211, 153),
    Crimson = Color3.fromRGB(239, 68, 68),
}

CardDatabase.PACKS = {
    { id = "Starter",           name = "Starter Pack",           cost = 0,   luckBonus = 1.0,  accent = Color3.fromRGB(100, 116, 139), desc = "Free beginner summons for every challenger." },
    { id = "Shibuya Spirit",    name = "Shibuya Spirit Pack",    cost = 150, luckBonus = 1.15, accent = Color3.fromRGB(56, 189, 248),  desc = "Spatial sorcerers & domain curse breakers." },
    { id = "Crimson Pirate",    name = "Crimson Pirate Pack",    cost = 200, luckBonus = 1.25, accent = Color3.fromRGB(239, 68, 68),   desc = "Blazing ocean captains & molten brawlers." },
    { id = "Shadow Gate",       name = "Shadow Gate Pack",       cost = 300, luckBonus = 1.40, accent = Color3.fromRGB(124, 58, 237),  desc = "Monarch commanders rising from abyssal rifts." },
    { id = "Celestial Alchemy", name = "Celestial Alchemy Pack", cost = 450, luckBonus = 1.65, accent = Color3.fromRGB(16, 185, 129),  desc = "Solar spirit shinobi, storm blades & transmuters." },
    { id = "Eclipse Legends",   name = "Eclipse Legends Pack",   cost = 750, luckBonus = 2.10, accent = Color3.fromRGB(245, 158, 11),  desc = "Apex banner holding Secret Supernova Ascended Striker." },
}

CardDatabase.PACK_BY_ID = {}
for _, p in ipairs(CardDatabase.PACKS) do
    CardDatabase.PACK_BY_ID[p.id] = p
end

CardDatabase.TRAITS = {
    { id = "Golden",      weight = 24, atkMult = 1.22, hpMult = 1.05, spdBonus = 0,  coinMult = 1.25, critBonus = 0.05, regenPct = 0,    cdReduce = 0, color = Color3.fromRGB(250, 204, 21), desc = "+22% ATK, +25% Coin drops" },
    { id = "Voidtouched", weight = 18, atkMult = 1.18, hpMult = 1.08, spdBonus = 4,  coinMult = 1.00, critBonus = 0.16, regenPct = 0,    cdReduce = 0, color = Color3.fromRGB(168, 85, 247), desc = "+18% ATK, +16% Crit & Pierce" },
    { id = "Overcharged", weight = 18, atkMult = 1.20, hpMult = 1.00, spdBonus = 6,  coinMult = 1.00, critBonus = 0.08, regenPct = 0,    cdReduce = 1, color = Color3.fromRGB(56, 189, 248), desc = "-1 Skill Cooldown, +20% ATK" },
    { id = "Swift",       weight = 22, atkMult = 1.10, hpMult = 1.00, spdBonus = 22, coinMult = 1.00, critBonus = 0.12, regenPct = 0,    cdReduce = 0, color = Color3.fromRGB(52, 211, 153), desc = "+22 Speed, +12% Crit" },
    { id = "Immortal",    weight = 18, atkMult = 1.08, hpMult = 1.38, spdBonus = 0,  coinMult = 1.00, critBonus = 0.04, regenPct = 0.06, cdReduce = 0, color = Color3.fromRGB(244, 63, 94),  desc = "+38% HP, heals 6% HP each turn" },
}

CardDatabase.TRAIT_BY_ID = {}
for _, t in ipairs(CardDatabase.TRAITS) do
    CardDatabase.TRAIT_BY_ID[t.id] = t
end

CardDatabase.CHARMS = {
    { id = "Samehada",      name = "Samehada Fang",    shardCost = 40,  atkFlat = 95,  hpFlat = 180, spdFlat = 4,  lifesteal = 0.20, critBonus = 0.05, enemySlow = 0.0,  luckBonus = 0.0,  desc = "+95 ATK, +20% Lifesteal" },
    { id = "Spirit Arrow",  name = "Spirit Arrow",     shardCost = 75,  atkFlat = 140, hpFlat = 120, spdFlat = 10, lifesteal = 0.0,  critBonus = 0.22, enemySlow = 0.0,  luckBonus = 0.05, desc = "+140 ATK, +22% Crit Chance" },
    { id = "Gravity Core",  name = "Gravity Core",     shardCost = 120, atkFlat = 110, hpFlat = 420, spdFlat = 0,  lifesteal = 0.0,  critBonus = 0.06, enemySlow = 0.18, luckBonus = 0.10, desc = "+420 HP, -18% Enemy Speed" },
    { id = "Eclipse Crown", name = "Eclipse Crown",    shardCost = 220, atkFlat = 220, hpFlat = 550, spdFlat = 14, lifesteal = 0.10, critBonus = 0.15, enemySlow = 0.08, luckBonus = 0.35, desc = "+220 ATK, +550 HP, +35% Luck" },
}

CardDatabase.CHARM_BY_ID = {}
for _, c in ipairs(CardDatabase.CHARMS) do
    CardDatabase.CHARM_BY_ID[c.id] = c
end

CardDatabase.WEATHERS = {
    {
        id = "Clear",
        luckMult = 1.0,
        fusion = false,
        boostElement = "Solar",
        clockTime = 13.5,
        brightness = 2.8,
        density = 0.16,
        haze = 0.45,
        atmoColor = Color3.fromRGB(185, 230, 255),
        atmoDecay = Color3.fromRGB(125, 185, 245),
        tint = Color3.fromRGB(255, 252, 246),
        ambient = Color3.fromRGB(165, 195, 230),
        exclusiveCard = nil,
        banner = "Sunlit Skies Clear — vibrant island & balanced summons!",
    },
    {
        id = "Rain",
        luckMult = 1.25,
        fusion = false,
        boostElement = "Storm",
        clockTime = 15.6,
        brightness = 2.1,
        density = 0.28,
        haze = 1.2,
        atmoColor = Color3.fromRGB(160, 210, 250),
        atmoDecay = Color3.fromRGB(105, 155, 215),
        tint = Color3.fromRGB(220, 238, 255),
        ambient = Color3.fromRGB(130, 165, 210),
        exclusiveCard = "gale-kunoichi",
        banner = "Thunderstorm Rain! +25% Luck & Gale Kunoichi unlocked!",
    },
    {
        id = "Snow",
        luckMult = 1.35,
        fusion = false,
        boostElement = "Arcane",
        clockTime = 11.4,
        brightness = 2.5,
        density = 0.24,
        haze = 0.9,
        atmoColor = Color3.fromRGB(215, 242, 255),
        atmoDecay = Color3.fromRGB(155, 205, 248),
        tint = Color3.fromRGB(238, 249, 255),
        ambient = Color3.fromRGB(165, 195, 225),
        exclusiveCard = "sage-transmuter",
        banner = "Arcane Snowfall! +35% Luck & Sage Transmuter unlocked!",
    },
    {
        id = "Sandstorm",
        luckMult = 1.50,
        fusion = false,
        boostElement = "Crimson",
        clockTime = 16.5,
        brightness = 2.3,
        density = 0.30,
        haze = 1.4,
        atmoColor = Color3.fromRGB(255, 224, 175),
        atmoDecay = Color3.fromRGB(215, 165, 115),
        tint = Color3.fromRGB(255, 232, 195),
        ambient = Color3.fromRGB(185, 145, 110),
        exclusiveCard = "blazing-tides-corsair",
        banner = "Golden Sandstorm! +50% Luck & Blazing Tides Corsair rate-up!",
    },
    {
        id = "Blood Moon",
        luckMult = 1.85,
        fusion = false,
        boostElement = "Shadow",
        clockTime = 21.8,
        brightness = 1.35,
        density = 0.38,
        haze = 2.0,
        atmoColor = Color3.fromRGB(205, 78, 96),
        atmoDecay = Color3.fromRGB(125, 36, 56),
        tint = Color3.fromRGB(255, 185, 195),
        ambient = Color3.fromRGB(125, 60, 78),
        exclusiveCard = "blood-moon-gravity-ronin",
        banner = "Blood Moon rises! +85% Luck & Blood-Moon Gravity Ronin unlocked!",
    },
    {
        id = "Eclipse 2x Luck",
        luckMult = 2.00,
        fusion = false,
        boostElement = "Shadow",
        clockTime = 20.4,
        brightness = 1.25,
        density = 0.38,
        haze = 2.0,
        atmoColor = Color3.fromRGB(165, 120, 235),
        atmoDecay = Color3.fromRGB(85, 55, 155),
        tint = Color3.fromRGB(224, 200, 255),
        ambient = Color3.fromRGB(98, 76, 138),
        exclusiveCard = "lunar-eclipse-duelist",
        banner = "Solar Eclipse 2x Luck! Void & Eclipse Legends surge!",
    },
    {
        id = "Starfall",
        luckMult = 2.50,
        fusion = false,
        boostElement = "Solar",
        clockTime = 18.8,
        brightness = 2.0,
        density = 0.22,
        haze = 0.85,
        atmoColor = Color3.fromRGB(165, 205, 255),
        atmoDecay = Color3.fromRGB(115, 145, 235),
        tint = Color3.fromRGB(240, 238, 255),
        ambient = Color3.fromRGB(135, 155, 215),
        exclusiveCard = "celestial-star-oracle",
        banner = "Celestial Starfall! 2.5x Luck & Celestial Star Oracle unlocked!",
    },
    {
        id = "Crimson Eclipse",
        luckMult = 5.00,
        fusion = true,
        boostElement = "Crimson",
        clockTime = 20.8,
        brightness = 1.55,
        density = 0.34,
        haze = 1.8,
        atmoColor = Color3.fromRGB(225, 65, 90),
        atmoDecay = Color3.fromRGB(135, 28, 52),
        tint = Color3.fromRGB(255, 190, 200),
        ambient = Color3.fromRGB(140, 62, 78),
        exclusiveCard = "crimson-sea-captain",
        banner = "FUSION WEATHER STORM: Crimson Eclipse (5.0x LUCK)! Mythic surge active!",
    },
    {
        id = "Celestial Tempest",
        luckMult = 4.50,
        fusion = true,
        boostElement = "Storm",
        clockTime = 21.4,
        brightness = 1.75,
        density = 0.30,
        haze = 1.5,
        atmoColor = Color3.fromRGB(115, 180, 255),
        atmoDecay = Color3.fromRGB(60, 95, 195),
        tint = Color3.fromRGB(215, 236, 255),
        ambient = Color3.fromRGB(108, 142, 210),
        exclusiveCard = "thunder-flash-ronin",
        banner = "FUSION WEATHER STORM: Celestial Tempest (4.5x LUCK)! Storm & Solar surge!",
    },
}

CardDatabase.WEATHER_BY_ID = {}
for _, w in ipairs(CardDatabase.WEATHERS) do
    CardDatabase.WEATHER_BY_ID[w.id] = w
end

-- 24 Anime-Homage Cards mapped to the 8 Google Nano Banana portraits
CardDatabase.CARDS = {
    -- Starter Pack (6)
    {
        id = "ember-acolyte",
        title = "Ember Acolyte",
        pack = "Starter",
        rarity = "Common",
        element = "Crimson",
        atk = 115, hp = 520, speed = 72,
        skillName = "Cinder Volley", skillKind = "Burn AoE", cooldown = 3,
        passive = "Ignites all foes for 2 turns of steady flame burn.",
        portrait = "rbxasset://openro-assets/crimson-sea-captain.png",
        weatherLock = nil,
    },
    {
        id = "storm-scout",
        title = "Storm Scout",
        pack = "Starter",
        rarity = "Common",
        element = "Storm",
        atk = 125, hp = 490, speed = 82,
        skillName = "Spark Parry", skillKind = "Counter", cooldown = 3,
        passive = "Enters Counter stance to deflect incoming strikes.",
        portrait = "rbxasset://openro-assets/thunder-flash-ronin.png",
        weatherLock = nil,
    },
    {
        id = "arcane-scribe",
        title = "Arcane Scribe",
        pack = "Starter",
        rarity = "Common",
        element = "Arcane",
        atk = 108, hp = 560, speed = 70,
        skillName = "Sigil Ward", skillKind = "Mirror Shield", cooldown = 3,
        passive = "Projects a crystalline barrier absorbing early burst hits.",
        portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png",
        weatherLock = nil,
    },
    {
        id = "void-initiate",
        title = "Void Initiate",
        pack = "Starter",
        rarity = "Common",
        element = "Shadow",
        atk = 120, hp = 510, speed = 75,
        skillName = "Gravity Cleave", skillKind = "Guard Break", cooldown = 3,
        passive = "Pierces enemy barriers and exposes armor.",
        portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png",
        weatherLock = nil,
    },
    {
        id = "eclipse-arcane-alchemist",
        title = "Steel Alchemist",
        pack = "Starter",
        rarity = "Rare",
        element = "Arcane",
        atk = 230, hp = 960, speed = 80,
        skillName = "Crystal Transmutation", skillKind = "Mirror Shield", cooldown = 2,
        passive = "Equivalent Exchange: converts 15% damage dealt into team shield.",
        portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png",
        weatherLock = nil,
    },
    {
        id = "gale-kunoichi",
        title = "Gale Kunoichi",
        pack = "Starter",
        rarity = "Rare",
        element = "Storm",
        atk = 245, hp = 880, speed = 90,
        skillName = "Tempest Bind", skillKind = "Stun", cooldown = 3,
        passive = "Rain-forged wind step stuns fast targets for 1 turn.",
        portrait = "rbxasset://openro-assets/thunder-flash-ronin.png",
        weatherLock = "Rain",
    },

    -- Shibuya Spirit Pack (4)
    {
        id = "cursed-fist-brawler",
        title = "Cursed Fist Brawler",
        pack = "Shibuya Spirit",
        rarity = "Common",
        element = "Shadow",
        atk = 135, hp = 540, speed = 76,
        skillName = "Black Impact", skillKind = "Guard Break", cooldown = 2,
        passive = "Shatters barriers with concentrated dark-flash strikes.",
        portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png",
        weatherLock = nil,
    },
    {
        id = "shikigami-summoner",
        title = "Shikigami Summoner",
        pack = "Shibuya Spirit",
        rarity = "Rare",
        element = "Shadow",
        atk = 250, hp = 940, speed = 84,
        skillName = "Shade Fang", skillKind = "Lifesteal", cooldown = 3,
        passive = "Spirit hounds siphon 35% of strike damage back as HP.",
        portrait = "rbxasset://openro-assets/shadow-sovereign.png",
        weatherLock = nil,
    },
    {
        id = "domain-curse-blade",
        title = "Domain Curse Blade",
        pack = "Shibuya Spirit",
        rarity = "Epic",
        element = "Arcane",
        atk = 315, hp = 1140, speed = 88,
        skillName = "Astral Barrier", skillKind = "Domain Buff", cooldown = 3,
        passive = "Expands a luminous domain boosting ally ATK & SPD by 22%.",
        portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png",
        weatherLock = nil,
    },
    {
        id = "infinity-void-sorcerer",
        title = "Six-Eyes Sorcerer",
        pack = "Shibuya Spirit",
        rarity = "Mythic",
        element = "Arcane",
        atk = 560, hp = 1780, speed = 96,
        skillName = "Hollow Singularity", skillKind = "Time Stop", cooldown = 4,
        passive = "Limitless Barrier: blocks 35% first-strike damage & freezes time.",
        portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png",
        weatherLock = nil,
    },

    -- Crimson Pirate Pack (4)
    {
        id = "harbor-deckhand",
        title = "Harbor Deckhand",
        pack = "Crimson Pirate",
        rarity = "Common",
        element = "Crimson",
        atk = 130, hp = 550, speed = 74,
        skillName = "Powder Wave", skillKind = "Burn AoE", cooldown = 3,
        passive = "Scorches enemy frontline with cannon embers.",
        portrait = "rbxasset://openro-assets/crimson-sea-captain.png",
        weatherLock = nil,
    },
    {
        id = "iron-cutlass-firstmate",
        title = "Iron Cutlass Firstmate",
        pack = "Crimson Pirate",
        rarity = "Rare",
        element = "Crimson",
        atk = 260, hp = 1020, speed = 82,
        skillName = "Steel Riposte", skillKind = "Counter", cooldown = 2,
        passive = "Deflects heavy blows and counters with crimson steel.",
        portrait = "rbxasset://openro-assets/crimson-sea-captain.png",
        weatherLock = nil,
    },
    {
        id = "blazing-tides-corsair",
        title = "Blazing Tides Corsair",
        pack = "Crimson Pirate",
        rarity = "Legendary",
        element = "Crimson",
        atk = 405, hp = 1360, speed = 89,
        skillName = "Heatwave Barrage", skillKind = "Burn AoE", cooldown = 2,
        passive = "Sandstorm Raider: gains +20% ATK when below 70% HP.",
        portrait = "rbxasset://openro-assets/crimson-sea-captain.png",
        weatherLock = "Sandstorm",
    },
    {
        id = "crimson-sea-captain",
        title = "Joyful Pirate",
        pack = "Crimson Pirate",
        rarity = "Mythic",
        element = "Crimson",
        atk = 545, hp = 1720, speed = 92,
        skillName = "Red Horizon Impact", skillKind = "Burn AoE", cooldown = 3,
        passive = "Molten Will: +25% Crimson team ATK below 60% HP.",
        portrait = "rbxasset://openro-assets/crimson-sea-captain.png",
        weatherLock = nil,
    },

    -- Shadow Gate Pack (4)
    {
        id = "umbral-dusk-stalker",
        title = "Umbral Dusk Stalker",
        pack = "Shadow Gate",
        rarity = "Rare",
        element = "Shadow",
        atk = 255, hp = 920, speed = 87,
        skillName = "Night Drain", skillKind = "Lifesteal", cooldown = 2,
        passive = "Drains vitality with twin shadow daggers.",
        portrait = "rbxasset://openro-assets/shadow-sovereign.png",
        weatherLock = nil,
    },
    {
        id = "blood-moon-gravity-ronin",
        title = "Gravity Ronin",
        pack = "Shadow Gate",
        rarity = "Epic",
        element = "Shadow",
        atk = 330, hp = 1180, speed = 86,
        skillName = "Crimson Orbit Crush", skillKind = "Guard Break", cooldown = 2,
        passive = "Monolith Pull: reduces enemy Speed by 20% under Blood Moon.",
        portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png",
        weatherLock = "Blood Moon",
    },
    {
        id = "abyssal-dread-marshall",
        title = "Abyssal Dread Marshall",
        pack = "Shadow Gate",
        rarity = "Legendary",
        element = "Shadow",
        atk = 430, hp = 1420, speed = 91,
        skillName = "Legion Banner", skillKind = "Domain Buff", cooldown = 3,
        passive = "Empowers shadow allies with +15% Crit & barrier shred.",
        portrait = "rbxasset://openro-assets/shadow-sovereign.png",
        weatherLock = nil,
    },
    {
        id = "shadow-sovereign",
        title = "Shadow Monarch",
        pack = "Shadow Gate",
        rarity = "Mythic",
        element = "Shadow",
        atk = 585, hp = 1840, speed = 95,
        skillName = "Monarch Domain", skillKind = "Lifesteal", cooldown = 3,
        passive = "Arise Legion: defeated foes grant +15% ATK stack to team.",
        portrait = "rbxasset://openro-assets/shadow-sovereign.png",
        weatherLock = nil,
    },

    -- Celestial Alchemy Pack (3)
    {
        id = "sage-transmuter",
        title = "Sage Transmuter",
        pack = "Celestial Alchemy",
        rarity = "Epic",
        element = "Arcane",
        atk = 310, hp = 1160, speed = 83,
        skillName = "Frost Aegis", skillKind = "Mirror Shield", cooldown = 2,
        passive = "Snow-forged crystal barrier reflects 20% incoming damage.",
        portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png",
        weatherLock = "Snow",
    },
    {
        id = "thunder-flash-ronin",
        title = "Thunder Flash Blade",
        pack = "Celestial Alchemy",
        rarity = "Legendary",
        element = "Storm",
        atk = 420, hp = 1240, speed = 100,
        skillName = "Sevenfold Lightning Flash", skillKind = "Stun", cooldown = 2,
        passive = "First Clap: always strikes first with +30% Crit chance.",
        portrait = "rbxasset://openro-assets/thunder-flash-ronin.png",
        weatherLock = nil,
    },
    {
        id = "nine-tail-ember-shinobi",
        title = "Nine-Tail Shinobi",
        pack = "Celestial Alchemy",
        rarity = "Mythic",
        element = "Solar",
        atk = 555, hp = 1760, speed = 94,
        skillName = "Spiral Sun Barrage", skillKind = "Domain Buff", cooldown = 3,
        passive = "Solar Cloak: restores 8% team HP every turn.",
        portrait = "rbxasset://openro-assets/nine-tail-ember-shinobi.png",
        weatherLock = nil,
    },

    -- Eclipse Legends Pack (3)
    {
        id = "lunar-eclipse-duelist",
        title = "Lunar Eclipse Duelist",
        pack = "Eclipse Legends",
        rarity = "Legendary",
        element = "Storm",
        atk = 440, hp = 1320, speed = 97,
        skillName = "Eclipse Counter", skillKind = "Counter", cooldown = 2,
        passive = "Reflects 45% of incoming Eclipse strikes with lightning.",
        portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png",
        weatherLock = "Eclipse 2x Luck",
    },
    {
        id = "celestial-star-oracle",
        title = "Celestial Star Oracle",
        pack = "Eclipse Legends",
        rarity = "Mythic",
        element = "Solar",
        atk = 570, hp = 1800, speed = 96,
        skillName = "Starfall Chronos", skillKind = "Time Stop", cooldown = 4,
        passive = "Starfall Grace: freezes foe cooldowns & grants extra tempo.",
        portrait = "rbxasset://openro-assets/nine-tail-ember-shinobi.png",
        weatherLock = "Starfall",
    },
    {
        id = "supernova-ascended-striker",
        title = "Golden Ascended Warrior",
        pack = "Eclipse Legends",
        rarity = "Secret",
        element = "Solar",
        atk = 760, hp = 2350, speed = 99,
        skillName = "Galactic Star Beam", skillKind = "Burn AoE", cooldown = 3,
        passive = "Ascended Limit: gains +20% ATK & +10% Crit every round.",
        portrait = "rbxasset://openro-assets/supernova-ascended-striker.png",
        weatherLock = nil,
    },
}

CardDatabase.CARD_BY_ID = {}
for _, card in ipairs(CardDatabase.CARDS) do
    CardDatabase.CARD_BY_ID[card.id] = card
end

CardDatabase.STORY_BOSSES = {
    {
        id = 1,
        name = "Ember Dock Tyrant",
        element = "Crimson",
        rewardCoins = 220,
        rewardShards = 12,
        team = {
            { title = "Ember Buccaneer", element = "Crimson", atk = 115, hp = 680, speed = 70, skillKind = "Burn AoE", portrait = "rbxasset://openro-assets/crimson-sea-captain.png" },
            { title = "Cannon Lookout", element = "Storm", atk = 105, hp = 590, speed = 74, skillKind = "Counter", portrait = "rbxasset://openro-assets/thunder-flash-ronin.png" },
            { title = "Harbor Enforcer", element = "Crimson", atk = 135, hp = 790, speed = 72, skillKind = "Guard Break", portrait = "rbxasset://openro-assets/crimson-sea-captain.png" },
            { title = "Ember Dock Tyrant", element = "Crimson", atk = 175, hp = 1150, speed = 78, skillKind = "Burn AoE", portrait = "rbxasset://openro-assets/crimson-sea-captain.png" },
        },
    },
    {
        id = 2,
        name = "Shibuya Curse Weaver",
        element = "Shadow",
        rewardCoins = 420,
        rewardShards = 24,
        team = {
            { title = "Shibuya Phantom", element = "Shadow", atk = 180, hp = 980, speed = 78, skillKind = "Lifesteal", portrait = "rbxasset://openro-assets/shadow-sovereign.png" },
            { title = "Hex Acolyte", element = "Arcane", atk = 165, hp = 940, speed = 76, skillKind = "Mirror Shield", portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png" },
            { title = "Rift Stalker", element = "Shadow", atk = 205, hp = 1120, speed = 82, skillKind = "Stun", portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png" },
            { title = "Shibuya Curse Weaver", element = "Arcane", atk = 265, hp = 1680, speed = 85, skillKind = "Time Stop", portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png" },
        },
    },
    {
        id = 3,
        name = "Thunder Spire Shogun",
        element = "Storm",
        rewardCoins = 680,
        rewardShards = 42,
        team = {
            { title = "Cloud Vanguard", element = "Storm", atk = 250, hp = 1320, speed = 88, skillKind = "Counter", portrait = "rbxasset://openro-assets/thunder-flash-ronin.png" },
            { title = "Arcane Warden", element = "Arcane", atk = 240, hp = 1380, speed = 81, skillKind = "Mirror Shield", portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png" },
            { title = "Tempest Ronin", element = "Storm", atk = 285, hp = 1480, speed = 92, skillKind = "Guard Break", portrait = "rbxasset://openro-assets/thunder-flash-ronin.png" },
            { title = "Thunder Spire Shogun", element = "Storm", atk = 345, hp = 2350, speed = 96, skillKind = "Stun", portrait = "rbxasset://openro-assets/thunder-flash-ronin.png" },
        },
    },
    {
        id = 4,
        name = "Arcane Citadel Grandmaster",
        element = "Arcane",
        rewardCoins = 1050,
        rewardShards = 68,
        team = {
            { title = "Prism Sentinel", element = "Arcane", atk = 330, hp = 1850, speed = 86, skillKind = "Mirror Shield", portrait = "rbxasset://openro-assets/eclipse-arcane-alchemist.png" },
            { title = "Lunar Executioner", element = "Shadow", atk = 365, hp = 1790, speed = 89, skillKind = "Guard Break", portrait = "rbxasset://openro-assets/blood-moon-gravity-ronin.png" },
            { title = "Solar Flame Monk", element = "Solar", atk = 385, hp = 1980, speed = 91, skillKind = "Domain Buff", portrait = "rbxasset://openro-assets/nine-tail-ember-shinobi.png" },
            { title = "Arcane Citadel Grandmaster", element = "Arcane", atk = 455, hp = 3100, speed = 94, skillKind = "Time Stop", portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png" },
        },
    },
    {
        id = 5,
        name = "Impossible Overlord",
        element = "Solar",
        rewardCoins = 1800,
        rewardShards = 120,
        team = {
            { title = "Void Sovereign Guard", element = "Shadow", atk = 460, hp = 2450, speed = 93, skillKind = "Lifesteal", portrait = "rbxasset://openro-assets/shadow-sovereign.png" },
            { title = "Crimson Fleet Admiral", element = "Crimson", atk = 485, hp = 2580, speed = 91, skillKind = "Burn AoE", portrait = "rbxasset://openro-assets/crimson-sea-captain.png" },
            { title = "Limitless Archmage", element = "Arcane", atk = 510, hp = 2620, speed = 95, skillKind = "Domain Buff", portrait = "rbxasset://openro-assets/infinity-void-sorcerer.png" },
            { title = "Impossible Overlord", element = "Solar", atk = 620, hp = 4400, speed = 98, skillKind = "Burn AoE", portrait = "rbxasset://openro-assets/supernova-ascended-striker.png" },
        },
    },
}

CardDatabase.WORLD_RAID_BOSS = {
    id = "abyssal-eclipse-leviathan",
    name = "Abyssal Eclipse Leviathan",
    element = "Shadow",
    baseHp = 18000,
    atk = 380,
    speed = 92,
    portrait = "rbxasset://openro-assets/shadow-sovereign.png",
    durationSec = 90,
    cooldownSec = 90,
}

CardDatabase.UPGRADES = {
    { id = "Luck",  name = "Celestial Luck Blessing", baseCost = 180, maxLevel = 10, perLevelText = "+20% Base Luck per level" },
    { id = "Speed", name = "Chrono Summon Haste",     baseCost = 200, maxLevel = 8,  perLevelText = "+12% Roll Speed per level" },
    { id = "Cap",   name = "Grimoire Storage",        baseCost = 220, maxLevel = 8,  perLevelText = "+10 Card Inventory Cap per level" },
}

CardDatabase.POTIONS = {
    { id = "Luck1",          name = "Luck Potion I",    cost = 120, luckMult = 1.5, speedMult = 1.0,  duration = 120, desc = "1.5x Summon Luck for 120s" },
    { id = "Luck2",          name = "Luck Potion II",   cost = 260, luckMult = 2.0, speedMult = 1.0,  duration = 150, desc = "2.0x Summon Luck for 150s" },
    { id = "Luck3",          name = "Luck Potion III",  cost = 520, luckMult = 3.0, speedMult = 1.0,  duration = 180, desc = "3.0x Summon Luck for 180s" },
    { id = "Speed1",         name = "Speed Potion I",   cost = 110, luckMult = 1.0, speedMult = 1.45, duration = 120, desc = "1.45x Fast Roll Speed for 120s" },
    { id = "Speed2",         name = "Speed Potion II",  cost = 240, luckMult = 1.0, speedMult = 1.9,  duration = 150, desc = "1.9x Fast Roll Speed for 150s" },
    { id = "StarPotion",     name = "Starlight Elixir", cost = 650, luckMult = 2.5, speedMult = 1.6,  duration = 210, desc = "2.5x Luck & 1.6x Speed for 210s" },
    { id = "WeatherReroll",  name = "Weather Reroll",   cost = 350, luckMult = 1.0, speedMult = 1.0,  duration = 0,   desc = "Immediately shifts the server Weather Biome" },
}

CardDatabase.CODES = {
    REBIRTH   = { coins = 500,  shards = 25, luckMult = 2.0, luckSec = 180, speedMult = 1.0,  speedSec = 0 },
    APEX2026  = { coins = 1000, shards = 50, luckMult = 2.5, luckSec = 240, speedMult = 1.6,  speedSec = 180 },
    LUCKBOOST = { coins = 300,  shards = 15, luckMult = 2.0, luckSec = 300, speedMult = 1.0,  speedSec = 0 },
}

return CardDatabase
