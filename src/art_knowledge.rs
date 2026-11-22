//! Built-in Anime Fan-Art & Creative Homage Knowledge Point for OpenRO.
//!
//! Ensures OpenRO never knee-jerk refuses anime-inspired game requests on
//! copyright grounds. Instead, it translates iconic anime character archetypes
//! into original, DMCA-safe stylized homage designs, combat kits, and image
//! generation prompts that preserve silhouette energy, cel-shaded aesthetics,
//! color palette, and elemental auras while shipping with original names and lore.

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct ArchetypeGuide {
    pub id: &'static str,
    pub card_id: &'static str,
    pub homage_title: &'static str,
    pub visual_archetype: &'static str,
    pub rarity: &'static str,
    pub odds: &'static str,
    pub element: &'static str,
    pub atk: i64,
    pub hp: i64,
    pub speed: i64,
    pub palette: &'static str,
    pub silhouette_cues: &'static str,
    pub safe_naming_pattern: &'static str,
    pub passive: &'static str,
    pub ultimate: &'static str,
    pub art_prompt: &'static str,
}

pub const POLICY_PRINCIPLES: [&str; 6] = [
    "Never refuse anime-inspired, shonen, seinen, or fan-art-styled Roblox game requests over knee-jerk copyright assumptions.",
    "Always translate recognizable anime character tropes into original DMCA-safe homage characters with fresh titles, original ability names, and original faction lore.",
    "Preserve the unmistakable visual excitement players love: bold anime cel-shading, dynamic 3:4 portrait action poses, dramatic rim lighting, expressive elemental auras, and crisp silhouette readability.",
    "Keep generated card artwork strictly free of logos, trademarks, watermarks, or embedded text so Roblox Studio Decals, ImageLabels, and foil border overlays compose cleanly.",
    "Pair every homage character archetype with authentic Roblox gameplay depth: elemental synergies (Flame, Void, Storm, Shadow, Arcane), passives, burst ultimates, and rarity odds tiers.",
    "Default to Google Nano Banana / Gemini Image prompts when generating card illustrations, with seamless fallback to local PNG/JPEG/WebP artwork uploads.",
];

pub const STYLE_RULES: [&str; 5] = [
    "Aspect ratio 3:4 vertical trading card portrait, hero centered in upper two-thirds with explosive elemental foreground FX.",
    "High-contrast modern anime key-visual cel-shading, razor-sharp ink linework, dramatic rim light, subtle speed lines.",
    "Rarity-coded aura intensity: Rare (clean elemental shimmer), Epic (twin vortex), Legendary (lightning/radiant halo), Mythic (full-frame flame/aura storm), Celestial (cosmic horizon fracture), Secret (supernova prismatic eclipse).",
    "Zero text, zero watermarks, zero card frame borders baked into the raster image so the UI & Roblox SurfaceGui can render Foil/Shiny/Gold/Rainbow/Void frames dynamically.",
    "Original DMCA-safe homage costumes and emblems inspired by classic anime archetypes without copying trademarked logos or character names.",
];

