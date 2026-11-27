//! Built-in Original Anime-Style Character Design Knowledge Point for OpenRO.
//!
//! Ensures OpenRO welcomes anime-inspired Roblox game concepts while strictly
//! enforcing 100% original character design: original silhouettes, costumes,
//! props, lore, names, balanced combat kits, and eye-level neutral three-quarter
//! cel-shaded image generation prompts with zero copied franchise likenesses.

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct ArchetypeGuide {
    pub id: &'static str,
    pub card_id: &'static str,
    pub homage_title: &'static str,
    pub epithet_title: &'static str,
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

pub const VISUAL_FIDELITY_RULE: &str =
    "All generated anime-style character art MUST be 100% original — original silhouette, hair, eyes, costume, props, and lore — with high-craft cel-shaded studio quality and zero copied likenesses from existing franchises.";

pub const TITLE_EPITHET_NAMING_RULE: &str =
    "Character display names and titles MUST be 100% original (e.g., \"Cinder-Sail Navigator\", \"Prism-Rift Archivist\", \"Velvet-Bell Warden\", \"Amber-Crane Dancer\", \"Gale-Lantern Fencer\", \"Verdigris Glass Tinker\", \"Basalt-Tide Anchorite\", \"Helios Clockwork Paragon\") — never trademarked names or thinly veiled franchise look-alikes.";

pub const POLICY_PRINCIPLES: [&str; 6] = [
    "Embrace original anime-inspired, shonen, and cel-shaded card-battler Roblox games built around 100% original characters, worlds, and lore.",
    "Original Character Design Rule: every character MUST have original silhouettes, costumes, props, lore, and names — zero copied likenesses or franchise look-alikes.",
    "Deliver bold modern anime visual craft: expressive silhouettes, clean cel-shaded key-visual polish, dynamic 3:4 portrait poses, dramatic rim lighting, and cohesive elemental palettes.",
    "Keep generated card artwork strictly free of logos, trademarks, watermarks, or embedded text so Roblox Studio Decals, ImageLabels, and foil border overlays compose cleanly.",
    "Pair every original character archetype with authentic Roblox gameplay depth: elemental synergies (Flame, Void, Storm, Shadow, Arcane), passives, burst ultimates, and rarity odds tiers.",
    "Default to reference_search (pose/lighting mood references saved to gitignored art/references/) -> ChatGPT shared-browser image generation (art_generate with neutral eye-level / three-quarter framing, single-line prompt, never Higgsfield Soul Cinema), with Gemini / OpenAI optional.",
];

pub const STYLE_RULES: [&str; 5] = [
    "Aspect ratio 3:4 vertical trading card portrait, hero centered in upper two-thirds with explosive elemental foreground FX.",
    "100% original anime-style character design: distinct original silhouette, outfit, weapon/prop, color palette, and original name with zero copied franchise likenesses.",
    "High-contrast modern anime key-visual cel-shading, razor-sharp ink linework, dramatic rim light, subtle speed lines.",
    "Rarity-coded aura intensity: Rare (clean elemental shimmer), Epic (twin vortex), Legendary (lightning/radiant halo), Mythic (full-frame flame/aura storm), Celestial (cosmic horizon fracture), Secret (supernova prismatic eclipse).",
    "Zero text, zero watermarks, zero logos, and zero baked-in card frame borders so the UI & Roblox SurfaceGui can render Foil/Shiny/Gold/Rainbow/Void frames dynamically.",
];

pub const ANIME_ARCHETYPES: [ArchetypeGuide; 8] = [
    ArchetypeGuide {
        id: "skyship-astrolabe-navigator",
        card_id: "cinder-sail-navigator",
        homage_title: "Cinder-Sail Navigator",
        epithet_title: "Ashwake Quartermaster",
        visual_archetype: "Sunlit sky-ship navigator in a cropped brass-buttoned indigo brigandine coat with a glowing astrolabe lantern",
        rarity: "Mythic",
        odds: "1 in 512",
        element: "Flame",
        atk: 420,
        hp: 1480,
        speed: 88,
        palette: "hearth amber, indigo navy, burnished brass",
        silhouette_cues: "cropped brass-riveted navigator coat, braided auburn hair with brass compass pins, floating astrolabe lantern",
        safe_naming_pattern: "<Original Motif> <Maritime/Guild Role>",
        passive: "Beacon Drift: +25% Flame team ATK when below 60% HP",
        ultimate: "Astrolabe Flare (2.8x AoE Flame + Burn)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original sunlit sky-ship navigator character with braided auburn hair pinned by brass compass clasps, cropped indigo brigandine coat with amber trim, holding a glowing clockwork astrolabe lantern surrounded by warm ember sparks, bold cel-shading, razor-sharp ink linework, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "observatory-prism-mage",
        card_id: "prism-rift-archivist",
        homage_title: "Prism-Rift Archivist",
        epithet_title: "Refraction Keeper",
        visual_archetype: "Monocle-wearing star-library mage in an asymmetrical slate-and-violet mantle orbiting glass polyhedra",
        rarity: "Celestial",
        odds: "1 in 2,048",
        element: "Void",
        atk: 540,
        hp: 1720,
        speed: 96,
        palette: "prismatic cyan, plum violet, slate charcoal, pearl",
        silhouette_cues: "asymmetrical scholar mantle with crystal shoulder clasp, brass monocle, three orbiting prism octahedra",
        safe_naming_pattern: "<Original Phenomenon> <Scholarly/Mystic Title>",
        passive: "Prism Ward: blocks 35% of incoming first-strike damage",
        ultimate: "Refracted Horizon (3.4x True Void DMG + 1-turn Stun)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original astral library archivist with dark teal bob hair and a brass monocle, asymmetrical slate-violet mantle fastened by a crystal clasp, three glowing glass octahedron prisms orbiting open palms against a starry observatory arch, crisp cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "cloud-ridge-storm-fencer",
        card_id: "gale-lantern-fencer",
        homage_title: "Gale-Lantern Fencer",
        epithet_title: "Tempest Duelist",
        visual_archetype: "Swift cloud-ridge fencer in a layered teal wind-cape wielding a forked bronze-hilted rapier",
        rarity: "Legendary",
        odds: "1 in 128",
        element: "Storm",
        atk: 360,
        hp: 1150,
        speed: 100,
        palette: "cerulean cyan, storm teal, copper gold, cloud white",
        silhouette_cues: "layered asymmetrical wind-cape, ribbon-tied ponytail, forked bronze rapier trailing storm arcs",
        safe_naming_pattern: "<Weather/Instrument Noun> <Original Duelist Class>",
        passive: "Tailwind Riposte: always strikes first with +30% Crit chance",
        ultimate: "Squall Needle Volley (2.6x Crit Storm Strike)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original cloud-ridge fencer with ribbon-tied dark blue hair in a layered teal wind-cape and leather gauntlets, holding a forked bronze-hilted rapier wreathed in crisp cyan wind-and-spark ribbons, high-contrast anime cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "belfry-censer-sentinel",
        card_id: "velvet-bell-warden",
        homage_title: "Velvet-Bell Warden",
        epithet_title: "Dusk Chime Sentinel",
        visual_archetype: "Nocturnal cathedral sentinel carrying a wrought-iron censer halberd and twilight chime ribbons",
        rarity: "Celestial",
        odds: "1 in 2,048",
        element: "Shadow",
        atk: 560,
        hp: 1650,
        speed: 94,
        palette: "velvet plum, midnight indigo, tarnished silver, lantern lilac",
        silhouette_cues: "high-collared bell-warden cloak, twin silver chime-ribbons, wrought-iron censer halberd",
        safe_naming_pattern: "<Nocturnal Motif> <Original Sentinel Rank>",
        passive: "Twilight Echo: defeated foes grant +15% ATK stack to team",
        ultimate: "Belfry Requiem (3.2x Shadow Barrage + Lifesteal)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original dusk cathedral sentinel with braided charcoal hair and amber eyes, wearing a velvet plum high-collared cloak with silver chime ribbons, wielding a wrought-iron censer halberd with lilac mist wisps, crisp anime cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "solstice-origami-dancer",
        card_id: "amber-crane-dancer",
        homage_title: "Amber-Crane Dancer",
        epithet_title: "Solstice Ribbon Weaver",
        visual_archetype: "Festival hearth-dancer weaving twin sunlit silk fans and glowing origami crane embers",
        rarity: "Mythic",
        odds: "1 in 512",
        element: "Flame",
        atk: 440,
        hp: 1520,
        speed: 92,
        palette: "vermilion coral, marigold gold, ivory silk, warm dusk",
        silhouette_cues: "flowing pleated festival sleeves, twin lacquered ember fans, circling paper-crane sparks",
        safe_naming_pattern: "<Original Craft/Season Motif> <Performer/Guardian>",
        passive: "Hearth Cadence: restores 8% team HP every turn",
        ultimate: "Solstice Crane Waltz (2.9x Flame + Team Speed Boost)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original solstice festival dancer in ivory and vermilion pleated sleeves with gold hairpin tassels, holding twin lacquered battle fans surrounded by swirling glowing origami crane embers, vibrant anime cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "meridian-sundial-vanguard",
        card_id: "helios-clockwork-paragon",
        homage_title: "Helios Clockwork Paragon",
        epithet_title: "Dawn Meridian Champion",
        visual_archetype: "Armored solar vanguard wearing segmented porcelain-white plates and an orbital sundial halo",
        rarity: "Secret",
        odds: "1 in 8,192",
        element: "Flame",
        atk: 680,
        hp: 2100,
        speed: 98,
        palette: "porcelain white, sunrise gold, coral plasma, cobalt",
        silhouette_cues: "segmented porcelain-and-brass shoulder plates, floating astrolabe sundial halo, radiant gauntlet blades",
        safe_naming_pattern: "<Original Astral/Craft Prefix> <Champion Title>",
        passive: "Meridian Surge: gains +20% ATK & +10% Crit every round",
        ultimate: "Zenith Sundial Cleave (4.0x Radiant DMG, ignores Shields)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original solar vanguard with swept copper-brown hair in segmented porcelain-white and brass armor, a floating golden astrolabe halo behind shoulders and coral-gold energy blades on gauntlets, crisp anime key-visual cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "abyssal-trench-anchorite",
        card_id: "basalt-tide-anchorite",
        homage_title: "Basalt-Tide Anchorite",
        epithet_title: "Reef Monolith Keeper",
        visual_archetype: "Deep-trench warden in weathered sea-stone pauldrons wielding a twin-pronged bronze tidepolearm",
        rarity: "Epic",
        odds: "1 in 32",
        element: "Void",
        atk: 295,
        hp: 1080,
        speed: 84,
        palette: "basalt slate, seafoam cyan, deep abyssal teal, bronze",
        silhouette_cues: "carved sea-stone shoulder plates, draped kelp-green sash, twin-pronged bronze tidepolearm",
        safe_naming_pattern: "<Geological/Oceanic Motif> <Original Warden>",
        passive: "Tidal Undertow: reduces enemy Speed by 20%",
        ultimate: "Abyssal Reef Crash (2.3x Void + Armor Break)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original sea-trench guardian in weathered basalt shoulder armor and draped sea-green sash, wielding a twin-pronged bronze tidepolearm pulsing with abyssal cyan bioluminescent rings, dramatic cel-shading, zero text, zero watermark, no logos",
    },
    ArchetypeGuide {
        id: "botanical-stained-glass-artificer",
        card_id: "verdigris-glass-tinker",
        homage_title: "Verdigris Glass Tinker",
        epithet_title: "Prism Botanist",
        visual_archetype: "Greenhouse artificer in a verdigris coat projecting stained-glass botanical lotus shields",
        rarity: "Rare",
        odds: "1 in 8",
        element: "Arcane",
        atk: 230,
        hp: 960,
        speed: 80,
        palette: "verdigris mint, stained-glass amber, mahogany leather, pearl",
        silhouette_cues: "botanical satchel, brass goggles on forehead, floating stained-glass lotus bucklers",
        safe_naming_pattern: "<Original Material/Craft Prefix> <Artisan Class>",
        passive: "Verdant Refraction: converts 15% DMG into Team Shield",
        ultimate: "Cathedral Glass Bloom (2.0x Arcane + Party Shield)",
        art_prompt: "Modern anime trading card illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, original greenhouse artificer with messy chestnut hair and brass goggles on forehead, wearing a verdigris coat and leather satchel, projecting luminous stained-glass lotus shields from a brass prism rod, sharp cel-shaded anime art, zero text, zero watermark, no logos",
    },
];

pub fn policy_principles() -> &'static [&'static str] {
    &POLICY_PRINCIPLES
}

