import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { Check, ImagePlus, RefreshCw, Search, ShieldCheck, Sparkles, Upload, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogTitle,
  MorphingDialogTrigger,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, enc, projUrl } from "./api";
import { Field } from "./ProjectsRail";
import type { Ship } from "./useShip";

type RefCandidate = {
  index: number;
  path?: string;
  url?: string;
  title?: string;
  width?: number;
  height?: number;
  score?: number;
  reasons?: string[];
  rejected_reason?: string | null;
};

type RefRecord = {
  slug: string;
  character?: string;
  show?: string;
  chosen_index?: number;
  reference_path?: string;
  url?: string;
  candidates?: RefCandidate[];
};

type PendingReviewItem = {
  candidate_path: string;
  candidate_url?: string;
  candidate_sha256?: string;
  requested_path?: string;
  reference_images?: string[];
  reference_urls?: string[];
  angle_review?: { reason?: string; details?: string };
};

type ArtItem = {
  path?: string;
  kind?: string;
  status?: string;
  width?: number;
  height?: number;
  prompt?: string;
  sha256?: string;
};

type ArtGenReply = {
  status?: "generated" | "pending_review" | "refused";
  pending_review?: boolean;
  refused?: boolean;
  moderation_reason?: string;
  path?: string;
  url?: string;
  candidate_path?: string;
  candidate_url?: string;
  candidate_sha256?: string;
  requested_path?: string;
  reference_images?: string[];
  reference_urls?: string[];
};

type ArtTab = "cards" | "weapons" | "store" | "refs";

const imgSrc = (project: string, path?: string, directUrl?: string) =>
  directUrl || (path ? `/api/art/image?project=${enc(project)}&path=${enc(path)}` : "");

function artTabFor(path?: string, kind?: string): Exclude<ArtTab, "refs"> {
  const p = (path || "").toLowerCase();
  const k = (kind || "").toLowerCase();
  if (p.includes("/weapons/") || k.includes("weapon")) return "weapons";
  if (p.includes("/store/") || p.includes("/ui/") || p.includes("/worlds/") || ["icon", "frame", "store", "world"].includes(k)) {
    return "store";
  }
  return "cards";
}

