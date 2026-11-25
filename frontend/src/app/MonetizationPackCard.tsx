import { useState } from "react";
import { toast } from "sonner";
import { Check, Coins, KeyRound, Play, ShieldCheck, Sparkles, Ticket } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { post, type Place } from "./data";
import { analyzePlaceLuau } from "./luauAudit";

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
  const [currency, setCurrency] = useState("Coins");
  const [dailyReward, setDailyReward] = useState("100");
  const [installing, setInstalling] = useState(false);

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
        currency: currency.trim() || "Coins",
        daily_base_reward: Math.max(10, Number.parseInt(dailyReward, 10) || 100),
      });
      if (res.ok === false) {
        toast.error(res.error || "Could not install Monetization & Retention Pack.");
        return;
      }
      await onInstalled();
      toast.success("Monetization & Retention Luau pack installed");
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
      label: "Daily Reward 24h retention streak",
      ok: hasDailyRetention,
      detail: hasDailyRetention ? "BindToClose & UpdateAsync wired" : "Not yet added",
    },
    {
      label: "MonetizationConfig shared module",
      ok: hasConfig,
      detail: hasConfig ? "Currency & 7-day reward tiers ready" : "Not yet added",
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
            <h3 className="text-ui font-medium text-ink">Monetization &amp; Retention Luau Pack</h3>
          </div>
          <p className="mt-0.5 text-sm text-ink-2">
            Generates strict server-authoritative GamePass caching, idempotent ProcessReceipt DataStore guards, PolicyService checks, and 24h Daily Login streaks.
          </p>
        </div>
        <Pill className={hasMonetization && hasDailyRetention ? "bg-pill-active-bg text-pill-active-fg" : "bg-board text-ink-2"}>
          <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden />
          <span>{hasMonetization && hasDailyRetention ? "Installed" : "Ready to install"}</span>
        </Pill>
      </div>

      <div className="grid gap-2.5 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
          <span>Currency stat name</span>
          <Input
            value={currency}
            onChange={(e) => setCurrency(e.target.value)}
            placeholder="Coins"
            aria-label="Monetization currency name"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
          <span>Day-1 base reward</span>
          <Input
            inputMode="numeric"
            value={dailyReward}
            onChange={(e) => setDailyReward(e.target.value)}
            placeholder="100"
            aria-label="Day-1 daily reward amount"
            className="font-mono"
          />
        </label>
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
          {hasMonetization ? "Update Monetization Pack" : "Install Monetization Pack"}
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
