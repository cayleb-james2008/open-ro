import { useEffect, useState } from "react";
import { toast } from "sonner";
import { BadgeCheck, Check, CircleAlert, KeyRound, Rocket, Save, Settings2, Ticket, Package } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, prettyBytes, projUrl, type Product } from "./api";
import { Field } from "./ProjectsRail";
import type { Ship } from "./useShip";

type SyncRow = { key: string; kind?: string; action: string; id?: number | string; error?: string };
const ACTIONS: Record<string, string> = { created: "Created", updated: "Updated", adopted: "Linked to the one already on Roblox", unchanged: "Already up to date", skipped: "Skipped" };

function Check1({ ok, label, fix, onFix }: { ok: boolean; label: string; fix?: string; onFix?: () => void }) {
  return (
    <li className="flex min-h-11 items-center gap-2 text-sm">
      <span className={`grid size-6 flex-none place-items-center rounded-full ${ok ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-warn-bg text-pill-warn-fg"}`}>
        {ok ? <Check className="size-3.5" strokeWidth={2} aria-hidden /> : <CircleAlert className="size-3.5" strokeWidth={2} aria-hidden />}
      </span>
      <span className="min-w-0 flex-1 text-ink">{label}</span>
      {!ok && fix && onFix && <Button variant="ghost" size="sm" onClick={onFix} className="flex-none max-md:min-h-11">{fix}</Button>}
    </li>
  );
}

function ProductRow({ p, canApprove, onApprove }: { p: Product; canApprove: boolean; onApprove: () => void }) {
  const approved = p.priceApproved === true;
  const Icon = p.kind === "game_pass" ? Ticket : Package;
  return (
    <li className="card-stock flex flex-wrap items-center gap-3 px-3 py-2.5" data-product={p.key}>
      <span className="grid size-9 flex-none place-items-center rounded-card bg-tag-research-bg text-tag-research-fg"><Icon className="size-4" strokeWidth={1.5} aria-hidden /></span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-ui font-medium">{p.name}</span>
          <Pill className="bg-tag-neutral-bg px-2 py-0.5 text-sm text-tag-neutral-fg">{p.kind === "game_pass" ? "game pass" : "product"}</Pill>
          {p.forSale === false && <Pill className="bg-pill-inactive-bg px-2 py-0.5 text-sm text-pill-inactive-fg">not for sale</Pill>}
        </div>
        <p className="mt-0.5 truncate text-sm text-ink-2">{p.description || p.key}</p>
      </div>
      <div className="flex flex-none items-center gap-2">
        {approved ? (
          <span className="inline-flex items-center gap-1.5 font-mono text-ui font-medium tabular-nums" data-price="approved"><BadgeCheck className="size-4 text-dot-green" strokeWidth={1.5} aria-hidden />{p.priceRobux} Robux</span>
        ) : (
          <span className="flex flex-col items-end gap-0.5" data-price="pending">
            <Pill className="bg-pill-warn-bg px-2.5 py-0.5 text-sm text-pill-warn-fg">pending approval</Pill>
            <span className="text-sm text-ink-2">proposed <span className="font-mono tabular-nums">{p.priceRobux}</span></span>
          </span>
        )}
        {!approved && canApprove && <Button variant="outline" size="sm" onClick={onApprove} className="max-md:min-h-11">Approve price</Button>}
      </div>
    </li>
  );
}