/** Visual-first art & reference studio: category tabs (Cards | Weapon Sheets | Store | Refs), thumbnail tiles, 1-click Approve/Redraw/Pack. */
export function ArtStudioPanel({ ship }: { ship: Ship }) {
  const project = ship.active?.id;
  const [tab, setTab] = useState<ArtTab>("cards");
  const [refs, setRefs] = useState<RefRecord[]>([]);
  const [items, setItems] = useState<ArtItem[]>([]);
  const [pending, setPending] = useState<PendingReviewItem[]>([]);
  const [searching, setSearching] = useState(false);
  const [character, setCharacter] = useState("");
  const [show, setShow] = useState("");
  const [qualifiers, setQualifiers] = useState("");

  const [genProvider, setGenProvider] = useState<"chatgpt" | "gemini" | "openai">("chatgpt");
  const [storeKind, setStoreKind] = useState<"icon" | "frame" | "store" | "world">("store");
  const [charArt, setCharArt] = useState(true);
  const [selectedRef, setSelectedRef] = useState("");
  const [prompt, setPrompt] = useState("");
  const [generating, setGenerating] = useState(false);
  const [lastResult, setLastResult] = useState<ArtGenReply | null>(null);
  const [reviewer, setReviewer] = useState("owner");
  const [notes, setNotes] = useState("");
  const [error, setError] = useState("");
  const [packingPath, setPackingPath] = useState<string | null>(null);
  const [packedPaths, setPackedPaths] = useState<Record<string, boolean>>({});
  const promptRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    if (!project) return;
    const [r1, r2] = await Promise.all([
      call<{ references?: RefRecord[] }>(`/api/art/references?project=${enc(project)}`),
      call<{ items?: ArtItem[]; pending_review?: PendingReviewItem[] }>(`/api/art?project=${enc(project)}`),
    ]);
    if (r1.ok) {
      const list = r1.references ?? [];
      setRefs(list);
      if (!selectedRef && list[0]?.reference_path) setSelectedRef(list[0].reference_path);
    }
    if (r2.ok) {
      setItems(r2.items ?? []);
      setPending(r2.pending_review ?? []);
    }
  }, [project, selectedRef]);

  useEffect(() => {
    setRefs([]);
    setItems([]);
    setPending([]);
    setLastResult(null);
    setError("");
    void load();
  }, [load]);

  const tabItems = useMemo(() => {
    if (tab === "refs") return [];
    return items.filter((it) => artTabFor(it.path, it.kind) === tab);
  }, [items, tab]);

  const tabPending = useMemo(() => {
    if (tab === "refs") return [];
    return pending.filter((p) => artTabFor(p.requested_path || p.candidate_path) === tab);
  }, [pending, tab]);

  if (!project) return null;

  const activeKind = tab === "weapons" ? "weapon_sheet" : tab === "store" ? storeKind : "card";

  const runSearch = async () => {
    if (!character.trim() || !show.trim()) return;
    setSearching(true);
    setError("");
    const r = await call<RefRecord>("/api/art/references/search", {
      project,
      character: character.trim(),
      show: show.trim(),
      qualifiers: qualifiers.trim() || undefined,
      auto_pick: true,
    });
    setSearching(false);
    if (!r.ok) {
      setError(r.error || "Reference search did not finish.");
      return;
    }
    if (r.reference_path) setSelectedRef(r.reference_path);
    toast.success(`Found ${r.candidates?.length ?? 0} candidate(s)`);
    await load();
  };

  const pickCandidate = async (slug: string, candidateIndex: number) => {
    const r = await call<{ reference_path?: string }>("/api/art/references/select", {
      project,
      slug,
      candidate_index: candidateIndex,
    });
    if (!r.ok) {
      toast.error(r.error || "Could not switch reference.");
      return;
    }
    if (r.reference_path) setSelectedRef(r.reference_path);
    toast.success("Reference updated");
    await load();
  };

  const generate = async () => {
    if (!prompt.trim()) return;
    setGenerating(true);
    setError("");
    setLastResult(null);
    const r = await call<ArtGenReply>("/api/art/generate", {
      project,
      provider: genProvider,
      kind: activeKind,
      character_art: tab === "cards" ? charArt : false,
      prompt: prompt.trim(),
      reference_images: selectedRef ? [selectedRef] : undefined,
    });
    setGenerating(false);
    setLastResult(r as ArtGenReply);
    if (!r.ok && !r.refused) {
      setError(r.error || "Art generation failed.");
      return;
    }
    if (r.refused) toast.error(r.moderation_reason || "Generation was refused by moderation.");
    else if (r.pending_review || r.status === "pending_review") toast("Angle review required before saving");
    else toast.success("Art generated");
    await load();
  };

  const approvePending = async (item: PendingReviewItem, verdict: "pass" | "redo") => {
    const r = await call("/api/art/approve", {
      project,
      candidate_path: item.candidate_path,
      target_path: item.requested_path,
      expected_sha256: item.candidate_sha256,
      reviewer: reviewer.trim() || "owner",
      verdict,
      notes: notes.trim() || undefined,
    });
    if (!r.ok) {
      toast.error(r.error || "Could not record review.");
      return;
    }
    toast.success(verdict === "pass" ? "Approved and promoted" : "Marked for redraw");
    setNotes("");
    await load();
  };

  const approveExisting = async (artPath: string) => {
    const r = await call("/api/art/approve", {
      project,
      candidate_path: artPath,
      target_path: artPath,
      reviewer: reviewer.trim() || "owner",
      verdict: "pass",
    });
    if (!r.ok) {
      toast.error(r.error || "Could not approve artwork.");
      return;
    }
    toast.success(`Approved ${artPath.split("/").pop() || "artwork"}`);
    await load();
  };

  const packAsset = async (artPath: string) => {
    const rel = artPath.replace(/^art\//, "assets/");
    setPackingPath(artPath);
    const r = await call(projUrl(project, "/assets/authorise"), { paths: [artPath, rel], revoke: false });
    setPackingPath(null);
    if (!r.ok) {
      toast.error(r.error || "Could not mark asset for upload.");
      return;
    }
    setPackedPaths((p) => ({ ...p, [artPath]: true }));
    toast.success("Marked ready for upload");
    await ship.refresh();
  };

  const triggerRedraw = (item: ArtItem) => {
    const base = item.prompt || `Redraw ${item.path || "asset"} matching canonical colors and framing.`;
    setPrompt(base);
    promptRef.current?.focus();
  };

  const counts = {
    cards: items.filter((i) => artTabFor(i.path, i.kind) === "cards").length + pending.filter((p) => artTabFor(p.requested_path || p.candidate_path) === "cards").length,
    weapons: items.filter((i) => artTabFor(i.path, i.kind) === "weapons").length + pending.filter((p) => artTabFor(p.requested_path || p.candidate_path) === "weapons").length,
    store: items.filter((i) => artTabFor(i.path, i.kind) === "store").length + pending.filter((p) => artTabFor(p.requested_path || p.candidate_path) === "store").length,
    refs: refs.length,
  };

  return (
    <div className="flex flex-col gap-4" data-panel="art-studio">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <SegmentedControl aria-label="Art studio category" size="sm" data-art-tabs className="max-w-full flex-wrap">
          {(
            [
              { id: "cards", label: "Cards", count: counts.cards },
              { id: "weapons", label: "Weapon Sheets", count: counts.weapons },
              { id: "store", label: "Store", count: counts.store },
              { id: "refs", label: "Refs", count: counts.refs },
            ] as const
          ).map((t) => (
            <SegmentedControlItem
              key={t.id}
              data-checked={tab === t.id ? "" : undefined}
              aria-pressed={tab === t.id}
              onClick={() => setTab(t.id)}
              className="gap-1.5 max-md:!h-11 max-md:px-2.5"
            >
              <span>{t.label}</span>
              <span className="font-mono text-xs text-ink-2">{t.count}</span>
            </SegmentedControlItem>
          ))}
        </SegmentedControl>
        <Button variant="ghost" size="icon" aria-label="Reload art" onClick={() => void load()} className="active:opacity-80 max-md:size-11">
          <RefreshCw className="size-4" strokeWidth={1.5} aria-hidden />
        </Button>
      </div>

      {error && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}

      {tab === "refs" ? (
        <section aria-label="Reference image search" className="card-stock flex flex-col gap-3 p-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <h3 className="font-heading text-ui font-semibold">Reference search</h3>
              <p className="text-sm text-ink-2">Find clean character or prop references in the shared browser.</p>
            </div>
          </div>
          <div className="grid gap-2 sm:grid-cols-3">
            <Field label="Character or weapon"><Input value={character} onChange={(e) => setCharacter(e.target.value)} placeholder="Gojo Satoru" /></Field>
            <Field label="Show or universe"><Input value={show} onChange={(e) => setShow(e.target.value)} placeholder="Jujutsu Kaisen" /></Field>
            <Field label="Qualifiers (optional)"><Input value={qualifiers} onChange={(e) => setQualifiers(e.target.value)} placeholder="hollow purple battle" /></Field>
          </div>
          <Button variant="default" size="lg" disabled={!character.trim() || !show.trim() || searching} onClick={() => void runSearch()} className="self-start active:opacity-80 disabled:opacity-50 max-md:min-h-11">
            <Search className="size-4" strokeWidth={1.5} aria-hidden />{searching ? "Searching…" : "Search references"}
          </Button>

          {refs.length > 0 && (
            <div className="flex flex-col gap-3 border-t border-hairline pt-3">
              {refs.map((ref) => (
                <div key={ref.slug} className="flex flex-col gap-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="text-sm font-medium text-ink">{ref.character || ref.slug} {ref.show ? `(${ref.show})` : ""}</span>
                    {ref.reference_path && <code className="truncate font-mono text-xs text-ink-2">{ref.reference_path}</code>}
                  </div>
                  {ref.candidates && ref.candidates.length > 0 && (
                    <ul className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
                      {ref.candidates.map((c, idx) => {
                        const active = c.index === ref.chosen_index;
                        return (
                          <li key={c.index} className={`flex flex-col justify-between gap-1.5 rounded-sm border p-2 ${active ? "border-blue bg-blue-soft/40" : "border-hairline bg-board/40"}`}>
                            {c.path || c.url ? (
                              <img
                                src={imgSrc(project, c.path, c.url)}
                                alt={c.title || `Candidate ${c.index}`}
                                loading={idx === 0 ? "eager" : "lazy"}
                                fetchPriority={idx === 0 ? "high" : undefined}
                                className="aspect-square w-full rounded-xs object-cover"
                              />
                            ) : null}
                            <div className="flex items-center justify-between gap-1 text-xs text-ink-2">
                              <span>score <span className="font-mono tabular-nums">{c.score ?? 0}</span></span>
                              {c.width && c.height ? <span><span className="font-mono tabular-nums">{c.width}</span>x<span className="font-mono tabular-nums">{c.height}</span></span> : null}
                            </div>
                            {c.rejected_reason && <span className="truncate text-xs text-pill-pro-fg">{c.rejected_reason}</span>}
                            <Button variant={active ? "secondary" : "outline"} size="xs" onClick={() => void pickCandidate(ref.slug, c.index)} className="w-full active:opacity-80 max-md:min-h-11">
                              {active && <Check className="size-3" strokeWidth={2} aria-hidden />}
                              {active ? "Selected" : "Use reference"}
                            </Button>
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      ) : (
        <>
          {/* Pending Angle Review for active category */}
          {tabPending.length > 0 && (
            <section aria-label="Pending angle review" className="card-stock flex flex-col gap-3 p-4" data-pending-review>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="font-heading text-ui font-semibold">Pending review</h3>
                <Pill className="bg-pill-warn-bg px-2 py-0.5 text-sm text-pill-warn-fg">
                  <span className="font-mono tabular-nums">{tabPending.length}</span> awaiting review
                </Pill>
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <Field label="Reviewer"><Input value={reviewer} onChange={(e) => setReviewer(e.target.value)} /></Field>
                <Field label="Notes (optional)"><Input value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Framing matches reference" /></Field>
              </div>
              <ul className="grid gap-3 md:grid-cols-2">
                {tabPending.map((item) => (
                  <li key={item.candidate_path} className="rounded-sm border border-hairline bg-board/50 p-3" data-review-item={item.candidate_path}>
                    <div className="grid grid-cols-2 gap-2.5">
                      <div className="flex flex-col gap-1">
                        <span className="text-xs font-medium text-ink-2">Reference</span>
                        {item.reference_images?.[0] || item.reference_urls?.[0] ? (
                          <img src={imgSrc(project, item.reference_images?.[0], item.reference_urls?.[0])} alt="Reference image" className="aspect-square w-full rounded-sm border border-hairline bg-surface object-contain" />
                        ) : (
                          <div className="grid aspect-square place-items-center rounded-sm border border-hairline bg-surface text-xs text-ink-2">No ref</div>
                        )}
                      </div>
                      <div className="flex flex-col gap-1">
                        <span className="text-xs font-medium text-ink-2">Candidate</span>
                        <img src={imgSrc(project, item.candidate_path, item.candidate_url)} alt="Generated candidate" className="aspect-square w-full rounded-sm border border-hairline bg-surface object-contain" />
                      </div>
                    </div>
                    <div className="mt-2.5 flex flex-wrap items-center justify-between gap-2">
                      <code className="min-w-0 truncate font-mono text-xs text-ink-2">{item.requested_path || item.candidate_path}</code>
                      <div className="flex flex-wrap gap-1.5">
                        <Button variant="outline" size="xs" onClick={() => void approvePending(item, "redo")} className="active:opacity-80 max-md:min-h-11">
                          <XCircle className="size-3.5" strokeWidth={1.5} aria-hidden />Redraw
                        </Button>
                        <Button variant="default" size="xs" onClick={() => void approvePending(item, "pass")} className="active:opacity-80 max-md:min-h-11">
                          <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden />Approve
                        </Button>
                      </div>
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {/* Gallery Tiles for Active Category */}
          {tabItems.length > 0 && (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3" data-art-gallery>
              {tabItems.map((it, idx) => {
                const artPath = it.path || "";
                const name = artPath.split("/").pop() || "Artwork";
                const src = imgSrc(project, artPath);
                const approved = it.status === "approved";
                const packed = Boolean(packedPaths[artPath]);

                return (
                  <li key={artPath || idx} className="card-stock flex flex-col justify-between gap-2 p-2.5" data-art-item={artPath}>
                    {src && (
                      <MorphingDialog transition={dialogSpring}>
                        <MorphingDialogTrigger
                          aria-label={`Preview ${name}`}
                          className="block w-full overflow-hidden rounded-sm border border-hairline bg-board outline-none active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus"
                        >
                          <img
                            src={src}
                            alt={name}
                            loading={idx === 0 ? "eager" : "lazy"}
                            fetchPriority={idx === 0 ? "high" : undefined}
                            className="aspect-[4/3] w-full object-cover"
                          />
                        </MorphingDialogTrigger>
                        <MorphingDialogContainer>
                          <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[780px] flex-col gap-3 rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                            <MorphingDialogTitle className="font-heading text-ui font-semibold">{name}</MorphingDialogTitle>
                            <img src={src} alt={name} className="min-h-0 w-full flex-1 rounded-sm object-contain" />
                            <code className="truncate font-mono text-xs text-ink-3">{artPath}</code>
                            <MorphingDialogClose aria-label="Close preview" className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Close</MorphingDialogClose>
                          </MorphingDialogContent>
                        </MorphingDialogContainer>
                      </MorphingDialog>
                    )}

                    <div className="flex min-w-0 flex-col gap-1">
                      <div className="flex items-center justify-between gap-1.5">
                        <span className="truncate text-sm font-medium text-ink" title={artPath}>{name}</span>
                        {it.width && it.height ? (
                          <span className="shrink-0 text-xs text-ink-3"><span className="font-mono tabular-nums">{it.width}</span>x<span className="font-mono tabular-nums">{it.height}</span></span>
                        ) : null}
                      </div>
                      <code className="truncate font-mono text-xs text-ink-3">{artPath}</code>
                    </div>

                    <div className="flex flex-wrap items-center gap-1.5 border-t border-hairline pt-2">
                      {approved ? (
                        <Pill className="bg-pill-active-bg px-1.5 py-0 text-xs text-pill-active-fg">Approved</Pill>
                      ) : (
                        <Button variant="default" size="xs" onClick={() => void approveExisting(artPath)} className="active:opacity-80 max-md:min-h-11">
                          <Check className="size-3" strokeWidth={1.5} aria-hidden />Approve
                        </Button>
                      )}
                      <Button variant="outline" size="xs" onClick={() => triggerRedraw(it)} className="active:opacity-80 max-md:min-h-11">
                        <RefreshCw className="size-3" strokeWidth={1.5} aria-hidden />Redraw
                      </Button>
                      <Button
                        variant={packed ? "secondary" : "outline"}
                        size="xs"
                        disabled={packingPath === artPath}
                        onClick={() => void packAsset(artPath)}
                        className="active:opacity-80 disabled:opacity-50 max-md:min-h-11"
                      >
                        <Upload className="size-3" strokeWidth={1.5} aria-hidden />{packed ? "Packed" : "Pack"}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}

          {/* Compact Generator Bar */}
          <section aria-label="Generate art" className="card-stock flex flex-col gap-3 p-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <Sparkles className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                <h3 className="font-heading text-ui font-semibold">
                  {tab === "weapons" ? "Generate weapon sheet" : tab === "store" ? "Generate store art" : "Generate card art"}
                </h3>
              </div>
              <SegmentedControl aria-label="Art provider" size="sm">
                {(["chatgpt", "gemini", "openai"] as const).map((p) => (
                  <SegmentedControlItem key={p} data-checked={genProvider === p ? "" : undefined} aria-pressed={genProvider === p} onClick={() => setGenProvider(p)} className="max-md:!h-11">
                    {p === "chatgpt" ? "ChatGPT" : p === "gemini" ? "Gemini" : "OpenAI"}
                  </SegmentedControlItem>
                ))}
              </SegmentedControl>
            </div>

            {tab === "store" && (
              <SegmentedControl aria-label="Store art kind" size="sm" className="self-start">
                {(["icon", "frame", "store", "world"] as const).map((k) => (
                  <SegmentedControlItem key={k} data-checked={storeKind === k ? "" : undefined} aria-pressed={storeKind === k} onClick={() => setStoreKind(k)} className="max-md:!h-11">
                    {k}
                  </SegmentedControlItem>
                ))}
              </SegmentedControl>
            )}

            {tab === "cards" && (
              <Button variant={charArt ? "secondary" : "outline"} size="sm" aria-pressed={charArt} onClick={() => setCharArt((v) => !v)} className="self-start active:opacity-80 max-md:min-h-11">
                Character framing check
              </Button>
            )}

            <div className="grid gap-2 sm:grid-cols-2">
              <Field label="Reference image path (optional)">
                <Input value={selectedRef} onChange={(e) => setSelectedRef(e.target.value)} className="font-mono" placeholder="art/references/gojo/chosen.png" />
              </Field>
              <Field label="Composition prompt">
                <Input ref={promptRef} value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder={tab === "weapons" ? "Orthographic weapon sheet on dark slate background" : "Eye-level three-quarter action pose, anime cel shading"} />
              </Field>
            </div>

            <Button variant="default" size="lg" disabled={!prompt.trim() || generating} onClick={() => void generate()} className="self-start active:opacity-80 disabled:opacity-50 max-md:min-h-11">
              <ImagePlus className="size-4" strokeWidth={1.5} aria-hidden />{generating ? "Generating…" : "Generate"}
            </Button>

            {lastResult?.refused && (
              <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
                Refused by moderation: {lastResult.moderation_reason || "Adjust prompt and retry."}
              </p>
            )}
            {lastResult?.status === "generated" && (lastResult.path || lastResult.url) && (
              <div className="flex items-center gap-3 rounded-sm border border-hairline bg-board/40 p-2.5">
                <img src={imgSrc(project, lastResult.path, lastResult.url)} alt="Generated art" className="size-16 rounded-xs object-cover" />
                <code className="min-w-0 flex-1 truncate font-mono text-xs text-ink">{lastResult.path}</code>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}
