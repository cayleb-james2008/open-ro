import { useState } from "react";
import { toast } from "sonner";
import { Check, Coins, KeyRound, Play, ShieldCheck, Sparkles, Ticket } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { post, type Place } from "./data";
import { analyzePlaceLuau } from "./luauAudit";

export type TierPresetId = "starter_obby" | "anime_gacha_rpg" | "competitive_arena";

type TierPreset = {
  id: TierPresetId;
  label: string;
  currency: string;
  baseReward: number;
  comebackMultiplier: number;
  curveMultipliers: readonly number[];
  summary: string;
};

const TIER_PRESETS: TierPreset[] = [
  {
    id: "starter_obby",
    label: "Starter Obby / Sim",
    currency: "Coins",
    baseReward: 100,
    comebackMultiplier: 2,
    curveMultipliers: [1, 2, 3, 4, 5, 7, 12],
    summary: "2 Passes (79-149 R$) + 3 DevProducts (25-99 R$)",
  },
  {
    id: "anime_gacha_rpg",
    label: "Anime Gacha / RPG",
    currency: "Gems",
    baseReward: 150,
    comebackMultiplier: 3,
    curveMultipliers: [1, 2, 3, 5, 7, 10, 15],
    summary: "2 Passes (149-299 R$) + 3 Summon Packs (99-399 R$)",
  },
  {
    id: "competitive_arena",
    label: "Competitive Arena",
    currency: "Credits",
    baseReward: 120,
    comebackMultiplier: 2,
    curveMultipliers: [1, 2, 3, 4, 6, 8, 12],
    summary: "Battle Pass + Loadout+ (129-249 R$) + 3 Boosters",
  },
];

