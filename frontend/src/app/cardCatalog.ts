// Canonical card art catalog, Nano Banana WebP bundles, Anime Fan-Art Visual Fidelity &
// Title-Epithet Knowledge Point, Elemental Synergies, Foil Tiers, Weather & Fusion Storms,
// and Boss roster for Untitled Anime Card Game (OpenRO v0.9.0).
import bloodMoonImg from "../assets/cards/blood-moon-gravity-ronin.webp";
import crimsonSeaImg from "../assets/cards/crimson-sea-captain.webp";
import eclipseArcaneImg from "../assets/cards/eclipse-arcane-alchemist.webp";
import infinityVoidImg from "../assets/cards/infinity-void-sorcerer.webp";
import nineTailImg from "../assets/cards/nine-tail-ember-shinobi.webp";
import shadowSovereignImg from "../assets/cards/shadow-sovereign.webp";
import supernovaStrikerImg from "../assets/cards/supernova-ascended-striker.webp";
import thunderFlashImg from "../assets/cards/thunder-flash-ronin.webp";

export type CardRarity =
  | "Common"
  | "Uncommon"
  | "Rare"
  | "Epic"
  | "Legendary"
  | "Mythic"
  | "Celestial"
  | "Secret";

export type CardElement = "Flame" | "Void" | "Storm" | "Shadow" | "Arcane";

export type FoilVariant = "Normal" | "Shiny" | "Gold" | "Rainbow" | "Void";

export type CardAssetRecord = {
  card_id: string;
  title: string;
  rarity: CardRarity;
  odds: string;
  element: CardElement;
  atk: number;
  hp: number;
  speed: number;
  passive: string;
  ultimate: string;
  prompt?: string;
  file?: string;
  image_url: string;
  source: "google-nano-banana" | "nano-banana" | "gemini-api" | "openai-api" | "upload";
};

export type HomageArchetype = {
  id: string;
  archetype: string;
  homage_title: string;
  element: CardElement;
  rarity: CardRarity;
  visual_cues: string;
  prompt_template: string;
};

export type ArtKnowledgeGuide = {
  title: string;
  summary: string;
  policy_note: string;
  style_pillars: string[];
  archetypes: HomageArchetype[];
};

export const RARITY_ORDER: CardRarity[] = [
  "Common",
  "Uncommon",
  "Rare",
  "Epic",
  "Legendary",
  "Mythic",
  "Celestial",
  "Secret",
];

export const RARITY_BADGE_CLS: Record<CardRarity, string> = {
  Common: "bg-tag-neutral-bg text-tag-neutral-fg",
  Uncommon: "bg-tag-mint-bg text-tag-mint-fg",
  Rare: "bg-tag-sky-bg text-tag-sky-fg",
  Epic: "bg-tag-research-bg text-tag-research-fg",
  Legendary: "bg-tag-dev-bg text-tag-dev-fg",
  Mythic: "bg-tag-plan-bg text-tag-plan-fg",
  Celestial: "bg-tag-design-bg text-tag-design-fg",
  Secret: "bg-ink-button text-ink-button-fg",
};

export const ELEMENT_BADGE_CLS: Record<CardElement, string> = {
  Flame: "bg-tag-plan-bg text-tag-plan-fg",
  Void: "bg-tag-research-bg text-tag-research-fg",
  Storm: "bg-tag-sky-bg text-tag-sky-fg",
  Shadow: "bg-tag-neutral-bg text-tag-neutral-fg",
  Arcane: "bg-tag-mint-bg text-tag-mint-fg",
};

export const FOIL_MULTIPLIER: Record<FoilVariant, { mult: number; cls: string; odds: string }> = {
  Normal: { mult: 1.0, cls: "bg-board text-ink-2", odds: "Base" },
  Shiny: { mult: 1.5, cls: "bg-tag-sky-bg text-tag-sky-fg", odds: "1/8" },
  Gold: { mult: 2.0, cls: "bg-tag-dev-bg text-tag-dev-fg", odds: "1/25" },
  Rainbow: { mult: 3.0, cls: "bg-tag-design-bg text-tag-design-fg", odds: "1/100" },
  Void: { mult: 5.0, cls: "bg-tag-research-bg text-tag-research-fg", odds: "1/400" },
};