function UniverseForm({ id }: { id: string }) {
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [vis, setVis] = useState("PRIVATE");
  const [state, setState] = useState<"loading" | "idle" | "saving">("loading");
  const [msg, setMsg] = useState("");
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
          <Button variant="default" size="lg" disabled={state === "saving" || !name.trim()} onClick={() => void save()} className="self-start max-md:min-h-11"><Save className="size-4" strokeWidth={1.5} aria-hidden />{state === "saving" ? "Saving…" : "Save game page"}</Button>
          <p className="text-sm text-ink-2">The game icon, thumbnails, age questionnaire and payments policy are not available to OpenRO. Set those in the Roblox Creator Dashboard.</p>
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
      <MorphingDialogTrigger thing="set-project-ids" className="inline-flex min-h-11 cursor-pointer items-center rounded-lg px-2 text-sm font-medium text-ink outline-none hover:bg-track focus-visible:ring-2 focus-visible:ring-focus md:min-h-8">
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
  useEffect(() => { setPublished(null); setRows(null); setError(""); }, [active?.id]);
  if (!active) return null;

  const products: Product[] = detail?.manifest?.products ?? detail?.products ?? [];
  const hasKey = !!cloud?.has_key;
  const hasIds = !!(active.universeId && active.placeId);
  const hasBuild = !!(build ?? active.last_build);
  const ready = [hasKey, hasIds, hasBuild].filter(Boolean).length;
  const canPublish = hasKey && hasIds && hasBuild && !publishing;
  const pending = products.filter((p) => p.priceApproved !== true).length;

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
      <section aria-label="Ready to publish" className="card-stock flex flex-col gap-1 p-4">
        <div className="flex items-center gap-3">
          <ProgressCircle value={Math.round((ready / 3) * 100)} radius={15} strokeWidth={3.5} className="size-9 flex-none">
            <span className="font-mono text-sm font-medium tabular-nums text-ink">{ready}/3</span>
          </ProgressCircle>
          <div className="min-w-0">
            <h3 className="font-heading text-ui font-semibold">Publish {active.name}</h3>
            <p className="text-sm text-ink-2">{ready === 3 ? "Everything is in place." : "A few things are missing before this can go to Roblox."}</p>
          </div>
        </div>
        <ul className="mt-1 flex flex-col">
          <Check1 ok={hasKey} label="Open Cloud key saved" fix="Add it in Settings" onFix={onOpenSettings} />
          <li className="flex min-h-11 items-center gap-2 text-sm">
            <span className={`grid size-6 flex-none place-items-center rounded-full ${hasIds ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-warn-bg text-pill-warn-fg"}`}>
              {hasIds ? <Check className="size-3.5" strokeWidth={2} aria-hidden /> : <CircleAlert className="size-3.5" strokeWidth={2} aria-hidden />}
            </span>
            <span className="min-w-0 flex-1 text-ink">Game number and place number set</span>
            {!hasIds && <SetProjectIdsDialog active={active} ship={ship} />}
          </li>
          <Check1 ok={hasBuild} label="A build exists" />
        </ul>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <SegmentedControl aria-label="Publish kind" size="sm">
            <SegmentedControlItem data-checked={!live ? "" : undefined} aria-pressed={!live} onClick={() => setLive(false)} className="max-md:!h-11 max-md:px-3">Save version</SegmentedControlItem>
            <SegmentedControlItem data-checked={live ? "" : undefined} aria-pressed={live} onClick={() => setLive(true)} className="max-md:!h-11 max-md:px-3">Publish live</SegmentedControlItem>
          </SegmentedControl>
          {live ? (
            <MorphingDialog transition={dialogSpring} open={dialogOpen} onOpenChange={setDialogOpen}>
              <MorphingDialogTrigger blocked={!canPublish} className="inline-flex min-h-11 items-center justify-center gap-2 rounded-lg border border-primary bg-primary px-4 text-sm font-medium text-primary-foreground outline-none hover:bg-primary/90 focus-visible:ring-2 focus-visible:ring-focus disabled:border-hairline disabled:bg-track disabled:text-ink-3" thing="publish">
                <Rocket className="size-4" strokeWidth={1.5} aria-hidden />Publish
              </MorphingDialogTrigger>
              <MorphingDialogContainer>
                <MorphingDialogContent className="relative mx-3 flex w-full max-w-[440px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                  <div>
                    <MorphingDialogTitle className="font-heading text-title font-semibold">Publish live?</MorphingDialogTitle>
                    <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Players get this version right away. Only continue if you or the owner said to go live.</MorphingDialogDescription>
                  </div>
                  <div className="flex flex-wrap justify-end gap-2">
                    <MorphingDialogClose aria-label="Not yet" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Not yet</MorphingDialogClose>
                    <Button variant="default" size="lg" disabled={publishing} onClick={() => void publish()} className="max-md:min-h-11">{publishing ? "Publishing…" : "Yes, publish live"}</Button>
                  </div>
                </MorphingDialogContent>
              </MorphingDialogContainer>
            </MorphingDialog>
          ) : (
            <Button variant="default" size="lg" disabled={!canPublish} onClick={() => void publish()} data-publish className="max-md:min-h-11"><Rocket className="size-4" strokeWidth={1.5} aria-hidden />{publishing ? "Saving…" : "Save to Roblox"}</Button>
          )}
        </div>
        {!hasKey && <p className="mt-1 inline-flex items-center gap-1.5 text-sm text-ink-2"><KeyRound className="size-3.5" strokeWidth={1.5} aria-hidden />The button turns on once the key is saved.</p>}
        {error && <p role="alert" className="mt-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
        {published && <p className="mt-2 rounded-sm bg-pill-active-bg px-3 py-2 text-sm text-pill-active-fg" data-published>Version <span className="font-mono">{published.versionNumber}</span> is on Roblox ({published.versionType === "Published" ? "live" : "saved, not live"}){published.bytes ? ` · ${prettyBytes(published.bytes)}` : ""}.</p>}
      </section>

      <section aria-label="Game passes and products" className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="font-heading text-ui font-semibold">Game passes and products <span className="font-mono text-sm font-normal text-ink-2">{products.length}</span></h3>
          <div className="flex flex-wrap gap-2">
            {pending > 0 && <Button variant="outline" size="sm" onClick={() => approve(products.filter((p) => p.priceApproved !== true).map((p) => p.key))} className="max-md:min-h-11">Approve all {pending} prices</Button>}
            <Button variant="default" size="sm" disabled={!hasKey || !active.universeId || syncing || products.length === 0 || pending === products.length} onClick={() => void syncProducts()} className="max-md:min-h-11">{syncing ? "Sending…" : "Send to Roblox"}</Button>
          </div>
        </div>
        {products.length === 0 ? (
          <p className="card-stock p-4 text-sm text-ink-2">No products in this project yet. They are listed in <code className="font-mono">openro.project.json</code>.</p>
        ) : (
          <ul className="flex flex-col gap-2">{products.map((p) => <ProductRow key={p.key} p={p} canApprove onApprove={() => approve([p.key])} />)}</ul>
        )}
        {pending > 0 && <p className="text-sm text-ink-2">Prices stay pending until you approve them. OpenRO does not send an unapproved price.</p>}
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