export function MonetizationPackCard({
  place,
  studioOk,
  activeProjectId,
  onInstalled,
  onRunPlaytest,
  onOpenCodeDiff,
}: {
  place: Place | null | "error";
  studioOk: boolean;
  activeProjectId: string | null;
  onInstalled: () => Promise<void> | void;
  onRunPlaytest?: () => Promise<void> | void;
  onOpenCodeDiff?: (scriptPath: string) => void;
}) {
  const [tierPreset, setTierPreset] = useState<TierPresetId>("starter_obby");
  const [currency, setCurrency] = useState("Coins");
  const [dailyReward, setDailyReward] = useState("100");
  const [installing, setInstalling] = useState(false);

  const activePreset = TIER_PRESETS.find((p) => p.id === tierPreset) ?? TIER_PRESETS[0];
  const baseRewardNum = Math.max(10, Number.parseInt(dailyReward, 10) || activePreset.baseReward);
  const curveValues = activePreset.curveMultipliers.map((m) => m * baseRewardNum);

  const selectPreset = (preset: TierPreset) => {
    setTierPreset(preset.id);
    setCurrency(preset.currency);
    setDailyReward(String(preset.baseReward));
  };

  const scripts = place && place !== "error" ? place.scripts || {} : {};
  const hasMonetization = Boolean(scripts["ServerScriptService.MonetizationService"]);
  const hasDailyRetention = Boolean(scripts["ServerScriptService.DailyRewardRetention"]);
  const hasConfig = Boolean(scripts["ReplicatedStorage.MonetizationConfig"]);
  const diags = place && place !== "error" ? analyzePlaceLuau(place) : [];
  const luauErrors = diags.filter((d) => d.severity === "error").length;

  const installPack = async () => {
    setInstalling(true);
    try {
      const endpoint = activeProjectId
        ? `/api/projects/${encodeURIComponent(activeProjectId)}/monetization/pack`
        : "/api/monetization/pack";
      const res = await post<{ ok?: boolean; error?: string; scripts?: string[]; written?: string[] }>(endpoint, {
        tier_preset: tierPreset,
        currency: currency.trim() || activePreset.currency,
        daily_base_reward: baseRewardNum,
      });
      if (res.ok === false) {
        toast.error(res.error || "Could not install Monetization & Retention Pack.");
        return;
      }
      await onInstalled();
      toast.success(`${activePreset.label} pack wired (${activePreset.comebackMultiplier}x comeback streak)`);
    } catch {
      toast.error("Could not install Monetization & Retention Pack.");
    } finally {
      setInstalling(false);
    }
  };

  const checks = [
    {
      label: "Idempotent ProcessReceipt & GamePass service",
      ok: hasMonetization,
      detail: hasMonetization ? "PurchaseId DataStore ledger active" : "Not yet added",
    },
    {
      label: "Daily streak & 48h comeback hook",
      ok: hasDailyRetention,
      detail: hasDailyRetention
        ? `BindToClose, UpdateAsync & ${activePreset.comebackMultiplier}x comeback wired`
        : "Not yet added",
    },
    {
      label: "MonetizationConfig shared module",
      ok: hasConfig,
      detail: hasConfig ? `${activePreset.label} 7-day curve ready` : "Not yet added",
    },
    {
      label: "Static Luau safety audit",
      ok: luauErrors === 0,
      detail: luauErrors === 0 ? "0 errors across place scripts" : `${luauErrors} error(s) found`,
    },
  ];

  return (
    <section
      data-monetization-pack
      aria-label="Monetization and Retention Luau Pack"
      className="flex flex-col gap-3 rounded-card border border-hairline bg-surface p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Ticket className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="text-ui font-medium text-ink">Monetization Tier &amp; Retention Designer</h3>
          </div>
          <p className="mt-0.5 text-sm text-ink-2">
            Pick a balanced genre tier preset to scaffold strict GamePasses, idempotent ProcessReceipt guards, draft Robux tiers, and a 7-day streak + 48h comeback loop.
          </p>
        </div>
        <Pill className={hasMonetization && hasDailyRetention ? "bg-pill-active-bg text-pill-active-fg" : "bg-board text-ink-2"}>
          <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden />
          <span>{hasMonetization && hasDailyRetention ? "Installed" : "Ready to install"}</span>
        </Pill>
      </div>

      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Monetization tier preset">
        {TIER_PRESETS.map((preset) => {
          const active = preset.id === tierPreset;
          return (
            <Button
              key={preset.id}
              type="button"
              variant={active ? "default" : "outline"}
              size="xs"
              data-tier-preset={preset.id}
              aria-pressed={active}
              onClick={() => selectPreset(preset)}
            >
              {preset.label}
            </Button>
          );
        })}
      </div>

      <div className="grid gap-2.5 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
          <span>Currency stat name</span>
          <Input
            value={currency}
            onChange={(e) => setCurrency(e.target.value)}
            placeholder={activePreset.currency}
            aria-label="Monetization currency name"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
          <span>Day-1 base reward</span>
          <Input
            inputMode="numeric"
            value={dailyReward}
            onChange={(e) => setDailyReward(e.target.value)}
            placeholder={String(activePreset.baseReward)}
            aria-label="Day-1 daily reward amount"
            className="font-mono"
          />
        </label>
      </div>

      <div
        data-retention-curve-preview
        className="flex flex-col gap-1.5 rounded-lg border border-hairline bg-board/40 px-3 py-2 text-xs"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="font-medium text-ink">
            7-Day Streak + {activePreset.comebackMultiplier}x Comeback Curve (48h+)
          </span>
          <span className="text-ink-2">
            {activePreset.summary} · Drafts require owner approval
          </span>
        </div>
        <div className="grid grid-cols-7 gap-1 tabular-nums">
          {curveValues.map((val, idx) => (
            <div
              key={idx}
              className="flex flex-col items-center rounded border border-hairline bg-surface px-1 py-1"
            >
              <span className="text-[11px] text-ink-3">D{idx + 1}</span>
              <span className="font-medium text-ink">{val}</span>
            </div>
          ))}
        </div>
      </div>

      <ul className="grid gap-1.5 sm:grid-cols-2" aria-label="Publish and monetization readiness checks">
        {checks.map((c) => (
          <li
            key={c.label}
            className="flex items-start gap-2 rounded-lg border border-hairline bg-board/50 px-2.5 py-2 text-xs"
          >
            <span
              className={`mt-0.5 grid size-4 shrink-0 place-items-center rounded-full ${
                c.ok ? "bg-pill-active-bg text-pill-active-fg" : "bg-track text-ink-3"
              }`}
            >
              {c.ok ? <Check className="size-3" strokeWidth={2} aria-hidden /> : <KeyRound className="size-2.5" strokeWidth={1.75} aria-hidden />}
            </span>
            <div className="min-w-0">
              <div className="font-medium text-ink">{c.label}</div>
              <div className="text-ink-2">{c.detail}</div>
            </div>
          </li>
        ))}
      </ul>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="default"
          size="sm"
          loading={installing}
          disabled={!studioOk && !activeProjectId}
          onClick={() => void installPack()}
          data-install-monetization
        >
          <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
          {hasMonetization ? "Update Tier & Retention Scripts" : "Apply Tier Preset & Wire Scripts"}
        </Button>
        {hasMonetization && onOpenCodeDiff && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenCodeDiff("ServerScriptService.MonetizationService")}
          >
            <Coins className="size-3.5" strokeWidth={1.5} aria-hidden />
            Inspect Luau
          </Button>
        )}
        {hasMonetization && onRunPlaytest && (
          <Button variant="outline" size="sm" onClick={() => void onRunPlaytest()}>
            <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
            Verify with Playtest
          </Button>
        )}
      </div>
    </section>
  );
}