pub fn anime_archetypes() -> &'static [ArchetypeGuide] {
    &ANIME_ARCHETYPES
}

/// Concise knowledge block injected into the OpenRO agent system prompt so
/// anime-inspired Roblox game requests are guided toward 100% original
/// anime-style character designs rather than copied likenesses.
pub fn prompt_guidance() -> &'static str {
    "ORIGINAL ANIME-STYLE CHARACTER DESIGN KNOWLEDGE POINT:\n\
     - Build anime-inspired, shonen, and card-battler Roblox games with 100% original anime-style characters (original silhouettes, costumes, lore, and names; zero copied likenesses or franchise look-alikes).\n\
     - ORIGINAL VISUAL & NAMING RULE: every generated character MUST feature an original silhouette, costume, prop, palette, and original name/title (e.g., \"Cinder-Sail Navigator\", \"Prism-Rift Archivist\", \"Velvet-Bell Warden\", \"Amber-Crane Dancer\", \"Gale-Lantern Fencer\", \"Verdigris Glass Tinker\", \"Basalt-Tide Anchorite\", \"Helios Clockwork Paragon\") — never copied characters or franchise epithets.\n\
     - ART PIPELINE: call `reference_search` first to gather pose and lighting references into the project's gitignored `art/references/` folder, then call `art_generate` (default provider: ChatGPT shared browser; never Higgsfield Soul Cinema) with a SINGLE-LINE prompt using neutral `eye-level` or `neutral three-quarter` camera framing (NEVER low-angle / worm's-eye / dynamic-action-angle phrases that fail the angle gate).\n\
     - Save art into project-relative `art/` paths (`art/cards/<id>/art.png`, `art/frames/`, `art/ui/`, `art/store/`), upload authorised assets via `assets_upload` into `assets.lock.json`, and bind uploaded `rbxassetid://` entries through `src/shared/AssetIds.luau` (never hardcoded local asset URI paths)."
}