export const BUNDLED_ART_BY_ID: Record<string, string> = {
  "crimson-sea-captain": crimsonSeaImg,
  "infinity-void-sorcerer": infinityVoidImg,
  "thunder-flash-ronin": thunderFlashImg,
  "shadow-sovereign": shadowSovereignImg,
  "nine-tail-ember-shinobi": nineTailImg,
  "supernova-ascended-striker": supernovaStrikerImg,
  "blood-moon-gravity-ronin": bloodMoonImg,
  "eclipse-arcane-alchemist": eclipseArcaneImg,
};

export const APEX_CARDS: CardAssetRecord[] = [
  {
    card_id: "supernova-ascended-striker",
    title: "Golden Ascended Warrior",
    rarity: "Secret",
    odds: "1 in 8,192",
    element: "Flame",
    atk: 680,
    hp: 2100,
    speed: 98,
    passive: "Ascended Limit: gains +20% ATK & +10% Crit every round",
    ultimate: "Galactic Star Beam (4.0x Cosmic DMG, ignores Shields)",
    file: "supernova-ascended-striker.webp",
    image_url: supernovaStrikerImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable golden-haired ascended martial warrior surging in supernova aura, battle-torn gi, starlight sparks, crisp ink linework, zero text or watermarks.",
  },
  {
    card_id: "shadow-sovereign",
    title: "Shadow Monarch",
    rarity: "Celestial",
    odds: "1 in 2,048",
    element: "Shadow",
    atk: 560,
    hp: 1650,
    speed: 94,
    passive: "Arise Legion: defeated foes grant +15% ATK stack to team",
    ultimate: "Monarch Domain (3.2x Shadow Barrage + Lifesteal)",
    file: "shadow-sovereign.webp",
    image_url: shadowSovereignImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable obsidian-cloaked shadow monarch wielding twin glowing violet-blue daggers, phantom armored knights rising behind in neon indigo mist, zero text.",
  },
  {
    card_id: "infinity-void-sorcerer",
    title: "Six-Eyes Sorcerer",
    rarity: "Celestial",
    odds: "1 in 2,048",
    element: "Void",
    atk: 540,
    hp: 1720,
    speed: 96,
    passive: "Limitless Barrier: blocks 35% of incoming first-strike damage",
    ultimate: "Hollow Singularity (3.4x True Void DMG + 1-turn Stun)",
    file: "infinity-void-sorcerer.webp",
    image_url: infinityVoidImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable silver-haired six-eyes sorcerer with luminous cyan gaze conjuring a collapsing crimson-and-indigo singularity sphere, dramatic anime key visual, zero text.",
  },
  {
    card_id: "nine-tail-ember-shinobi",
    title: "Nine-Tail Shinobi",
    rarity: "Mythic",
    odds: "1 in 512",
    element: "Flame",
    atk: 440,
    hp: 1520,
    speed: 92,
    passive: "Solar Cloak: restores 8% team HP every turn",
    ultimate: "Spiral Sun Barrage (2.9x Flame + Team Speed Boost)",
    file: "nine-tail-ember-shinobi.webp",
    image_url: nineTailImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable orange-and-gold nine-tail shinobi cloaked in translucent solar flame chakra channeling a spinning wind-fire sphere, dynamic speedlines, zero text.",
  },
  {
    card_id: "crimson-sea-captain",
    title: "Joyful Pirate",
    rarity: "Mythic",
    odds: "1 in 512",
    element: "Flame",
    atk: 420,
    hp: 1480,
    speed: 88,
    passive: "Molten Will: +25% Flame team ATK when below 60% HP",
    ultimate: "Red Horizon Impact (2.8x AoE Flame + Burn)",
    file: "crimson-sea-captain.webp",
    image_url: crimsonSeaImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable fearless red-coated joyful pirate captain leaping over crashing ocean waves with blazing ember fists and billowing crimson cape, vibrant shonen shading, zero text.",
  },
  {
    card_id: "thunder-flash-ronin",
    title: "Thunder Flash Blade",
    rarity: "Legendary",
    odds: "1 in 128",
    element: "Storm",
    atk: 360,
    hp: 1150,
    speed: 100,
    passive: "First Clap: always strikes first with +30% Crit chance",
    ultimate: "Sevenfold Lightning Flash (2.6x Crit Storm Strike)",
    file: "thunder-flash-ronin.webp",
    image_url: thunderFlashImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable yellow-haori lightning swordsman mid-iaido slash surrounded by crackling golden thunder arcs and storm clouds, sharp high-contrast anime key art, zero text.",
  },
  {
    card_id: "blood-moon-gravity-ronin",
    title: "Gravity Ronin",
    rarity: "Epic",
    odds: "1 in 32",
    element: "Void",
    atk: 295,
    hp: 1080,
    speed: 84,
    passive: "Monolith Pull: reduces enemy Speed by 20%",
    ultimate: "Crimson Orbit Crush (2.3x Void + Armor Break)",
    file: "blood-moon-gravity-ronin.webp",
    image_url: bloodMoonImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable scarred gravity ronin in midnight cloak beneath a crimson moon pulling floating basalt boulders into orbit with a gleaming katana, zero text.",
  },
  {
    card_id: "eclipse-arcane-alchemist",
    title: "Steel Alchemist",
    rarity: "Rare",
    odds: "1 in 8",
    element: "Arcane",
    atk: 230,
    hp: 960,
    speed: 80,
    passive: "Equivalent Exchange: converts 15% DMG into Team Shield",
    ultimate: "Crystal Transmutation (2.0x Arcane + Party Shield)",
    file: "eclipse-arcane-alchemist.webp",
    image_url: eclipseArcaneImg,
    source: "google-nano-banana",
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable crimson-coat steel alchemist clapping hands over a glowing turquoise transmutation circle with crystalline shields erupting around, crisp ink contours, zero text.",
  },
];