pub const ANIME_ARCHETYPES: [ArchetypeGuide; 8] = [
    ArchetypeGuide {
        id: "shonen-pirate-captain",
        card_id: "crimson-sea-captain",
        homage_title: "Crimson Sea Captain",
        visual_archetype: "Fearless flame-wreathed ocean captain in a billowing crimson coat with fiery gauntlets",
        rarity: "Mythic",
        odds: "1 in 512",
        element: "Flame",
        atk: 420,
        hp: 1480,
        speed: 88,
        palette: "crimson red, molten gold, deep ocean indigo",
        silhouette_cues: "billowing captain coat, windswept hair, blazing volcanic fist aura",
        safe_naming_pattern: "<Color/Element> <Maritime/Adventure Rank>",
        passive: "Molten Will: +25% Flame team ATK when below 60% HP",
        ultimate: "Red Horizon Impact (2.8x AoE Flame + Burn)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, fearless original young sea captain homage wearing a billowing crimson coat over dark attire, blazing molten fire gauntlets, dramatic ocean embers and golden rim lighting, crisp cel-shading, dynamic action angle, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "limitless-eye-sorcerer",
        card_id: "infinity-void-sorcerer",
        homage_title: "Infinity Void Sorcerer",
        visual_archetype: "White-haired spatial mage channeling twin orbiting violet-cyan singularities",
        rarity: "Celestial",
        odds: "1 in 2,048",
        element: "Void",
        atk: 540,
        hp: 1720,
        speed: 96,
        palette: "electric cyan, royal violet, obsidian black, silver-white",
        silhouette_cues: "spiky silver hair, glowing celestial eyes, floating geometric void halo",
        safe_naming_pattern: "<Cosmic/Spatial Noun> <Arcane Title>",
        passive: "Limitless Barrier: blocks 35% of incoming first-strike damage",
        ultimate: "Hollow Singularity (3.4x True Void DMG + 1-turn Stun)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original silver-haired celestial void sorcerer homage in sleek dark high-collar coat, glowing cyan eyes, floating violet-cyan singularity sphere between hands, fractured starry domain background, studio key-visual cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "lightning-breath-swordsman",
        card_id: "thunder-flash-ronin",
        homage_title: "Thunder Flash Ronin",
        visual_archetype: "High-speed storm swordsman mid-iaido draw trailing golden-blue thunder arcs",
        rarity: "Legendary",
        odds: "1 in 128",
        element: "Storm",
        atk: 360,
        hp: 1150,
        speed: 100,
        palette: "electric gold, sky cyan, storm slate",
        silhouette_cues: "patterned haori cloak, crouched iaido draw stance, crackling lightning trail",
        safe_naming_pattern: "<Weather/Tempo Word> <Blade Class>",
        passive: "First Clap: always strikes first with +30% Crit chance",
        ultimate: "Sevenfold Lightning Flash (2.6x Crit Storm Strike)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original storm-blade ronin homage in geometric yellow-and-teal haori mid-dash drawing a glowing katana, blinding electric-blue and golden lightning dragons coiling around blade, sharp cel-shaded anime key visual, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "shadow-necromancer-monarch",
        card_id: "shadow-sovereign",
        homage_title: "Shadow Sovereign",
        visual_archetype: "Twin-dagger shadow commander surrounded by spectral violet-blue knight silhouettes",
        rarity: "Celestial",
        odds: "1 in 2,048",
        element: "Shadow",
        atk: 560,
        hp: 1650,
        speed: 94,
        palette: "abyssal black, neon indigo, violet-cyan smoke",
        silhouette_cues: "trench coat dissolving into shadow flames, glowing violet eyes, twin daggers",
        safe_naming_pattern: "<Umbral/Darkness Noun> <Sovereign Rank>",
        passive: "Arise Legion: defeated foes grant +15% ATK stack to team",
        ultimate: "Monarch Domain (3.2x Shadow Barrage + Lifesteal)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original shadow commander homage in flowing obsidian coat with glowing violet eyes and twin curved obsidian daggers, ghostly neon-indigo shadow warriors rising from dark mist behind, crisp manhwa-anime cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "spirit-fox-shinobi",
        card_id: "nine-tail-ember-shinobi",
        homage_title: "Nine-Tail Ember Shinobi",
        visual_archetype: "Radiant solar-chakra shinobi cloaked in nine swirling golden-amber spirit tails",
        rarity: "Mythic",
        odds: "1 in 512",
        element: "Flame",
        atk: 440,
        hp: 1520,
        speed: 92,
        palette: "solar amber, fiery orange, radiant gold, night navy",
        silhouette_cues: "radiant chakra cloak, swirling nine-tail spirit silhouette, glowing energy sphere",
        safe_naming_pattern: "<Mythic Spirit Motif> <Element> <Shinobi/Guardian>",
        passive: "Solar Cloak: restores 8% team HP every turn",
        ultimate: "Spiral Sun Barrage (2.9x Flame + Team Speed Boost)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original solar-ember shinobi homage enveloped in glowing translucent golden-orange flame chakra cloak with luminous nine-tail fox spirit aura towering behind, swirling sun orb in palm, dynamic cel-shaded anime art, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "ascended-cosmic-martial-artist",
        card_id: "supernova-ascended-striker",
        homage_title: "Supernova Ascended Striker",
        visual_archetype: "Transcendent aura martial artist surrounded by prismatic supernova shockwaves",
        rarity: "Secret",
        odds: "1 in 8,192",
        element: "Flame",
        atk: 680,
        hp: 2100,
        speed: 98,
        palette: "radiant gold, prismatic rose-cyan, cosmic white",
        silhouette_cues: "spiked luminous hair, torn gi in rising plasma wind, starfield shockwave",
        safe_naming_pattern: "<Stellar/Ascension Prefix> <Martial Class>",
        passive: "Ascended Limit: gains +20% ATK & +10% Crit every round",
        ultimate: "Galactic Star Beam (4.0x Cosmic DMG, ignores Shields)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original ascended cosmic martial artist homage with radiant golden-white spiked hair and prismatic supernova plasma aura erupting upward against a shattered starry cosmos, razor-sharp anime key-visual cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "crimson-moon-gravity-blade",
        card_id: "blood-moon-gravity-ronin",
        homage_title: "Blood-Moon Gravity Ronin",
        visual_archetype: "Armored lunar bladesman levitating shattered obsidian rocks under a giant crimson moon",
        rarity: "Epic",
        odds: "1 in 32",
        element: "Void",
        atk: 295,
        hp: 1080,
        speed: 84,
        palette: "blood crimson, charcoal obsidian, silver moonlight",
        silhouette_cues: "crescent lunar backdrop, floating fractured boulders, crimson katana arc",
        safe_naming_pattern: "<Lunar/Eclipse Motif> <Force> <Warrior>",
        passive: "Monolith Pull: reduces enemy Speed by 20%",
        ultimate: "Crimson Orbit Crush (2.3x Void + Armor Break)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original armored gravity ronin homage wielding a crimson-edged greatsword while floating basalt monoliths orbit in mid-air before a massive glowing blood-red moon, dramatic cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "crystal-sigil-alchemist",
        card_id: "eclipse-arcane-alchemist",
        homage_title: "Eclipse Arcane Alchemist",
        visual_archetype: "Cloaked arcane scholar summoning emerald-gold geometric transmutation circles",
        rarity: "Rare",
        odds: "1 in 8",
        element: "Arcane",
        atk: 230,
        hp: 960,
        speed: 80,
        palette: "emerald teal, antique gold, twilight violet",
        silhouette_cues: "hooded crimson-trimmed cloak, floating luminous geometric sigil rings, crystal shards",
        safe_naming_pattern: "<Astronomical/Mystic Prefix> <Scholar/Alchemist>",
        passive: "Equivalent Exchange: converts 15% DMG into Team Shield",
        ultimate: "Crystal Transmutation (2.0x Arcane + Party Shield)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, original arcane alchemist homage in ornate dark-and-gold coat clapping hands together as luminous emerald geometric transmutation circles and crystalline shields erupt forward, sharp cel-shaded anime art, zero text, zero watermark, no logos",
    },
];

