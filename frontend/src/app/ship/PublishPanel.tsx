import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ArrowRight, BadgeCheck, Check, CircleAlert, ExternalLink, Globe, KeyRound, Package, Rocket, Save, Settings2, Ticket } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, prettyBytes, projUrl, type CreatorHubConfigureResult, type CreatorHubSetupResult, type Product } from "./api";
import { Field } from "./ProjectsRail";
import type { Ship } from "./useShip";

type SyncRow = { key: string; kind?: string; action: string; id?: number | string; error?: string };
const ACTIONS: Record<string, string> = { created: "Created", updated: "Updated", adopted: "Linked to existing", unchanged: "Up to date", skipped: "Skipped" };

function PipelineStep({
  title,
  ok,
  badge,
  children,
}: {
  title: string;
  ok: boolean;
  badge: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-1 flex-col justify-between gap-2 rounded-sm border border-hairline bg-board/40 p-2.5">
      <div className="flex items-center justify-between gap-1.5">
        <span className="inline-flex items-center gap-1.5 text-xs font-medium text-ink">
          <span className={`grid size-5 shrink-0 place-items-center rounded-full ${ok ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-warn-bg text-pill-warn-fg"}`}>
            {ok ? <Check className="size-3" strokeWidth={2} aria-hidden /> : <CircleAlert className="size-3" strokeWidth={2} aria-hidden />}
          </span>
          {title}
        </span>
        <Pill className={`px-1.5 py-0 text-xs ${ok ? "bg-pill-active-bg text-pill-active-fg" : "bg-surface text-ink-2"}`}>{badge}</Pill>
      </div>
      {children && <div className="flex flex-wrap items-center gap-1">{children}</div>}
    </div>
  );
}