export type WeatherProfile = {
  id: string;
  name: string;
  luckMult: number;
  boostedElement: CardElement;
  elementBonusPct: number;
  desc: string;
  badgeCls: string;
};

export const WEATHER_PROFILES: WeatherProfile[] = [
  {
    id: "clear",
    name: "Clear Sky",
    luckMult: 1.0,
    boostedElement: "Arcane",
    elementBonusPct: 10,
    desc: "1.0x Baseline Luck · +10% Arcane Shield",
    badgeCls: "bg-tag-neutral-bg text-tag-neutral-fg",
  },
  {
    id: "rain",
    name: "Rain",
    luckMult: 1.5,
    boostedElement: "Storm",
    elementBonusPct: 18,
    desc: "1.5x Roll Luck · +18% Storm Speed",
    badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
  },
  {
    id: "snow",
    name: "Snow",
    luckMult: 1.75,
    boostedElement: "Arcane",
    elementBonusPct: 22,
    desc: "1.75x Roll Luck · +22% Arcane Shield",
    badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
  },
  {
    id: "sandstorm",
    name: "Sandstorm",
    luckMult: 2.0,
    boostedElement: "Flame",
    elementBonusPct: 22,
    desc: "2.0x Roll Luck · +22% Flame ATK",
    badgeCls: "bg-tag-plan-bg text-tag-plan-fg",
  },
  {
    id: "blood_moon",
    name: "Blood Moon",
    luckMult: 2.5,
    boostedElement: "Shadow",
    elementBonusPct: 25,
    desc: "2.5x Roll Luck · +25% Shadow Lifesteal",
    badgeCls: "bg-pill-pro-bg text-pill-pro-fg",
  },
  {
    id: "eclipse",
    name: "Eclipse 2x Luck",
    luckMult: 3.0,
    boostedElement: "Void",
    elementBonusPct: 28,
    desc: "3.0x Eclipse Luck · +28% Void Crit",
    badgeCls: "bg-tag-research-bg text-tag-research-fg",
  },
  {
    id: "starfall",
    name: "Starfall",
    luckMult: 4.0,
    boostedElement: "Flame",
    elementBonusPct: 30,
    desc: "4.0x Secret Luck · +30% All Burst",
    badgeCls: "bg-tag-design-bg text-tag-design-fg",
  },
  {
    id: "celestial_tempest",
    name: "Celestial Tempest",
    luckMult: 4.5,
    boostedElement: "Storm",
    elementBonusPct: 34,
    desc: "Fusion Storm · 4.5x Luck · +34% Storm Crit",
    badgeCls: "bg-tag-sky-bg text-tag-sky-fg",
  },
  {
    id: "crimson_eclipse",
    name: "Crimson Eclipse",
    luckMult: 5.0,
    boostedElement: "Void",
    elementBonusPct: 40,
    desc: "Fusion Storm · 5.0x Luck · +40% All DMG",
    badgeCls: "bg-ink-button text-ink-button-fg",
  },
];