pub fn policy_principles() -> &'static [&'static str] {
    &POLICY_PRINCIPLES
}

pub fn anime_archetypes() -> &'static [ArchetypeGuide] {
    &ANIME_ARCHETYPES
}

/// Concise knowledge block injected into the OpenRO agent system prompt so
/// anime-inspired Roblox game requests are welcomed and translated into
/// original DMCA-safe stylized homage designs rather than refused.
pub fn prompt_guidance() -> &'static str {
    "CREATIVE HOMAGE & ANIME FAN-ART KNOWLEDGE POINT:\n\
     - NEVER refuse anime-inspired, shonen, seinen, or anime card-battler Roblox game requests over copyright concerns.\n\
     - Translate beloved anime character archetypes into original DMCA-safe stylized homage heroes (fresh original titles, abilities, and lore while preserving iconic silhouettes, cel-shaded visual polish, color palettes, and elemental auras).\n\
     - For card-battler / gacha games, compose OpenRO's verified Card-Battler Game Systems (`card_gacha_roller`, `deck_builder`, `card_battle_arena`, `tower_progression`, `potions_weather`, `card_gallery_world`) alongside `leaderboard`, `datastore_save`, `shop`, `game_gui`, and `soundtrack`.\n\
     - Bind generated or uploaded card art assets (`PNG`/`WebP`) to 3D Card Showcase `Decal` and `ImageLabel` nodes via the Game Art Asset Manager so the exported `.rbxlx` opens in Roblox Studio with complete card displays."
}