function UniverseForm({ id }: { id: string }) {
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [vis, setVis] = useState("PRIVATE");
  const [psEnabled, setPsEnabled] = useState(true);
  const [psPrice, setPsPrice] = useState("0");
  const [iconPath, setIconPath] = useState("");
  const [thumbPaths, setThumbPaths] = useState("");
  const [state, setState] = useState<"loading" | "idle" | "saving" | "browser">("loading");
  const [msg, setMsg] = useState("");
  const [hubResult, setHubResult] = useState<CreatorHubConfigureResult | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      const r = await call<{ universe?: Record<string, unknown> } & Record<string, unknown>>(projUrl(id, "/universe"));
      if (!live) return;
      const u = (r.universe ?? r) as Record<string, unknown>;
      if (!r.ok) setMsg(r.error || "Could not read the game page.");
      else { setName(String(u.displayName ?? "")); setDesc(String(u.description ?? "")); setVis(String(u.visibility ?? "PRIVATE").toUpperCase()); }
      setState("idle");
    })();
    return () => { live = false; };
  }, [id]);
  const save = async () => {
    setState("saving");
    const r = await call(projUrl(id, "/universe"), { settings: { displayName: name, description: desc, visibility: vis } });
    setState("idle");
    if (!r.ok) { setMsg(r.error || "Could not save the game page."); return; }
    setMsg(""); toast.success("Game page saved");
  };
  const configureBrowser = async () => {
    setState("browser"); setMsg(""); setHubResult(null);
    const thumbs = thumbPaths.split(",").map((s) => s.trim()).filter(Boolean);
    const priceNum = Number.parseInt(psPrice.trim(), 10);
    const r = await call<CreatorHubConfigureResult>(projUrl(id, "/creator-hub/configure"), {
      ...(name.trim() ? { name: name.trim() } : {}),
      ...(desc.trim() ? { description: desc.trim() } : {}),
      visibility: vis,
      private_servers: {
        enabled: psEnabled,
        ...(psEnabled && Number.isFinite(priceNum) && priceNum >= 0 ? { price_robux: priceNum } : {}),
      },
      ...(iconPath.trim() ? { icon_path: iconPath.trim() } : {}),
      ...(thumbs.length > 0 ? { thumbnail_paths: thumbs } : {}),
      check_questionnaire: true,
    });
    setState("idle");
    if (!r.ok && r.status !== "action_required" && !r.blocked_reason) {
      setMsg(r.error || "Could not configure game page via browser.");
      return;
    }
    setHubResult(r as CreatorHubConfigureResult);
    if (r.configured?.length) toast.success(`Configured ${r.configured.length} setting(s) via browser`);
  };
  return (
    <div className="mt-3 flex flex-col gap-3">
      {msg && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{msg}</p>}
      {state === "loading" ? <p className="text-sm text-ink-2">Reading the game page…</p> : (
        <>
          <Field label="Game name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
          <Field label="Description"><Input value={desc} onChange={(e) => setDesc(e.target.value)} /></Field>
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm font-medium text-ink-2">Who can play</span>
            <SegmentedControl aria-label="Who can play" size="sm">
              {(["PRIVATE", "PUBLIC"] as const).map((v) => (
                <SegmentedControlItem key={v} data-checked={vis === v ? "" : undefined} aria-pressed={vis === v} onClick={() => setVis(v)} className="max-md:!h-11 max-md:px-3">{v === "PUBLIC" ? "Everyone" : "Only me"}</SegmentedControlItem>
              ))}
            </SegmentedControl>
          </div>
          <div className="grid gap-2 sm:grid-cols-3">
            <div className="flex flex-col gap-1">
              <span className="text-sm font-medium text-ink-2">Private servers</span>
              <SegmentedControl aria-label="Private servers" size="sm">
                <SegmentedControlItem data-checked={psEnabled ? "" : undefined} aria-pressed={psEnabled} onClick={() => setPsEnabled(true)} className="max-md:!h-11">Enabled</SegmentedControlItem>
                <SegmentedControlItem data-checked={!psEnabled ? "" : undefined} aria-pressed={!psEnabled} onClick={() => setPsEnabled(false)} className="max-md:!h-11">Off</SegmentedControlItem>
              </SegmentedControl>
            </div>
            <Field label="Private server Robux"><Input inputMode="numeric" value={psPrice} onChange={(e) => setPsPrice(e.target.value)} className="font-mono" disabled={!psEnabled} /></Field>
            <Field label="Game icon path (optional)"><Input value={iconPath} onChange={(e) => setIconPath(e.target.value)} placeholder="art/icon.png" className="font-mono text-xs" /></Field>
          </div>
          <Field label="Thumbnail paths (comma-separated, optional)"><Input value={thumbPaths} onChange={(e) => setThumbPaths(e.target.value)} placeholder="art/thumb1.png, art/thumb2.png" className="font-mono text-xs" /></Field>
          <div className="flex flex-wrap gap-2">
            <Button variant="default" size="lg" disabled={state === "saving" || state === "browser" || !name.trim()} onClick={() => void save()} className="active:opacity-80 disabled:opacity-50 max-md:min-h-11"><Save className="size-4" strokeWidth={1.5} aria-hidden />{state === "saving" ? "Saving…" : "Save game page"}</Button>
            <Button variant="outline" size="lg" disabled={state === "saving" || state === "browser"} onClick={() => void configureBrowser()} data-configure-browser className="active:opacity-80 disabled:opacity-50 max-md:min-h-11"><Globe className="size-4" strokeWidth={1.5} aria-hidden />{state === "browser" ? "Configuring…" : "Configure via open browser"}</Button>
          </div>
          {hubResult && (
            <div className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/60 p-3 text-sm" data-creator-hub-configure-result>
              {(hubResult.blocked_reason || hubResult.message) && (
                <p className="text-pill-warn-fg">
                  {hubResult.message || hubResult.blocked_reason}
                  {hubResult.url && (
                    <a href={hubResult.url} target="_blank" rel="noreferrer" className="ml-2 inline-flex items-center gap-1 font-medium underline">
                      Open page <ExternalLink className="size-3.5" strokeWidth={1.5} aria-hidden />
                    </a>
                  )}
                </p>
              )}
              {(hubResult.configured?.length ?? 0) > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {hubResult.configured!.map((c) => (
                    <Pill key={c} className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">{c}</Pill>
                  ))}
                </div>
              )}
              {(hubResult.manual_steps_remaining?.length ?? 0) > 0 && (
                <ul className="flex flex-col gap-1.5" aria-label="Manual steps remaining">
                  {hubResult.manual_steps_remaining!.map((m) => (
                    <li key={m.step} className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-surface px-2.5 py-1.5 text-xs">
                      <span className="text-ink">{m.step}</span>
                      {m.url && (
                        <a href={m.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 font-medium text-ink underline">
                          Open <ExternalLink className="size-3" strokeWidth={1.5} aria-hidden />
                        </a>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}

function UniverseSettings({ id }: { id: string }) {
  return (
    <Expandable expandDirection="vertical" expandBehavior="push">
      {({ isExpanded }) => (
        <div className="card-stock p-3">
          <ExpandableTrigger aria-label="Game page on Roblox" className="flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 text-left text-ui font-medium outline-none focus-visible:ring-2 focus-visible:ring-focus">
            <span className="inline-flex items-center gap-2"><Settings2 className="size-4 text-icon" strokeWidth={1.5} aria-hidden />Game page on Roblox</span>
            <span className="text-sm font-normal text-ink-2">{isExpanded ? "Hide" : "Name, description, who can play"}</span>
          </ExpandableTrigger>
          {isExpanded && <UniverseForm id={id} />}
        </div>
      )}
    </Expandable>
  );
}

function SetProjectIdsDialog({ active, ship }: { active: { universeId?: number | null; placeId?: number | null }; ship: Ship }) {
  const [open, setOpen] = useState(false);
  const [uni, setUni] = useState(active.universeId ? String(active.universeId) : "");
  const [place, setPlace] = useState(active.placeId ? String(active.placeId) : "");
  const [saving, setSaving] = useState(false);
  const submit = async () => {
    setSaving(true);
    const ok = await ship.saveManifest({
      universeId: /^\d+$/.test(uni.trim()) ? Number(uni.trim()) : null,
      placeId: /^\d+$/.test(place.trim()) ? Number(place.trim()) : null,
    });
    setSaving(false);
    if (ok) { toast.success("Project IDs saved"); setOpen(false); }
  };
  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={setOpen}>
      <MorphingDialogTrigger thing="set-project-ids" className="inline-flex min-h-8 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-2.5 text-xs font-medium text-ink outline-none hover:bg-board active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus max-md:min-h-11">
        Set IDs
      </MorphingDialogTrigger>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex w-full max-w-[420px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
          <div>
            <MorphingDialogTitle className="font-heading text-title font-semibold">Roblox game and place numbers</MorphingDialogTitle>
            <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Saved to openro.project.json for publishing and passes.</MorphingDialogDescription>
          </div>
          <Field label="Game number (Universe ID)"><Input inputMode="numeric" value={uni} onChange={(e) => setUni(e.target.value)} className="font-mono" placeholder="123456789" /></Field>
          <Field label="Place number (Place ID)"><Input inputMode="numeric" value={place} onChange={(e) => setPlace(e.target.value)} className="font-mono" placeholder="987654321" /></Field>
          <div className="flex flex-wrap justify-end gap-2">
            <MorphingDialogClose aria-label="Cancel" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Cancel</MorphingDialogClose>
            <Button variant="default" size="lg" disabled={saving || !/^\d+$/.test(uni.trim()) || !/^\d+$/.test(place.trim())} onClick={() => void submit()} className="max-md:min-h-11">{saving ? "Saving…" : "Save IDs"}</Button>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}

export function PublishPanel({ ship, onOpenSettings }: { ship: Ship; onOpenSettings: () => void }) {
  const { active, detail, cloud, build } = ship;
  const [live, setLive] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [published, setPublished] = useState<{ versionNumber?: number; versionType?: string; bytes?: number } | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [rows, setRows] = useState<SyncRow[] | null>(null);
  const [error, setError] = useState("");
  const [dialogOpen, setDialogOpen] = useState(false);
  const [hubBusy, setHubBusy] = useState(false);
  const [hubSetupNotice, setHubSetupNotice] = useState<{ text: string; url?: string } | null>(null);
  useEffect(() => { setPublished(null); setRows(null); setError(""); setHubSetupNotice(null); }, [active?.id]);
  if (!active) return null;

  const products: Product[] = detail?.manifest?.products ?? detail?.products ?? [];
  const hasKey = !!cloud?.has_key;
  const hasIds = !!(active.universeId && active.placeId);
  const hasBuild = !!(build ?? active.last_build);
  const ready = [hasKey, hasIds, hasBuild].filter(Boolean).length;
  const canPublish = hasKey && hasIds && hasBuild && !publishing;
  const pending = products.filter((p) => p.priceApproved !== true).length;

  const runBrowserSetup = async () => {
    setHubBusy(true); setError(""); setHubSetupNotice(null);
    const r = await call<CreatorHubSetupResult>(projUrl(active.id, "/creator-hub/setup"), {});
    setHubBusy(false);
    if (r.status === "action_required" || r.blocked_reason) {
      setHubSetupNotice({
        text: r.message || r.blocked_reason || "Finish sign-in or verification in the open browser, then try again.",
        url: r.url,
      });
      return;
    }
    if (!r.ok) {
      setError(r.error || "Could not auto-setup from open browser.");
      return;
    }
    await Promise.all([ship.refresh(), ship.refreshCloud()]);
    toast.success("Project ready from open browser");
  };

  const publish = async () => {
    setPublishing(true); setError("");
    const r = await call<{ versionNumber: number; versionType: string; bytes: number }>(projUrl(active.id, "/publish"), { versionType: live ? "Published" : "Saved", confirm: live });
    setPublishing(false); setDialogOpen(false);
    if (!r.ok) { setError(r.error || "Roblox did not take the place."); return; }
    setPublished({ versionNumber: r.versionNumber, versionType: r.versionType, bytes: r.bytes });
    toast.success(live ? "Published live" : "Saved a new version");
  };
  const approve = (keys: string[]) => {
    const next = products.map((p) => (keys.includes(p.key) ? { ...p, priceApproved: true } : p));
    void ship.saveManifest({ products: next });
  };
  const syncProducts = async () => {
    setSyncing(true); setError("");
    const r = await call<{ results: SyncRow[] }>(projUrl(active.id, "/products/sync"), {});
    setSyncing(false);
    if (!r.ok) { setError(r.error || "Could not send the products."); return; }
    setRows(r.results ?? []);
    await ship.refresh();
  };

  return (
    <div className="flex flex-col gap-4" data-panel="publish">
      <section aria-label="Ready to publish" className="card-stock flex flex-col gap-3 p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <ProgressCircle value={Math.round((ready / 3) * 100)} radius={15} strokeWidth={3.5} className="size-9 flex-none">
              <span className="font-mono text-sm font-medium tabular-nums text-ink">{ready}/3</span>
            </ProgressCircle>
            <div className="min-w-0">
              <h3 className="font-heading text-ui font-semibold">Publish {active.name}</h3>
              <p className="text-sm text-ink-2">{ready === 3 ? "All release checks passed." : "Complete the highlighted release stages below."}</p>
            </div>
          </div>
          <SegmentedControl aria-label="Publish kind" size="sm">
            <SegmentedControlItem data-checked={!live ? "" : undefined} aria-pressed={!live} onClick={() => setLive(false)} className="max-md:!h-11 max-md:px-3">Save version</SegmentedControlItem>
            <SegmentedControlItem data-checked={live ? "" : undefined} aria-pressed={live} onClick={() => setLive(true)} className="max-md:!h-11 max-md:px-3">Publish live</SegmentedControlItem>
          </SegmentedControl>
        </div>

        {/* Visual 4-Stage Release Pipeline Diagram */}
        <div className="flex flex-col gap-2 sm:flex-row sm:items-stretch" data-release-pipeline>
          <PipelineStep title="Key" ok={hasKey} badge={hasKey ? "Saved" : "Needed"}>
            {!hasKey && (
              <>
                <Button variant="outline" size="xs" disabled={hubBusy} onClick={() => void runBrowserSetup()} data-publish-auto-setup className="active:opacity-80 disabled:opacity-50 max-md:min-h-11">
                  <Globe className="size-3" strokeWidth={1.5} aria-hidden />
                  {hubBusy ? "Connecting…" : "Auto-setup from browser"}
                </Button>
                <Button variant="ghost" size="xs" onClick={onOpenSettings} className="active:opacity-80 max-md:min-h-11">Settings</Button>
              </>
            )}
          </PipelineStep>
          <ArrowRight className="hidden size-4 shrink-0 self-center text-ink-3 sm:block" strokeWidth={1.5} aria-hidden />
          <PipelineStep title="Experience" ok={hasIds} badge={hasIds ? String(active.universeId) : "Missing"}>
            {!hasIds ? (
              <>
                <Button variant="outline" size="xs" disabled={hubBusy} onClick={() => void runBrowserSetup()} className="active:opacity-80 disabled:opacity-50 max-md:min-h-11">
                  <Globe className="size-3" strokeWidth={1.5} aria-hidden />
                  {hubBusy ? "Connecting…" : "Auto-setup"}
                </Button>
                <SetProjectIdsDialog active={active} ship={ship} />
              </>
            ) : (
              <SetProjectIdsDialog active={active} ship={ship} />
            )}
          </PipelineStep>
          <ArrowRight className="hidden size-4 shrink-0 self-center text-ink-3 sm:block" strokeWidth={1.5} aria-hidden />
          <PipelineStep title="Build" ok={hasBuild} badge={hasBuild ? "Built" : "None"}>
            {hasBuild && (
              <span className="font-mono text-xs text-ink-2">
                {(build ?? active.last_build)?.build_id || "ready"}
              </span>
            )}
          </PipelineStep>
          <ArrowRight className="hidden size-4 shrink-0 self-center text-ink-3 sm:block" strokeWidth={1.5} aria-hidden />
          <PipelineStep title="Live" ok={Boolean(published)} badge={published ? `v${published.versionNumber}` : live ? "Live" : "Save"}>
            {live ? (
              <MorphingDialog transition={dialogSpring} open={dialogOpen} onOpenChange={setDialogOpen}>
                <MorphingDialogTrigger blocked={!canPublish} className="inline-flex min-h-8 items-center justify-center gap-1.5 rounded-lg border border-primary bg-primary px-3 text-xs font-medium text-primary-foreground outline-none hover:bg-primary/90 active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus disabled:border-hairline disabled:bg-track disabled:text-ink-3 max-md:min-h-11" thing="publish">
                  <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />Publish
                </MorphingDialogTrigger>
                <MorphingDialogContainer>
                  <MorphingDialogContent className="relative mx-3 flex w-full max-w-[440px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                    <div>
                      <MorphingDialogTitle className="font-heading text-title font-semibold">Publish live?</MorphingDialogTitle>
                      <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Players receive this build immediately once published.</MorphingDialogDescription>
                    </div>
                    <div className="flex flex-wrap justify-end gap-2">
                      <MorphingDialogClose aria-label="Not yet" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Not yet</MorphingDialogClose>
                      <Button variant="default" size="lg" disabled={publishing} onClick={() => void publish()} className="active:opacity-80 disabled:opacity-50 max-md:min-h-11">{publishing ? "Publishing…" : "Yes, publish live"}</Button>
                    </div>
                  </MorphingDialogContent>
                </MorphingDialogContainer>
              </MorphingDialog>
            ) : (
              <Button variant="default" size="xs" disabled={!canPublish} onClick={() => void publish()} data-publish className="min-h-8 gap-1.5 px-3 text-xs active:opacity-80 disabled:opacity-50 max-md:min-h-11">
                <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />{publishing ? "Saving…" : "Save to Roblox"}
              </Button>
            )}
          </PipelineStep>
        </div>

        {hubSetupNotice && (
          <div role="status" className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-pill-warn-bg px-3 py-2 text-sm text-pill-warn-fg">
            <span>{hubSetupNotice.text}</span>
            {hubSetupNotice.url && (
              <a href={hubSetupNotice.url} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 font-medium underline">
                Open Creator Hub <ExternalLink className="size-3.5" strokeWidth={1.5} aria-hidden />
              </a>
            )}
          </div>
        )}
        {!hasKey && <p className="inline-flex items-center gap-1.5 text-sm text-ink-2"><KeyRound className="size-3.5" strokeWidth={1.5} aria-hidden />Save an Open Cloud key to enable publishing.</p>}
        {error && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
        {published && <p className="rounded-sm bg-pill-active-bg px-3 py-2 text-sm text-pill-active-fg" data-published>Version <span className="font-mono">{published.versionNumber}</span> on Roblox ({published.versionType === "Published" ? "live" : "saved"}){published.bytes ? ` (${prettyBytes(published.bytes)})` : ""}.</p>}
      </section>

      <section aria-label="Game passes and products" className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="font-heading text-ui font-semibold">Monetization <span className="font-mono text-sm font-normal text-ink-2">{products.length}</span></h3>
          <div className="flex flex-wrap gap-2">
            {pending > 0 && <Button variant="outline" size="sm" onClick={() => approve(products.filter((p) => p.priceApproved !== true).map((p) => p.key))} className="active:opacity-80 max-md:min-h-11">Approve all {pending} prices</Button>}
            <Button variant="default" size="sm" disabled={!hasKey || !active.universeId || syncing || products.length === 0 || pending === products.length} onClick={() => void syncProducts()} className="active:opacity-80 disabled:opacity-50 max-md:min-h-11">{syncing ? "Sending…" : "Send to Roblox"}</Button>
          </div>
        </div>
        {products.length === 0 ? (
          <p className="card-stock p-4 text-sm text-ink-2">No passes or products listed in <code className="font-mono">openro.project.json</code> yet.</p>
        ) : (
          <div className="card-stock overflow-x-auto" data-monetization-table>
            <table className="w-full border-collapse text-left text-sm">
              <thead>
                <tr className="border-b border-hairline bg-board/50 text-xs font-medium text-ink-2">
                  <th className="px-3 py-2">Item</th>
                  <th className="px-3 py-2">Kind</th>
                  <th className="px-3 py-2">Price (R$)</th>
                  <th className="px-3 py-2">Status</th>
                  <th className="px-3 py-2">Roblox ID</th>
                  <th className="px-3 py-2 text-right">Action</th>
                </tr>
              </thead>
              <tbody>
                {products.map((p) => {
                  const approved = p.priceApproved === true;
                  const Icon = p.kind === "game_pass" ? Ticket : Package;
                  return (
                    <tr key={p.key} className="border-b border-hairline/70 last:border-b-0" data-product={p.key}>
                      <td className="px-3 py-2 align-middle">
                        <div className="flex items-center gap-2">
                          <Icon className="size-4 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                          <span className="font-medium text-ink">{p.name}</span>
                        </div>
                      </td>
                      <td className="px-3 py-2 align-middle">
                        <Pill className="bg-tag-neutral-bg px-2 py-0.5 text-xs text-tag-neutral-fg">
                          {p.kind === "game_pass" ? "game pass" : "product"}
                        </Pill>
                      </td>
                      <td className="px-3 py-2 align-middle font-mono tabular-nums text-ink">{p.priceRobux}</td>
                      <td className="px-3 py-2 align-middle">
                        <div className="flex flex-wrap items-center gap-1">
                          {approved ? (
                            <Pill className="gap-1 bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" data-price="approved">
                              <BadgeCheck className="size-3.5" strokeWidth={1.5} aria-hidden />approved
                            </Pill>
                          ) : (
                            <Pill className="bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg" data-price="pending">pending approval</Pill>
                          )}
                          {p.forSale === false && <Pill className="bg-pill-inactive-bg px-2 py-0.5 text-xs text-pill-inactive-fg">off sale</Pill>}
                        </div>
                      </td>
                      <td className="px-3 py-2 align-middle font-mono text-xs text-ink-2">
                        {p.id ? String(p.id) : "-"}
                      </td>
                      <td className="px-3 py-2 text-right align-middle">
                        {!approved ? (
                          <Button variant="outline" size="xs" onClick={() => approve([p.key])} className="active:opacity-80 max-md:min-h-11">
                            Approve price
                          </Button>
                        ) : (
                          <span className="text-xs text-ink-3">Ready</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {pending > 0 && <p className="text-sm text-ink-2">Approve pending prices before syncing to Roblox.</p>}
        {rows && (
          <ul className="card-stock flex flex-col gap-1 p-3 text-sm" aria-label="Last send" data-sync-rows>
            {rows.map((r) => (
              <li key={r.key} className="flex flex-wrap items-baseline justify-between gap-2"><span className="font-medium">{r.key}</span><span className={r.error ? "text-pill-pro-fg" : "text-ink-2"}>{r.error || ACTIONS[r.action] || r.action}{r.id ? <span className="ml-2 font-mono">{r.id}</span> : null}</span></li>
            ))}
          </ul>
        )}
      </section>

      <UniverseSettings id={active.id} />
    </div>
  );
}