export type TowerBossDef = {
  floor: number;
  name: string;
  title: string;
  element: CardElement;
  hp: number;
  atk: number;
  rewardCoins: number;
  impossible?: boolean;
  cardArtId: string;
};

export const TOWER_BOSSES: TowerBossDef[] = [
  { floor: 1, name: "Obsidian Sentinel", title: "Floor 1 Guardian", element: "Arcane", hp: 1600, atk: 185, rewardCoins: 250, cardArtId: "eclipse-arcane-alchemist" },
  { floor: 2, name: "Crimson Orbit Ronin", title: "Floor 2 Duelist", element: "Void", hp: 2300, atk: 245, rewardCoins: 420, cardArtId: "blood-moon-gravity-ronin" },
  { floor: 3, name: "Stormblade Vanguard", title: "Floor 3 Captain", element: "Storm", hp: 3100, atk: 310, rewardCoins: 650, cardArtId: "thunder-flash-ronin" },
  { floor: 4, name: "Solar Fleet Admiral", title: "Floor 4 Warlord", element: "Flame", hp: 4200, atk: 375, rewardCoins: 950, cardArtId: "crimson-sea-captain" },
  { floor: 5, name: "Void Monarch (Phase II)", title: "Impossible Boss Raid", element: "Shadow", hp: 6200, atk: 460, rewardCoins: 1800, impossible: true, cardArtId: "shadow-sovereign" },
  { floor: 10, name: "Hollow Singularity", title: "Floor 10 Celestial Boss", element: "Void", hp: 9400, atk: 590, rewardCoins: 3200, impossible: true, cardArtId: "infinity-void-sorcerer" },
  { floor: 25, name: "Solar Leviathan", title: "Floor 25 Mythic Raid", element: "Flame", hp: 16500, atk: 820, rewardCoins: 6500, impossible: true, cardArtId: "nine-tail-ember-shinobi" },
  { floor: 50, name: "Chronos Ascended Apex", title: "Floor 50 Final Boss", element: "Flame", hp: 28000, atk: 1180, rewardCoins: 15000, impossible: true, cardArtId: "supernova-ascended-striker" },
];