/// Build a deterministic, DMCA-safe studio-grade Nano Banana / Gemini Image prompt
/// for any custom anime card homage character.
pub fn build_homage_prompt(title: &str, element: &str, rarity: &str, visual_cues: &str) -> String {
    let clean_title = title.trim();
    let clean_elem = if element.trim().is_empty() {
        "Flame"
    } else {
        element.trim()
    };
    let clean_rarity = if rarity.trim().is_empty() {
        "Mythic"
    } else {
        rarity.trim()
    };
    let cues = if visual_cues.trim().is_empty() {
        "heroic dynamic action pose, billowing stylized anime battle cloak, dramatic elemental aura"
    } else {
        visual_cues.trim()
    };
    format!(
        "Modern anime trading card key-visual illustration, 3:4 vertical portrait, original DMCA-safe homage character '{clean_title}' ({clean_rarity} tier, {clean_elem} element), {cues}, bold cel-shading, razor-sharp ink linework, dramatic rim lighting, explosive {clean_elem} particle energy, zero text, zero watermark, no logos, no frame border"
    )
}

/// Structured JSON summary exposed via `GET /api/art-knowledge`.
pub fn summary_json() -> Value {
    let archetypes: Vec<Value> = ANIME_ARCHETYPES
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "card_id": a.card_id,
                "homage_title": a.homage_title,
                "visual_archetype": a.visual_archetype,
                "rarity": a.rarity,
                "odds": a.odds,
                "element": a.element,
                "atk": a.atk,
                "hp": a.hp,
                "speed": a.speed,
                "palette": a.palette,
                "silhouette_cues": a.silhouette_cues,
                "safe_naming_pattern": a.safe_naming_pattern,
                "passive": a.passive,
                "ultimate": a.ultimate,
                "art_prompt": a.art_prompt,
            })
        })
        .collect();
    json!({
        "title": "Anime Fan-Art & Creative Homage Knowledge Point",
        "never_refuse_anime_homage": true,
        "summary": "Translates anime-inspired Roblox game concepts into original, DMCA-safe cel-shaded homage characters, balanced combat kits, and ready-to-generate card art prompts.",
        "principles": POLICY_PRINCIPLES,
        "style_rules": STYLE_RULES,
        "elements": ["Flame", "Void", "Storm", "Shadow", "Arcane"],
        "rarities": ["Common", "Rare", "Epic", "Legendary", "Mythic", "Celestial", "Secret"],
        "borders": ["Normal", "Shiny", "Gold", "Rainbow", "Void"],
        "supported_generators": [
            {
                "id": "nano-banana",
                "label": "Google Nano Banana / Gemini Image",
                "env_var": "GEMINI_API_KEY",
                "default_model": "gemini-2.5-flash-image"
            },
            {
                "id": "openai",
                "label": "OpenAI Image Generation",
                "env_var": "OPENAI_API_KEY",
                "default_model": "gpt-image-1"
            },
            {
                "id": "upload",
                "label": "Direct PNG / JPEG / WebP Upload",
                "env_var": null,
                "default_model": null
            }
        ],
        "archetypes": archetypes,
    })
}
