// Canonical card art catalog, Nano Banana WebP bundles, Anime Fan-Art & Creative Homage
// Knowledge Point, Elemental Synergies, Foil Tiers, Weather profiles & Boss roster
// for Anime Card Battles: Rebirth — Apex Edition (OpenRO v0.6.0).
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
    title: "Supernova Ascended Striker",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, cosmic martial artist ascending in golden supernova aura, spiky radiant hair, battle-torn gi, starlight sparks, crisp ink linework, zero text or watermarks.",
  },
  {
    card_id: "shadow-sovereign",
    title: "Shadow Sovereign",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, dark sovereign in obsidian cloak wielding twin glowing violet-blue spectral daggers, phantom armored knights rising behind in neon indigo mist, zero text.",
  },
  {
    card_id: "infinity-void-sorcerer",
    title: "Infinity Void Sorcerer",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, silver-haired void sorcerer with glowing cyan eyes raising hand to conjure a collapsing crimson-and-indigo singularity sphere, dramatic anime key visual, zero text.",
  },
  {
    card_id: "nine-tail-ember-shinobi",
    title: "Nine-Tail Ember Shinobi",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, orange-and-gold ember shinobi cloaked in translucent solar flame tails channeling a spinning wind-fire energy sphere, dynamic speedlines, zero text.",
  },
  {
    card_id: "crimson-sea-captain",
    title: "Crimson Sea Captain",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, fearless red-coated sea captain leaping over crashing ocean waves with blazing ember gauntlets and billowing crimson cape, vibrant shonen shading, zero text.",
  },
  {
    card_id: "thunder-flash-ronin",
    title: "Thunder Flash Ronin",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, yellow-haori lightning swordsman mid-iaido slash surrounded by crackling golden thunder arcs and storm clouds, sharp high-contrast anime key art, zero text.",
  },
  {
    card_id: "blood-moon-gravity-ronin",
    title: "Blood-Moon Gravity Ronin",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, scarred gravity ronin in midnight cloak beneath a huge crimson moon pulling floating basalt boulders into orbit with a gleaming katana, zero text.",
  },
  {
    card_id: "eclipse-arcane-alchemist",
    title: "Eclipse Arcane Alchemist",
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
    prompt: "Cel-shaded anime trading card portrait, 3:4 vertical, crimson-cloak alchemist clapping hands over a glowing turquoise transmutation circle with crystalline shields erupting around, crisp ink contours, zero text.",
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
  title: "Anime Fan-Art & Creative Homage Knowledge Point",
  summary:
    "OpenRO actively builds anime-inspired Roblox experiences by translating beloved shonen and fantasy archetypes into original, DMCA-safe homage characters, distinct skill kits, and custom cel-shaded card art.",
  policy_note:
    "Never refuse an anime-inspired game request out of knee-jerk copyright caution. Preserve the high-energy silhouette, color language, elemental aura, and dramatic framing players love while authoring original character titles, unique move names, and studio-clean artwork ready to ship on Roblox.",
  style_pillars: [
    "3:4 vertical trading-card framing with bold cel-shaded ink contours and high-contrast rim light.",
    "Distinct elemental aura palettes: Flame (molten gold/crimson), Void (indigo/magenta), Storm (electric amber/cyan), Shadow (violet/obsidian), Arcane (emerald/turquoise).",
    "Original DMCA-safe homage titles and ultimate move names — zero trademarked logos, watermarks, or baked-in text so Roblox ImageLabels & Decals stay crisp.",
    "Clean separation of artwork assets (`data_dir/game-assets/`) and `.rbxlx` `Decal`/`ImageLabel` bindings via `asset-manifest.json`.",
  ],
  archetypes: [
    {
      id: "shadow-monarch-homage",
      archetype: "Shadow Necromancer Monarch",
      homage_title: "Shadow Sovereign",
      element: "Shadow",
      rarity: "Celestial",
      visual_cues: "Obsidian coat, twin violet-cyan spectral blades, rising phantom knights",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, dark sovereign in obsidian cloak wielding twin glowing violet spectral daggers with shadow knights emerging behind, zero text.",
    },
    {
      id: "limitless-sorcerer-homage",
      archetype: "Spatial Infinity Sorcerer",
      homage_title: "Infinity Void Sorcerer",
      element: "Void",
      rarity: "Celestial",
      visual_cues: "Silver hair, luminous cyan gaze, collapsing violet-red void sphere",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, silver-haired sorcerer channeling a glowing crimson-and-violet singularity orb between outstretched fingers, crisp ink lines, zero text.",
    },
    {
      id: "thunder-iaido-homage",
      archetype: "Lightning Iaido Swordsman",
      homage_title: "Thunder Flash Ronin",
      element: "Storm",
      rarity: "Legendary",
      visual_cues: "Amber-gold haori, mid-flash katana draw, jagged golden lightning arcs",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, swordsman in golden-amber patterned haori unleashing a blinding thunderclap katana dash through storm sparks, zero text.",
    },
    {
      id: "ascended-martial-homage",
      archetype: "Ascended Starlight Martial Artist",
      homage_title: "Supernova Ascended Striker",
      element: "Flame",
      rarity: "Secret",
      visual_cues: "Radiant supernova aura, spiky golden crest, cosmic energy surge",
      prompt_template: "Cel-shaded anime trading card portrait, 3:4 vertical, ascended cosmic martial artist bursting with golden-white supernova lightning and starlight energy waves, dynamic camera angle, zero text.",
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

      byId.set(cardId, {
        card_id: cardId,
        title: typeof r.title === "string" && r.title.trim() ? r.title.trim() : base?.title || cardId,
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
      const homageTitle = String(a.homage_title || a.title || "Original Homage");
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

  return {
    title: typeof o.title === "string" && o.title.trim() ? o.title.trim() : ART_KNOWLEDGE_FALLBACK.title,
    summary: typeof o.summary === "string" && o.summary.trim() ? o.summary.trim() : ART_KNOWLEDGE_FALLBACK.summary,
    policy_note: typeof o.policy_note === "string" && o.policy_note.trim() ? o.policy_note.trim() : ART_KNOWLEDGE_FALLBACK.policy_note,
    style_pillars: pillars.length ? pillars : ART_KNOWLEDGE_FALLBACK.style_pillars,
    archetypes: archetypes.length ? archetypes : ART_KNOWLEDGE_FALLBACK.archetypes,
  };
}