export const ART_KNOWLEDGE_FALLBACK: ArtKnowledgeGuide = {
  title: "Anime Fan-Art Visual Fidelity & Title-Epithet Knowledge Point",
  summary:
    "OpenRO builds iconic anime-inspired Roblox games where generated fan-art MUST look unmistakably recognizable as the original character, while every in-game display name uses the character's iconic title/epithet instead of trademarked proper names.",
  policy_note:
    "Visual Fidelity + Title-Epithet Rule: Keep the exact unmistakable silhouette, hair, outfit cues, elemental aura, and heroic expression fans recognize instantly, while replacing trademarked proper names with iconic title epithets (Joyful Pirate, Six-Eyes Sorcerer, Shadow Monarch, Nine-Tail Shinobi, Thunder Flash Blade, Steel Alchemist, Gravity Ronin, Golden Ascended Warrior) and keeping artwork watermark-free and text-free.",
  style_pillars: [
    "Unmistakable Visual Fidelity: generated card art must clearly look like the iconic anime character silhouette, costume, and aura fans expect.",
    "Title-Epithet Display Naming: always replace trademarked proper names with iconic titles (e.g. Joyful Pirate, Six-Eyes Sorcerer, Shadow Monarch, Nine-Tail Shinobi, Thunder Flash Blade, Steel Alchemist, Gravity Ronin, Golden Ascended Warrior).",
    "3:4 vertical trading-card framing with bold cel-shaded ink contours, dramatic rim light, and zero baked-in text or watermarks so Roblox ImageLabels & Decals stay razor-sharp.",
    "Clean separation of artwork assets (`data_dir/game-assets/`) and `.rbxlx` `Decal`/`ImageLabel` bindings via `asset-manifest.json`.",
  ],
  archetypes: [
    {
      id: "joyful-pirate-epithet",
      archetype: "Straw-Hat Pirate Captain",
      homage_title: "Joyful Pirate",
      element: "Flame",
      rarity: "Mythic",
      visual_cues: "Crimson coat, straw hat silhouette, blazing ember fists over ocean spray",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable red-coated joyful pirate captain leaping over crashing ocean waves with blazing ember fists and billowing crimson cape, zero text.",
    },
    {
      id: "six-eyes-sorcerer-epithet",
      archetype: "Limitless Spatial Sorcerer",
      homage_title: "Six-Eyes Sorcerer",
      element: "Void",
      rarity: "Celestial",
      visual_cues: "Spiky silver hair, luminous cyan six-eyes gaze, collapsing violet-crimson singularity orb",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable silver-haired six-eyes sorcerer channeling a glowing crimson-and-violet singularity orb between outstretched fingers, crisp ink lines, zero text.",
    },
    {
      id: "shadow-monarch-epithet",
      archetype: "Shadow Necromancer Sovereign",
      homage_title: "Shadow Monarch",
      element: "Shadow",
      rarity: "Celestial",
      visual_cues: "Obsidian coat, twin violet-cyan spectral daggers, rising phantom shadow knights",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable obsidian-cloaked shadow monarch wielding twin glowing violet spectral daggers with shadow knights emerging behind, zero text.",
    },
    {
      id: "nine-tail-shinobi-epithet",
      archetype: "Nine-Tail Chakra Ninja",
      homage_title: "Nine-Tail Shinobi",
      element: "Flame",
      rarity: "Mythic",
      visual_cues: "Golden-orange chakra cloak, translucent flame tails, spinning spiral sphere",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable orange-and-gold nine-tail shinobi cloaked in glowing flame chakra channeling a spinning energy sphere, zero text.",
    },
    {
      id: "thunder-flash-blade-epithet",
      archetype: "Lightning Iaido Swordsman",
      homage_title: "Thunder Flash Blade",
      element: "Storm",
      rarity: "Legendary",
      visual_cues: "Amber-gold haori, mid-flash katana draw, crackling golden lightning arcs",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable swordsman in golden-amber patterned haori unleashing a blinding thunderclap katana dash through storm sparks, zero text.",
    },
    {
      id: "golden-ascended-warrior-epithet",
      archetype: "Ascended Golden Martial Artist",
      homage_title: "Golden Ascended Warrior",
      element: "Flame",
      rarity: "Secret",
      visual_cues: "Radiant golden supernova aura, spiky golden crest, battle-torn gi",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, unmistakably recognizable golden ascended martial warrior bursting with golden-white supernova lightning and starlight energy waves, dynamic camera angle, zero text.",
    },
  ],
};