/// Build a deterministic, original anime-style card art prompt for any custom
/// original character design.
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
    let cues_raw = if visual_cues.trim().is_empty() {
        "original anime-style hero silhouette, distinctive tailored battle attire, bespoke weapon prop, dramatic elemental aura"
    } else {
        visual_cues.trim()
    };
    let cues: String = cues_raw.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "Modern anime trading card key-visual illustration, 3:4 vertical portrait, eye-level neutral three-quarter framing, 100% original anime-style character '{clean_title}' ({clean_rarity} tier, {clean_elem} element), original silhouette, original costume and lore cues, {cues}, bold cel-shading, razor-sharp ink linework, dramatic rim lighting, explosive {clean_elem} particle energy, zero copied franchise likenesses, zero text, zero watermark, no logos, no frame border"
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
                "archetype": a.visual_archetype,
                "homage_title": a.homage_title,
                "epithet_title": a.epithet_title,
                "title_epithet": a.epithet_title,
                "visual_archetype": a.visual_archetype,
                "rarity": a.rarity,
                "odds": a.odds,
                "element": a.element,
                "atk": a.atk,
                "hp": a.hp,
                "speed": a.speed,
                "palette": a.palette,
                "silhouette_cues": a.silhouette_cues,
                "visual_cues": a.silhouette_cues,
                "safe_naming_pattern": a.safe_naming_pattern,
                "passive": a.passive,
                "ultimate": a.ultimate,
                "art_prompt": a.art_prompt,
                "prompt_template": a.art_prompt,
            })
        })
        .collect();
    json!({
        "ok": true,
        "title": "Original Anime-Style Character Design Knowledge Point",
        "never_refuse_anime_homage": true,
        "summary": "Guides anime-inspired Roblox game concepts toward 100% original anime-style characters with distinct silhouettes, costumes, lore, original names, balanced combat kits, and ready-to-generate card art prompts.",
        "policy_note": POLICY_PRINCIPLES[0],
        "visual_fidelity_rule": VISUAL_FIDELITY_RULE,
        "title_epithet_rule": TITLE_EPITHET_NAMING_RULE,
        "epithet_examples": ANIME_ARCHETYPES
            .iter()
            .map(|a| a.homage_title)
            .collect::<Vec<_>>(),
        "principles": POLICY_PRINCIPLES,
        "style_pillars": STYLE_RULES,
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
