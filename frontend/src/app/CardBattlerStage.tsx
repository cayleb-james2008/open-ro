import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { toast } from "sonner";
import {
  CloudLightning,
  Coins,
  Crown,
  Flame,
  Gift,
  Layers,
  Palette,
  Play,
  RefreshCw,
  RotateCcw,
  Shield,
  Sparkles,
  Square,
  Swords,
  Trophy,
  Zap,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { instant, press, reveal } from "@/motion";
import {
  ELEMENT_BADGE_CLS,
  FOIL_MULTIPLIER,
  RARITY_BADGE_CLS,
  RARITY_ORDER,
  TOWER_BOSSES,
  WEATHER_PROFILES,
  type CardAssetRecord,
  type CardElement,
  type FoilVariant,
  type TowerBossDef,
} from "./cardCatalog";

type BattlerTab = "roll" | "deck" | "arena";

type OwnedCard = {
  uid: string;
  card_id: string;
  foil: FoilVariant;
  stars: number; // 1..5
  copies: number;
};

type CombatEvent = {
  id: number;
  text: string;
  tone: "crit" | "ult" | "shield" | "boss" | "win" | "info";
};

type BattleState = {
  floor: number;
  impossible: boolean;
  boss: TowerBossDef;
  bossHp: number;
  bossMaxHp: number;
  bossPhase: 1 | 2;
  bossStunned: boolean;
  bossBurning: boolean;
  bossIntent: "strike" | "ultimate";
  teamHp: number;
  teamMaxHp: number;
  teamShield: number;
  burstGauge: number; // 0..100
  turn: number;
  outcome: "fighting" | "victory" | "defeat";
  events: CombatEvent[];
};

const CARD_TRAITS = ["Golden", "Voidtouched", "Overcharged", "Swift", "Immortal"] as const;
const SKILL_POPUPS = [
  "Counter",
  "Lifesteal",
  "Mirror Shield",
  "Burn AoE",
  "Guard Break",
  "Stun",
  "Time Stop",
  "Domain Buff",
] as const;

const PROMO_CODES: Record<string, { coins: number; potions: number; label: string }> = {
  REBIRTH: { coins: 600, potions: 2, label: "+600 Coins & 2 Luck Potions" },
  APEX2026: { coins: 1000, potions: 3, label: "+1,000 Coins & 3 Luck Potions" },
  LUCKBOOST: { coins: 350, potions: 4, label: "+350 Coins & 4 Luck Potions" },
};

function rarityScore(r: string): number {
  const idx = RARITY_ORDER.indexOf(r as (typeof RARITY_ORDER)[number]);
  return idx >= 0 ? idx + 1 : 1;
}

function cardStats(card: CardAssetRecord, owned: OwnedCard, rebirths: number) {
  const foilM = FOIL_MULTIPLIER[owned.foil].mult;
  const starM = 1 + (owned.stars - 1) * 0.35;
  const rebM = 1 + rebirths * 0.5;
  const mult = foilM * starM * rebM;
  return {
    atk: Math.round(card.atk * mult),
    hp: Math.round(card.hp * mult),
    speed: Math.round(card.speed * (1 + (owned.stars - 1) * 0.08)),
    power: Math.round((card.atk * 2.2 + card.hp * 0.45 + card.speed * 2) * mult),
  };
}

function computeSynergies(deckCards: CardAssetRecord[]) {
  const counts: Record<CardElement, number> = { Flame: 0, Void: 0, Storm: 0, Shadow: 0, Arcane: 0 };
  for (const c of deckCards) counts[c.element]++;
  const unique = Object.values(counts).filter((n) => n > 0).length;
  const badges: Array<{ label: string; atkBonus: number; critBonus: number; shieldBonus: number; cls: string }> = [];

  if (counts.Flame >= 2) {
    badges.push({ label: `Flame x${counts.Flame}: +20% ATK`, atkBonus: 0.2, critBonus: 0, shieldBonus: 0, cls: ELEMENT_BADGE_CLS.Flame });
  }
  if (counts.Void >= 2) {
    badges.push({ label: `Void x${counts.Void}: +25% Crit`, atkBonus: 0, critBonus: 0.25, shieldBonus: 0, cls: ELEMENT_BADGE_CLS.Void });
  }
  if (counts.Storm >= 2) {
    badges.push({ label: `Storm x${counts.Storm}: +18% Burst`, atkBonus: 0.12, critBonus: 0.1, shieldBonus: 0, cls: ELEMENT_BADGE_CLS.Storm });
  }
  if (counts.Shadow >= 2) {
    badges.push({ label: `Shadow x${counts.Shadow}: +15% Drain`, atkBonus: 0.15, critBonus: 0, shieldBonus: 0.1, cls: ELEMENT_BADGE_CLS.Shadow });
  }
  if (counts.Arcane >= 2) {
    badges.push({ label: `Arcane x${counts.Arcane}: +25% Shield`, atkBonus: 0, critBonus: 0, shieldBonus: 0.25, cls: ELEMENT_BADGE_CLS.Arcane });
  }
  if (unique >= 3) {
    badges.push({ label: `Prismatic ${unique}: +15% All`, atkBonus: 0.15, critBonus: 0.1, shieldBonus: 0.15, cls: "bg-tag-design-bg text-tag-design-fg" });
  }
  return badges;
}

function bossForFloor(floor: number, impossible: boolean): TowerBossDef {
  if (impossible) {
    const imps = TOWER_BOSSES.filter((b) => b.impossible);
    const pick = imps[Math.min(imps.length - 1, Math.floor((floor - 1) / 5))];
    if (pick) return pick;
  }
  const exact = TOWER_BOSSES.find((b) => b.floor === floor);
  if (exact) return exact;
  const scale = 1 + (floor - 1) * 0.28;
  const base = TOWER_BOSSES[(floor - 1) % TOWER_BOSSES.length];
  return {
    ...base,
    floor,
    title: floor % 5 === 0 ? `Floor ${floor} Boss Raid` : `Floor ${floor} Guardian`,
    hp: Math.round(1500 * scale),
    atk: Math.round(175 * (1 + (floor - 1) * 0.22)),
    rewardCoins: Math.round(240 * scale),
    impossible: floor % 5 === 0,
  };
}

function rollFoil(luckMult: number): FoilVariant {
  const r = Math.random() / Math.max(1, Math.sqrt(luckMult));
  if (r < 0.015) return "Void";
  if (r < 0.06) return "Rainbow";
  if (r < 0.16) return "Gold";
  if (r < 0.34) return "Shiny";
  return "Normal";
}

function rollCardFromPool(pool: CardAssetRecord[], luckMult: number, pityHit: boolean): CardAssetRecord {
  if (pool.length === 0) throw new Error("empty pool");
  const sorted = [...pool].sort((a, b) => rarityScore(b.rarity) - rarityScore(a.rarity));
  if (pityHit) {
    const high = sorted.filter((c) => rarityScore(c.rarity) >= 5);
    if (high.length) return high[Math.floor(Math.random() * high.length)];
  }
  // Weighted draw shifted upward by luck multiplier
  const weights = sorted.map((c) => {
    const tier = rarityScore(c.rarity);
    const base = Math.pow(1.9, 9 - tier);
    const boost = tier >= 5 ? luckMult : 1;
    return base * boost;
  });
  const sum = weights.reduce((a, b) => a + b, 0);
  let pick = Math.random() * sum;
  for (let i = 0; i < sorted.length; i++) {
    pick -= weights[i];
    if (pick <= 0) return sorted[i];
  }
  return sorted[sorted.length - 1];
}

export function CardBattlerStage({
  assets,
  onOpenArtStudio,
}: {
  assets: CardAssetRecord[];
  onOpenArtStudio?: () => void;
}) {
  const reduce = useReducedMotion();
  const cardMap = useMemo(() => new Map(assets.map((c) => [c.card_id, c])), [assets]);

  const [tab, setTab] = useState<BattlerTab>("roll");
  const [coins, setCoins] = useState(450);
  const [potions, setPotions] = useState(2);
  const [potionRollsLeft, setPotionRollsLeft] = useState(0);
  const [weatherIdx, setWeatherIdx] = useState(0);
  const [pity, setPity] = useState(0);
  const [rebirths, setRebirths] = useState(0);
  const [towerFloor, setTowerFloor] = useState(1);
  const [wins, setWins] = useState(0);
  const [autoRoll, setAutoRoll] = useState(false);
  const [autoBattle, setAutoBattle] = useState(false);
  const [codeText, setCodeText] = useState("");
  const [usedCodes, setUsedCodes] = useState<string[]>([]);
  const [lastPulls, setLastPulls] = useState<Array<{ card: CardAssetRecord; foil: FoilVariant; isNew: boolean }>>([]);

  // Seed initial starter collection of 4 real Nano Banana cards so players can battle immediately OR roll for upgrades!
  const [collection, setCollection] = useState<OwnedCard[]>(() => [
    { uid: "start-1", card_id: "eclipse-arcane-alchemist", foil: "Normal", stars: 1, copies: 2 },
    { uid: "start-2", card_id: "blood-moon-gravity-ronin", foil: "Normal", stars: 1, copies: 1 },
    { uid: "start-3", card_id: "thunder-flash-ronin", foil: "Shiny", stars: 1, copies: 1 },
    { uid: "start-4", card_id: "crimson-sea-captain", foil: "Normal", stars: 1, copies: 1 },
  ]);
  const [deckIds, setDeckIds] = useState<string[]>(["start-4", "start-3", "start-2", "start-1"]);

  const weather = WEATHER_PROFILES[weatherIdx % WEATHER_PROFILES.length];
  const luckMult = useMemo(() => {
    const rebBonus = 1 + rebirths * 0.5;
    const potBonus = potionRollsLeft > 0 ? 2.0 : 1.0;
    return Number((weather.luckMult * rebBonus * potBonus).toFixed(2));
  }, [potionRollsLeft, rebirths, weather.luckMult]);

  const deckEntries = useMemo(
    () =>
      deckIds
        .map((uid) => {
          const owned = collection.find((c) => c.uid === uid);
          const meta = owned ? cardMap.get(owned.card_id) : undefined;
          return owned && meta ? { owned, meta, stats: cardStats(meta, owned, rebirths) } : null;
        })
        .filter((x): x is NonNullable<typeof x> => Boolean(x)),
    [cardMap, collection, deckIds, rebirths],
  );

  const synergies = useMemo(() => computeSynergies(deckEntries.map((d) => d.meta)), [deckEntries]);
  const totalDeckAtk = useMemo(() => {
    const raw = deckEntries.reduce((s, d) => s + d.stats.atk, 0);
    const synBonus = synergies.reduce((s, b) => s + b.atkBonus, 0);
    return Math.round(raw * (1 + synBonus));
  }, [deckEntries, synergies]);
  const totalDeckHp = useMemo(() => Math.max(800, deckEntries.reduce((s, d) => s + d.stats.hp, 0)), [deckEntries]);
  const totalDeckPower = useMemo(() => deckEntries.reduce((s, d) => s + d.stats.power, 0), [deckEntries]);

  const evIdRef = useRef(1);
  const makeBattle = useCallback(
    (floor: number, impossible: boolean): BattleState => {
      const boss = bossForFloor(floor, impossible);
      return {
        floor,
        impossible,
        boss,
        bossHp: boss.hp,
        bossMaxHp: boss.hp,
        bossPhase: 1,
        bossStunned: false,
        bossBurning: false,
        bossIntent: "strike",
        teamHp: totalDeckHp,
        teamMaxHp: totalDeckHp,
        teamShield: Math.round(totalDeckHp * 0.12),
        burstGauge: 40,
        turn: 1,
        outcome: "fighting",
        events: [
          {
            id: evIdRef.current++,
            text: `Floor ${floor} — ${boss.name} (${boss.element}) enters! Squad Power ${totalDeckPower.toLocaleString()}.`,
            tone: "info",
          },
        ],
      };
    },
    [totalDeckHp, totalDeckPower],
  );

  const [battle, setBattle] = useState<BattleState>(() => makeBattle(1, false));

  // Roll handler (1x or 5x multi)
  const performRoll = useCallback(
    (count: number) => {
      if (assets.length === 0) return;
      const pulls: Array<{ card: CardAssetRecord; foil: FoilVariant; isNew: boolean }> = [];
      let curPity = pity;
      let curPot = potionRollsLeft;

      setCollection((prev) => {
        const next = prev.map((x) => ({ ...x }));
        for (let i = 0; i < count; i++) {
          curPity++;
          const hitPity = curPity >= 10;
          if (hitPity) curPity = 0;
          const activeLuck = weather.luckMult * (1 + rebirths * 0.5) * (curPot > 0 ? 2.0 : 1.0);
          if (curPot > 0) curPot--;
          const card = rollCardFromPool(assets, activeLuck, hitPity);
          const foil = rollFoil(activeLuck);
          const existing = next.find((o) => o.card_id === card.card_id && o.foil === foil);
          if (existing) {
            existing.copies += 1;
            pulls.push({ card, foil, isNew: false });
          } else {
            next.push({
              uid: `card-${Date.now()}-${Math.random().toString(36).slice(2, 6)}-${i}`,
              card_id: card.card_id,
              foil,
              stars: 1,
              copies: 1,
            });
            pulls.push({ card, foil, isNew: true });
          }
        }
        return next;
      });

      setPity(curPity);
      setPotionRollsLeft(curPot);
      setLastPulls(pulls);

      const best = [...pulls].sort((a, b) => rarityScore(b.card.rarity) - rarityScore(a.card.rarity))[0];
      if (best && (best.card.rarity === "Secret" || best.card.rarity === "Celestial")) {
        setAutoRoll(false);
        toast.success(`Pulled ${best.foil !== "Normal" ? `${best.foil} ` : ""}${best.card.title} (${best.card.rarity})!`);
      }
    },
    [assets, pity, potionRollsLeft, rebirths, weather.luckMult],
  );

  // Auto-Roll loop
  useEffect(() => {
    if (!autoRoll || tab !== "roll") return;
    const id = window.setInterval(() => performRoll(1), 650);
    return () => window.clearInterval(id);
  }, [autoRoll, performRoll, tab]);

  const activatePotion = () => {
    if (potions <= 0) {
      if (coins >= 150) {
        setCoins((c) => c - 150);
        setPotionRollsLeft((r) => r + 5);
        toast.success("Crafted & drank Luck Potion (2.0x Luck for 5 rolls)");
      } else {
        toast.error("Win a Tower battle or redeem code LUCKBOOST for more Luck Potions.");
      }
      return;
    }
    setPotions((p) => p - 1);
    setPotionRollsLeft((r) => r + 5);
    toast.success("Luck Potion active: 2.0x Roll Luck for 5 rolls");
  };

  const redeemCode = () => {
    const clean = codeText.trim().toUpperCase();
    if (!clean) return;
    if (usedCodes.includes(clean)) {
      toast.error(`Code ${clean} was already redeemed.`);
      return;
    }
    const reward = PROMO_CODES[clean];
    if (!reward) {
      toast.error("Unknown code. Try REBIRTH, APEX2026, or LUCKBOOST.");
      return;
    }
    setCoins((c) => c + reward.coins);
    setPotions((p) => p + reward.potions);
    setUsedCodes((u) => [...u, clean]);
    setCodeText("");
    toast.success(`Redeemed ${clean}: ${reward.label}`);
  };

  const toggleEquip = (uid: string) => {
    setDeckIds((prev) => {
      if (prev.includes(uid)) {
        if (prev.length <= 1) {
          toast.error("Keep at least 1 card in your battle deck.");
          return prev;
        }
        return prev.filter((id) => id !== uid);
      }
      if (prev.length >= 4) {
        return [...prev.slice(0, 3), uid];
      }
      return [...prev, uid];
    });
  };

  const autoEquipBest = () => {
    const ranked = [...collection]
      .map((o) => {
        const m = cardMap.get(o.card_id);
        return m ? { uid: o.uid, power: cardStats(m, o, rebirths).power } : null;
      })
      .filter((x): x is NonNullable<typeof x> => Boolean(x))
      .sort((a, b) => b.power - a.power)
      .slice(0, 4)
      .map((x) => x.uid);
    if (ranked.length) {
      setDeckIds(ranked);
      toast.success("Equipped top 4 highest-power cards");
    }
  };

  const fuseCard = (uid: string) => {
    const target = collection.find((c) => c.uid === uid);
    if (!target || target.copies < 2 || target.stars >= 5) return;
    setCollection((prev) =>
      prev.map((c) => (c.uid === uid ? { ...c, copies: c.copies - 1, stars: Math.min(5, c.stars + 1) } : c)),
    );
    const meta = cardMap.get(target.card_id);
    toast.success(`Star Fused ${meta?.title || "Card"} to ★${target.stars + 1} (+35% stats)`);
  };

  const triggerRebirth = () => {
    if (towerFloor < 2 && collection.length < 5) {
      toast.error("Clear Floor 1 or collect 5 cards to unlock Rebirth Ascension.");
      return;
    }
    setRebirths((r) => r + 1);
    setPotions((p) => p + 2);
    setCoins((c) => c + 400);
    toast.success(`Rebirth Ascension R${rebirths + 1}! Permanent +50% Luck & Squad Power unlocked.`);
  };

  // Tactical Turn Combat Engine
  const executeTurn = useCallback(
    (action: "strike" | "counter" | "ultimate") => {
      setBattle((prev) => {
        if (prev.outcome !== "fighting") return prev;
        const lead = deckEntries[0];
        const leadName = lead?.meta.title || "Squad";
        const leadUlt = lead?.meta.ultimate || "Apex Burst";
        const critChance = 0.22 + synergies.reduce((s, b) => s + b.critBonus, 0);
        const weatherHit = deckEntries.some((d) => d.meta.element === weather.boostedElement)
          ? 1 + weather.elementBonusPct / 100
          : 1;

        let bossHp = prev.bossHp;
        let bossPhase = prev.bossPhase;
        let bossStunned = prev.bossStunned;
        let bossBurning = prev.bossBurning;
        let teamHp = prev.teamHp;
        let teamShield = prev.teamShield;
        let burstGauge = prev.burstGauge;
        const events = [...prev.events];

        const pushEv = (text: string, tone: CombatEvent["tone"]) => {
          events.unshift({ id: evIdRef.current++, text, tone });
        };

        if (action === "ultimate" && burstGauge >= 100) {
          const isCrit = Math.random() < critChance + 0.25;
          const dmg = Math.round(totalDeckAtk * 2.4 * weatherHit * (isCrit ? 1.5 : 1));
          bossHp = Math.max(0, bossHp - dmg);
          burstGauge = 0;
          bossBurning = true;
          teamHp = Math.min(prev.teamMaxHp, teamHp + Math.round(prev.teamMaxHp * 0.1));
          pushEv(
            `${leadName} unleashed ${leadUlt} for ${dmg.toLocaleString()} DMG${isCrit ? " (CRIT!)" : ""}!`,
            "ult",
          );
        } else if (action === "counter") {
          const shieldGain = Math.round(totalDeckHp * (0.22 + synergies.reduce((s, b) => s + b.shieldBonus, 0)));
          teamShield += shieldGain;
          burstGauge = Math.min(100, burstGauge + 28);
          if (prev.bossIntent === "ultimate") {
            const reflect = Math.round(prev.boss.atk * 1.45);
            bossHp = Math.max(0, bossHp - reflect);
            bossStunned = true;
            pushEv(
              `Perfect Counter Shield (+${shieldGain.toLocaleString()} Shield)! Reflected ${reflect.toLocaleString()} DMG & STUNNED ${prev.boss.name}!`,
              "shield",
            );
          } else {
            pushEv(`Raised Counter Shield (+${shieldGain.toLocaleString()} Shield) & charged Burst +28%.`, "shield");
          }
        } else {
          const isCrit = Math.random() < critChance;
          const dmg = Math.round(totalDeckAtk * 0.92 * weatherHit * (isCrit ? 1.45 : 1));
          bossHp = Math.max(0, bossHp - dmg);
          burstGauge = Math.min(100, burstGauge + 34);
          pushEv(
            `Squad Strike hit ${prev.boss.name} for ${dmg.toLocaleString()} DMG${isCrit ? " (CRIT!)" : ""}.`,
            isCrit ? "crit" : "info",
          );
        }

        if (bossBurning && bossHp > 0) {
          const burn = Math.round(prev.bossMaxHp * 0.04);
          bossHp = Math.max(0, bossHp - burn);
        }

        // Boss Phase II transition for Impossible Bosses
        if (prev.boss.impossible && bossPhase === 1 && bossHp > 0 && bossHp <= prev.bossMaxHp * 0.5) {
          bossPhase = 2;
          pushEv(`${prev.boss.name} entered Phase II! Enraged aura +25% ATK!`, "boss");
        }

        if (bossHp <= 0) {
          setCoins((c) => c + prev.boss.rewardCoins);
          setPotions((p) => p + 1);
          setWins((w) => w + 1);
          if (!prev.impossible && prev.floor >= towerFloor && towerFloor < 50) {
            setTowerFloor((f) => Math.min(50, f + 1));
          }
          pushEv(
            `VICTORY! Defeated ${prev.boss.name} — earned +${prev.boss.rewardCoins} Coins & +1 Luck Potion!`,
            "win",
          );
          return {
            ...prev,
            bossHp: 0,
            bossPhase,
            teamHp,
            teamShield,
            burstGauge,
            outcome: "victory",
            events: events.slice(0, 6),
          };
        }

        // Boss counter-turn
        if (bossStunned) {
          bossStunned = false;
          pushEv(`${prev.boss.name} is stunned and skips its turn!`, "shield");
        } else {
          const mult = (prev.bossIntent === "ultimate" ? 1.55 : 1.0) * (bossPhase === 2 ? 1.25 : 1.0);
          let incoming = Math.round(prev.boss.atk * mult);
          if (teamShield > 0) {
            const absorbed = Math.min(teamShield, incoming);
            teamShield -= absorbed;
            incoming -= absorbed;
          }
          teamHp = Math.max(0, teamHp - incoming);
          pushEv(
            `${prev.boss.name} used ${prev.bossIntent === "ultimate" ? "Apex Cataclysm" : "Elemental Slash"} (${incoming.toLocaleString()} HP dmg).`,
            "boss",
          );
        }

        const nextIntent: "strike" | "ultimate" = prev.turn % 2 === 0 ? "ultimate" : "strike";
        const outcome: BattleState["outcome"] = teamHp <= 0 ? "defeat" : "fighting";
        if (outcome === "defeat") {
          pushEv("Squad defeated — fuse cards, adjust deck synergies, or retry!", "boss");
        }

        return {
          ...prev,
          bossHp,
          bossPhase,
          bossStunned,
          bossBurning,
          bossIntent: nextIntent,
          teamHp,
          teamShield,
          burstGauge,
          turn: prev.turn + 1,
          outcome,
          events: events.slice(0, 6),
        };
      });
    },
    [deckEntries, synergies, totalDeckAtk, totalDeckHp, towerFloor, weather.boostedElement, weather.elementBonusPct],
  );

  // Auto-Battle loop
  useEffect(() => {
    if (!autoBattle || tab !== "arena" || battle.outcome !== "fighting") return;
    const id = window.setInterval(() => {
      if (battle.burstGauge >= 100) executeTurn("ultimate");
      else if (battle.bossIntent === "ultimate") executeTurn("counter");
      else executeTurn("strike");
    }, 850);
    return () => window.clearInterval(id);
  }, [autoBattle, battle.bossIntent, battle.burstGauge, battle.outcome, executeTurn, tab]);

  const bossCardArt = cardMap.get(battle.boss.cardArtId)?.image_url || assets[0]?.image_url;
  const featuredPull = lastPulls[0] || (assets[0] ? { card: assets[0], foil: "Shiny" as FoilVariant, isNew: false } : null);

  return (
    <div data-card-battler-stage className="flex flex-col gap-3">
      {/* Live Server Weather, Luck, Tower & Economy HUD */}
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-board/55 px-3 py-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <Pill className={`px-2 py-0.5 text-xs font-medium ${weather.badgeCls}`}>
            <CloudLightning className="size-3" strokeWidth={1.5} aria-hidden />
            <span>{weather.name} ({weather.luckMult}x)</span>
          </Pill>
          <Pill className="bg-tag-dev-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-dev-fg">
            <Coins className="size-3" strokeWidth={1.5} aria-hidden />
            {coins.toLocaleString()} Coins
          </Pill>
          <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink">
            <Trophy className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
            Floor {towerFloor}/50
          </Pill>
          <Pill className="bg-tag-research-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-research-fg">
            <Sparkles className="size-3" strokeWidth={1.5} aria-hidden />
            Luck {luckMult}x · R{rebirths}
          </Pill>
          <Pill className="bg-surface px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
            Pity {pity}/10
          </Pill>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          <Button
            variant="outline"
            size="xs"
            onClick={() => setWeatherIdx((w) => (w + 1) % WEATHER_PROFILES.length)}
            aria-label="Cycle server weather"
            className="bg-surface max-md:min-h-11"
          >
            <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />
            Weather
          </Button>
          {onOpenArtStudio && (
            <Button
              variant="outline"
              size="xs"
              onClick={onOpenArtStudio}
              aria-label="Open Card Art Studio"
              className="bg-surface max-md:min-h-11"
            >
              <Palette className="size-3" strokeWidth={1.5} aria-hidden />
              Card Art ({assets.length})
            </Button>
          )}
        </div>
      </div>

      {/* Mode Switcher */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <SegmentedControl aria-label="Anime Card Battles modes" size="sm">
          <SegmentedControlItem
            data-checked={tab === "roll" ? "" : undefined}
            aria-pressed={tab === "roll"}
            onClick={() => setTab("roll")}
            className="gap-1.5 max-md:!h-11 max-md:px-2.5"
          >
            <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
            Roll Pack
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={tab === "deck" ? "" : undefined}
            aria-pressed={tab === "deck"}
            onClick={() => setTab("deck")}
            className="gap-1.5 max-md:!h-11 max-md:px-2.5"
          >
            <Layers className="size-3.5" strokeWidth={1.5} aria-hidden />
            Deck &amp; Fusion ({collection.length})
          </SegmentedControlItem>
          <SegmentedControlItem
            data-checked={tab === "arena" ? "" : undefined}
            aria-pressed={tab === "arena"}
            onClick={() => {
              setTab("arena");
              setBattle((b) => (b.outcome === "fighting" ? b : makeBattle(towerFloor, false)));
            }}
            className="gap-1.5 max-md:!h-11 max-md:px-2.5"
          >
            <Swords className="size-3.5" strokeWidth={1.5} aria-hidden />
            Boss &amp; Tower Arena
          </SegmentedControlItem>
        </SegmentedControl>

        <span className="font-mono text-xs tabular-nums text-ink-2">
          Deck Power {totalDeckPower.toLocaleString()} · {wins} {wins === 1 ? "win" : "wins"}
        </span>
      </div>

      {/* TAB 1: ROLL PACK & AUTO-ROLL */}
      {tab === "roll" && (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-[220px_1fr]">
          {featuredPull && (
            <motion.div
              key={`${featuredPull.card.card_id}-${featuredPull.foil}-${lastPulls.length}`}
              initial={reduce ? false : { opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              transition={reduce ? instant : reveal}
              className={`flex flex-col overflow-hidden rounded-card border-2 bg-surface ${
                featuredPull.foil === "Void" || featuredPull.foil === "Rainbow"
                  ? "border-blue"
                  : "border-hairline"
              }`}
            >
              <div className="relative aspect-[3/4] w-full overflow-hidden bg-board">
                <img
                  src={featuredPull.card.image_url}
                  alt={featuredPull.card.title}
                  className="h-full w-full object-cover"
                />
              </div>
              <div className="flex flex-col gap-1 p-2.5">
                <div className="flex flex-wrap items-center justify-between gap-1">
                  <Badge className={`border-transparent px-1.5 py-0 text-xs font-medium ${RARITY_BADGE_CLS[featuredPull.card.rarity]}`}>
                    {featuredPull.card.rarity}
                  </Badge>
                  <Badge className={`border-transparent px-1.5 py-0 text-xs font-medium ${FOIL_MULTIPLIER[featuredPull.foil].cls}`}>
                    {featuredPull.foil} {FOIL_MULTIPLIER[featuredPull.foil].mult}x
                  </Badge>
                </div>
                <div className="truncate text-sm font-medium text-ink">{featuredPull.card.title}</div>
                <div className="flex items-center justify-between font-mono text-xs tabular-nums text-ink-2">
                  <span>ATK {featuredPull.card.atk}</span>
                  <span>HP {featuredPull.card.hp}</span>
                  <span>{featuredPull.card.odds}</span>
                </div>
              </div>
            </motion.div>
          )}

          <div className="flex flex-col justify-between gap-3 rounded-card border border-hairline bg-board/40 p-3.5">
            <div className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="text-sm font-medium text-ink">Torii Summon Shrine</span>
                  <Pill className="bg-tag-design-bg px-2 py-0.5 text-xs text-tag-design-fg">
                    Trait: {CARD_TRAITS[(lastPulls.length + weatherIdx) % CARD_TRAITS.length]}
                  </Pill>
                </div>
                <span className="text-xs text-ink-2">{weather.desc}</span>
              </div>

              {featuredPull && (
                <div className="rounded-sm border border-hairline bg-surface p-2.5 text-xs">
                  <div className="font-medium text-ink">{featuredPull.card.passive}</div>
                  <div className="mt-1 text-ink-2">Ultimate: {featuredPull.card.ultimate}</div>
                </div>
              )}

              {lastPulls.length > 1 && (
                <div className="flex flex-wrap gap-1.5">
                  {lastPulls.map((p, idx) => (
                    <Pill key={idx} className={`px-2 py-0.5 text-xs ${RARITY_BADGE_CLS[p.card.rarity]}`}>
                      {p.foil !== "Normal" ? `${p.foil} ` : ""}
                      {p.card.title}
                    </Pill>
                  ))}
                </div>
              )}
            </div>

            <div className="flex flex-col gap-2.5">
              <div className="flex flex-wrap items-center gap-1.5">
                <Button
                  variant="default"
                  size="sm"
                  onClick={() => performRoll(1)}
                  className="max-md:min-h-11"
                >
                  <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Roll x1
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => performRoll(5)}
                  className="bg-surface max-md:min-h-11"
                >
                  Roll x5
                </Button>
                <Button
                  variant={autoRoll ? "secondary" : "outline"}
                  size="sm"
                  aria-pressed={autoRoll}
                  onClick={() => setAutoRoll((v) => !v)}
                  className="max-md:min-h-11"
                >
                  {autoRoll ? <Square className="size-3.5" strokeWidth={1.5} aria-hidden /> : <Play className="size-3.5" strokeWidth={1.5} aria-hidden />}
                  {autoRoll ? "Stop Auto-Roll" : "Auto-Roll"}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={activatePotion}
                  className="bg-surface max-md:min-h-11"
                >
                  <Flame className="size-3.5" strokeWidth={1.5} aria-hidden />
                  {potionRollsLeft > 0 ? `Luck 2x (${potionRollsLeft})` : `Luck Potion (${potions})`}
                </Button>
              </div>

              <div className="flex flex-wrap items-center gap-1.5 border-t border-hairline pt-2.5">
                <Input
                  value={codeText}
                  onChange={(e) => setCodeText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      redeemCode();
                    }
                  }}
                  aria-label="Redeem promo code"
                  placeholder="Code: REBIRTH, APEX2026, LUCKBOOST"
                  className="h-8 flex-1 bg-surface font-mono text-xs"
                />
                <Button variant="outline" size="xs" onClick={redeemCode} className="bg-surface max-md:min-h-11">
                  <Gift className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Redeem
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* TAB 2: DECK, SYNERGIES, STAR FUSION & REBIRTH */}
      {tab === "deck" && (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-board/40 p-3">
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="text-xs font-medium text-ink">Active Synergies:</span>
              {synergies.length === 0 ? (
                <span className="text-xs text-ink-2">Equip 2+ matching elements or 3+ unique elements</span>
              ) : (
                synergies.map((s) => (
                  <Pill key={s.label} className={`px-2 py-0.5 text-xs ${s.cls}`}>
                    {s.label}
                  </Pill>
                ))
              )}
            </div>
            <div className="flex flex-wrap items-center gap-1.5">
              <Button variant="outline" size="xs" onClick={autoEquipBest} className="bg-surface max-md:min-h-11">
                <Zap className="size-3.5" strokeWidth={1.5} aria-hidden />
                Auto-Equip Best
              </Button>
              <Button variant="default" size="xs" onClick={triggerRebirth} className="max-md:min-h-11">
                <Crown className="size-3.5" strokeWidth={1.5} aria-hidden />
                Rebirth (R{rebirths} → R{rebirths + 1})
              </Button>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-4">
            {collection.map((owned) => {
              const meta = cardMap.get(owned.card_id);
              if (!meta) return null;
              const stats = cardStats(meta, owned, rebirths);
              const equipped = deckIds.includes(owned.uid);
              const canFuse = owned.copies >= 2 && owned.stars < 5;
              return (
                <motion.div
                  key={owned.uid}
                  whileTap={reduce ? undefined : { scale: 0.985 }}
                  transition={reduce ? instant : press}
                  className={`flex flex-col overflow-hidden rounded-card border bg-surface ${
                    equipped ? "border-blue ring-1 ring-blue/35" : "border-hairline"
                  }`}
                >
                  <div className="flex items-center gap-2.5 p-2.5">
                    <img
                      src={meta.image_url}
                      alt={meta.title}
                      className="h-16 w-12 flex-none rounded-sm border border-hairline object-cover"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-1">
                        <Badge className={`border-transparent px-1.5 py-0 text-xs ${RARITY_BADGE_CLS[meta.rarity]}`}>
                          {meta.rarity}
                        </Badge>
                        {owned.foil !== "Normal" && (
                          <Badge className={`border-transparent px-1.5 py-0 text-xs ${FOIL_MULTIPLIER[owned.foil].cls}`}>
                            {owned.foil}
                          </Badge>
                        )}
                      </div>
                      <div className="mt-0.5 truncate text-xs font-medium text-ink">{meta.title}</div>
                      <div className="font-mono text-xs tabular-nums text-ink-2">
                        ★{owned.stars} · x{owned.copies} · {stats.atk} ATK
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5 border-t border-hairline bg-board/40 p-2">
                    <Button
                      variant={equipped ? "default" : "outline"}
                      size="xs"
                      onClick={() => toggleEquip(owned.uid)}
                      className="flex-1 justify-center max-md:min-h-11"
                    >
                      {equipped ? "Equipped" : "Equip"}
                    </Button>
                    {canFuse && (
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={() => fuseCard(owned.uid)}
                        className="bg-surface max-md:min-h-11"
                      >
                        Fuse ★{owned.stars + 1}
                      </Button>
                    )}
                  </div>
                </motion.div>
              );
            })}
          </div>
        </div>
      )}

      {/* TAB 3: TACTICAL BOSS & 50-FLOOR TOWER ARENA */}
      {tab === "arena" && (
        <div className="flex flex-col gap-3 rounded-card border border-hairline bg-board/40 p-3.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge className={`border-transparent px-2 py-0.5 text-xs font-medium ${ELEMENT_BADGE_CLS[battle.boss.element]}`}>
                {battle.boss.title}
              </Badge>
              <span className="text-sm font-medium text-ink">{battle.boss.name}</span>
              {battle.bossPhase === 2 && (
                <Pill className="bg-pill-pro-bg px-2 py-0.5 text-xs text-pill-pro-fg">Phase II Enraged</Pill>
              )}
              {battle.bossStunned && (
                <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">Stunned</Pill>
              )}
              {battle.bossBurning && (
                <Pill className="bg-tag-plan-bg px-2 py-0.5 text-xs text-tag-plan-fg">Burn</Pill>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-1.5">
              <Button
                variant={battle.impossible ? "default" : "outline"}
                size="xs"
                onClick={() => {
                  setAutoBattle(false);
                  setBattle(makeBattle(Math.max(5, towerFloor), !battle.impossible));
                }}
                className="max-md:min-h-11"
              >
                <Crown className="size-3.5" strokeWidth={1.5} aria-hidden />
                {battle.impossible ? "Impossible Boss" : "Challenge Boss"}
              </Button>
              <Button
                variant="outline"
                size="xs"
                onClick={() => {
                  setAutoBattle(false);
                  setBattle(makeBattle(towerFloor, battle.impossible));
                }}
                className="bg-surface max-md:min-h-11"
              >
                <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                {battle.outcome === "victory" ? `Next Floor (${towerFloor})` : "Reset"}
              </Button>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {/* Player Squad Panel (4v4 Clash Formation with Nano Banana Portraits & Live Skill Popup) */}
            <div className="flex flex-col gap-2 rounded-card border border-hairline bg-surface p-3">
              <div className="flex flex-wrap items-center justify-between gap-1.5 text-xs">
                <div className="flex items-center gap-1.5">
                  <span className="font-medium text-ink">4v4 Clash Squad ({deckEntries.length}/4)</span>
                  {battle.events[0] && (
                    <Pill
                      data-skill-popup
                      className="bg-tag-design-bg px-1.5 py-0 font-mono text-xs text-tag-design-fg"
                    >
                      {SKILL_POPUPS[battle.turn % SKILL_POPUPS.length]}
                    </Pill>
                  )}
                </div>
                <span className="font-mono tabular-nums text-ink-2">
                  HP {battle.teamHp.toLocaleString()} / {battle.teamMaxHp.toLocaleString()}
                  {battle.teamShield > 0 ? ` (+${battle.teamShield.toLocaleString()} Shield)` : ""}
                </span>
              </div>
              <div className="h-2.5 w-full overflow-hidden rounded-full bg-track">
                <div
                  className="h-full w-full origin-left bg-dot-green transition-transform duration-(--dur-swap) ease-(--ease-house)"
                  style={{ transform: `scaleX(${Math.max(0, Math.min(1, battle.teamHp / Math.max(1, battle.teamMaxHp)))})` }}
                />
              </div>
              <div className="grid grid-cols-4 gap-1.5 pt-1">
                {deckEntries.map(({ owned, meta }) => (
                  <div key={owned.uid} className="flex flex-col items-center gap-1">
                    <img
                      src={meta.image_url}
                      alt={meta.title}
                      className="aspect-[3/4] w-full rounded-sm border border-hairline object-cover"
                    />
                    <span className="w-full truncate text-center font-mono text-xs text-ink-2">
                      ★{owned.stars} {meta.element}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Boss Panel */}
            <div className="flex flex-col gap-2 rounded-card border border-hairline bg-surface p-3">
              <div className="flex items-center justify-between text-xs">
                <span className="font-medium text-ink">{battle.boss.name}</span>
                <span className="font-mono tabular-nums text-ink-2">
                  HP {battle.bossHp.toLocaleString()} / {battle.bossMaxHp.toLocaleString()}
                </span>
              </div>
              <div className="h-2.5 w-full overflow-hidden rounded-full bg-track">
                <div
                  className="h-full w-full origin-left bg-dot-red transition-transform duration-(--dur-swap) ease-(--ease-house)"
                  style={{ transform: `scaleX(${Math.max(0, Math.min(1, battle.bossHp / Math.max(1, battle.bossMaxHp)))})` }}
                />
              </div>
              <div className="flex items-center gap-3 pt-1">
                {bossCardArt && (
                  <img
                    src={bossCardArt}
                    alt={battle.boss.name}
                    className="h-24 w-18 flex-none rounded-sm border border-hairline object-cover"
                  />
                )}
                <div className="flex min-w-0 flex-1 flex-col gap-1.5 text-xs">
                  <div className="flex items-center justify-between">
                    <span className="text-ink-2">Next Boss Move:</span>
                    <Pill
                      className={`px-2 py-0.5 text-xs ${
                        battle.bossIntent === "ultimate"
                          ? "bg-pill-pro-bg text-pill-pro-fg"
                          : "bg-board text-ink"
                      }`}
                    >
                      {battle.bossIntent === "ultimate" ? "Charging Ultimate! (Counter!)" : "Elemental Slash"}
                    </Pill>
                  </div>
                  <div className="flex items-center gap-2 pt-1">
                    <ProgressCircle value={battle.burstGauge} radius={16} strokeWidth={3.5} className="size-9 flex-none">
                      <span className="font-mono text-xs tabular-nums text-ink">{battle.burstGauge}</span>
                    </ProgressCircle>
                    <div className="min-w-0">
                      <div className="font-medium text-ink">Ultimate Burst Gauge</div>
                      <div className="text-ink-2">
                        {battle.burstGauge >= 100 ? "Ready to unleash!" : "Strike or Counter to reach 100"}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {/* Tactical Action Bar */}
          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline pt-2.5">
            <div className="flex flex-wrap items-center gap-1.5">
              <Button
                variant="outline"
                size="sm"
                disabled={battle.outcome !== "fighting"}
                onClick={() => executeTurn("strike")}
                className="bg-surface max-md:min-h-11"
              >
                <Swords className="size-3.5" strokeWidth={1.5} aria-hidden />
                Strike
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={battle.outcome !== "fighting"}
                onClick={() => executeTurn("counter")}
                className="bg-surface max-md:min-h-11"
              >
                <Shield className="size-3.5" strokeWidth={1.5} aria-hidden />
                Counter Shield
              </Button>
              <Button
                variant="default"
                size="sm"
                disabled={battle.outcome !== "fighting" || battle.burstGauge < 100}
                onClick={() => executeTurn("ultimate")}
                className="max-md:min-h-11"
              >
                <Zap className="size-3.5" strokeWidth={1.5} aria-hidden />
                Ultimate Burst
              </Button>
            </div>

            <Button
              variant={autoBattle ? "secondary" : "outline"}
              size="sm"
              aria-pressed={autoBattle}
              disabled={battle.outcome !== "fighting"}
              onClick={() => setAutoBattle((v) => !v)}
              className="max-md:min-h-11"
            >
              {autoBattle ? <Square className="size-3.5" strokeWidth={1.5} aria-hidden /> : <Play className="size-3.5" strokeWidth={1.5} aria-hidden />}
              {autoBattle ? "Stop Auto" : "Auto-Battle"}
            </Button>
          </div>

          {/* Combat Log */}
          <ul className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2.5 text-xs">
            {battle.events.slice(0, 4).map((ev) => (
              <li
                key={ev.id}
                className={
                  ev.tone === "win"
                    ? "font-medium text-pill-active-fg"
                    : ev.tone === "ult"
                      ? "font-medium text-tag-design-fg"
                      : ev.tone === "shield"
                        ? "text-tag-sky-fg"
                        : ev.tone === "boss"
                          ? "text-pill-pro-fg"
                          : "text-ink-2"
                }
              >
                {ev.text}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