export function resolveCardImage(cardId: string, apiImageUrl?: string): string {
  if (apiImageUrl && (apiImageUrl.startsWith("/api/assets/") || apiImageUrl.startsWith("data:image/"))) {
    return apiImageUrl;
  }
  return BUNDLED_ART_BY_ID[cardId] || shadowSovereignImg;
}

export function normalizeAssetList(raw: unknown): CardAssetRecord[] {
  const byId = new Map<string, CardAssetRecord>();
  for (const c of APEX_CARDS) byId.set(c.card_id, c);

  if (raw && typeof raw === "object") {
    const obj = raw as Record<string, unknown>;
    const list = Array.isArray(obj.assets)
      ? obj.assets
      : Array.isArray(obj.cards)
        ? obj.cards
        : Array.isArray(raw)
          ? raw
          : [];

    for (const item of list) {
      if (!item || typeof item !== "object") continue;
      const r = item as Record<string, unknown>;
      const cardId = String(r.card_id || r.id || "").trim();
      if (!cardId) continue;
      const base = byId.get(cardId);
      const file = typeof r.file === "string" ? r.file : typeof r.filename === "string" ? r.filename : base?.file;
      const rawUrl =
        typeof r.image_url === "string"
          ? r.image_url
          : typeof r.url === "string"
            ? r.url
            : typeof r.data_url === "string"
              ? r.data_url
              : file && file.length > 0 && !file.startsWith("data:")
                ? `/api/assets/${encodeURIComponent(file)}`
                : undefined;

      const rarity = (typeof r.rarity === "string" && RARITY_ORDER.includes(r.rarity as CardRarity)
        ? r.rarity
        : base?.rarity || "Epic") as CardRarity;
      const element = (typeof r.element === "string" && ["Flame", "Void", "Storm", "Shadow", "Arcane"].includes(r.element)
        ? r.element
        : base?.element || "Void") as CardElement;
      const srcRaw = String(r.source || base?.source || "upload");
      const source: CardAssetRecord["source"] =
        srcRaw === "google-nano-banana" ||
        srcRaw === "nano-banana" ||
        srcRaw === "gemini-api" ||
        srcRaw === "openai-api" ||
        srcRaw === "upload"
          ? srcRaw
          : srcRaw === "openai-image"
            ? "openai-api"
            : "upload";

      const isBundledDefault =
        Boolean(base) && (source === "google-nano-banana" || source === "nano-banana");
      const rawTitle =
        typeof r.epithet_title === "string" && r.epithet_title.trim()
          ? r.epithet_title.trim()
          : typeof r.title_epithet === "string" && r.title_epithet.trim()
            ? r.title_epithet.trim()
            : isBundledDefault && base
              ? base.title
              : typeof r.title === "string" && r.title.trim()
                ? r.title.trim()
                : base?.title || cardId;

      byId.set(cardId, {
        card_id: cardId,
        title: rawTitle,
        rarity,
        odds: typeof r.odds === "string" && r.odds.trim() ? r.odds.trim() : base?.odds || "1 in 64",
        element,
        atk: typeof r.atk === "number" && Number.isFinite(r.atk) ? r.atk : base?.atk || 320,
        hp: typeof r.hp === "number" && Number.isFinite(r.hp) ? r.hp : base?.hp || 1100,
        speed: typeof r.speed === "number" && Number.isFinite(r.speed) ? r.speed : base?.speed || 86,
        passive: typeof r.passive === "string" && r.passive.trim() ? r.passive.trim() : base?.passive || "Battle Focus: +15% crit power",
        ultimate: typeof r.ultimate === "string" && r.ultimate.trim() ? r.ultimate.trim() : base?.ultimate || "Apex Strike (2.5x Elemental DMG)",
        prompt: typeof r.prompt === "string" ? r.prompt : base?.prompt,
        file,
        image_url: resolveCardImage(cardId, rawUrl),
        source,
      });
    }
  }
  return [...byId.values()];
}

export function normalizeArtKnowledge(raw: unknown): ArtKnowledgeGuide {
  if (!raw || typeof raw !== "object") return ART_KNOWLEDGE_FALLBACK;
  const o = raw as Record<string, unknown>;
  const pillars = Array.isArray(o.style_pillars)
    ? o.style_pillars.filter((x): x is string => typeof x === "string" && x.trim().length > 0)
    : Array.isArray(o.principles)
      ? o.principles.filter((x): x is string => typeof x === "string" && x.trim().length > 0)
      : Array.isArray(o.guidelines)
        ? o.guidelines.filter((x): x is string => typeof x === "string" && x.trim().length > 0)
        : ART_KNOWLEDGE_FALLBACK.style_pillars;

  const baseTpl =
    typeof o.prompt_template === "string" && o.prompt_template.trim()
      ? o.prompt_template.trim()
      : "High-craft modern anime cel-shaded trading card illustration (3:4 portrait), dynamic heroic pose, bold inked contours, glowing {element} elemental aura, dramatic rim lighting, original stylized homage character '{title}' ({rarity} tier), no text, no watermark, no borders.";

  const rawArch = Array.isArray(o.archetypes)
    ? o.archetypes
    : Array.isArray(o.archetype_translations)
      ? o.archetype_translations
      : [];
  const archetypes: HomageArchetype[] = rawArch
    .filter((a): a is Record<string, unknown> => Boolean(a && typeof a === "object"))
    .map((a, idx) => {
      const element = (["Flame", "Void", "Storm", "Shadow", "Arcane"].includes(String(a.element))
        ? String(a.element)
        : "Flame") as CardElement;
      const rarity = (RARITY_ORDER.includes(String(a.rarity) as CardRarity)
        ? String(a.rarity)
        : "Mythic") as CardRarity;
      const homageTitle = String(a.epithet_title || a.title_epithet || a.homage_title || a.title || "Iconic Title");
      const rawPrompt = String(a.prompt_template || a.prompt || "").trim();
      const promptTpl =
        rawPrompt ||
        baseTpl
          .replace(/\{title\}/g, homageTitle)
          .replace(/\{element\}/g, element)
          .replace(/\{rarity\}/g, rarity);
      return {
        id: String(a.id || `arch-${idx}`),
        archetype: String(a.archetype || a.inspiration || "Anime Hero Archetype"),
        homage_title: homageTitle,
        element,
        rarity,
        visual_cues: String(a.visual_cues || a.palette || "Cel-shaded portrait with distinct elemental aura"),
        prompt_template: promptTpl,
      };
    })
    .filter((a) => a.prompt_template.length > 0);

  const extraRules = [o.visual_fidelity_rule, o.title_epithet_rule].filter(
    (x): x is string => typeof x === "string" && x.trim().length > 0,
  );
  const combinedPolicy =
    extraRules.length > 0
      ? extraRules.join(" ")
      : typeof o.policy_note === "string" && o.policy_note.trim()
        ? o.policy_note.trim()
        : ART_KNOWLEDGE_FALLBACK.policy_note;

  return {
    title: typeof o.title === "string" && o.title.trim() ? o.title.trim() : ART_KNOWLEDGE_FALLBACK.title,
    summary: typeof o.summary === "string" && o.summary.trim() ? o.summary.trim() : ART_KNOWLEDGE_FALLBACK.summary,
    policy_note: combinedPolicy,
    style_pillars: pillars.length ? pillars : ART_KNOWLEDGE_FALLBACK.style_pillars,
    archetypes: archetypes.length ? archetypes : ART_KNOWLEDGE_FALLBACK.archetypes,
  };
}
