import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Check, ImagePlus, RefreshCw, Search, ShieldCheck, Sparkles, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { call, enc } from "./api";
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

const imgSrc = (project: string, path?: string, directUrl?: string) =>
  directUrl || (path ? `/api/art/image?project=${enc(project)}&path=${enc(path)}` : "");

/** Reference search (shared Camoufox), ChatGPT default reference-image generation, and side-by-side angle review. */
export function ArtStudioPanel({ ship }: { ship: Ship }) {
  const project = ship.active?.id;
  const [refs, setRefs] = useState<RefRecord[]>([]);
  const [pending, setPending] = useState<PendingReviewItem[]>([]);
  const [searching, setSearching] = useState(false);
  const [character, setCharacter] = useState("");
  const [show, setShow] = useState("");
  const [qualifiers, setQualifiers] = useState("");

  const [genProvider, setGenProvider] = useState<"chatgpt" | "gemini" | "openai">("chatgpt");
  const [kind, setKind] = useState<"card" | "icon" | "frame" | "store" | "world">("card");
  const [charArt, setCharArt] = useState(true);
  const [selectedRef, setSelectedRef] = useState("");
  const [prompt, setPrompt] = useState("");
  const [generating, setGenerating] = useState(false);
  const [lastResult, setLastResult] = useState<ArtGenReply | null>(null);
  const [reviewer, setReviewer] = useState("owner");
  const [notes, setNotes] = useState("");
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    if (!project) return;
    const [r1, r2] = await Promise.all([
      call<{ references?: RefRecord[] }>(`/api/art/references?project=${enc(project)}`),
      call<{ pending_review?: PendingReviewItem[] }>(`/api/art?project=${enc(project)}`),
    ]);
    if (r1.ok) {
      const list = r1.references ?? [];
      setRefs(list);
      if (!selectedRef && list[0]?.reference_path) setSelectedRef(list[0].reference_path);
    }
    if (r2.ok) setPending(r2.pending_review ?? []);
  }, [project, selectedRef]);

  useEffect(() => { setRefs([]); setPending([]); setLastResult(null); setError(""); void load(); }, [load]);

  if (!project) return null;

  const runSearch = async () => {
    if (!character.trim() || !show.trim()) return;
    setSearching(true); setError("");
    const r = await call<RefRecord>("/api/art/references/search", {
      project,
      character: character.trim(),
      show: show.trim(),
      qualifiers: qualifiers.trim() || undefined,
      auto_pick: true,
    });
    setSearching(false);
    if (!r.ok) { setError(r.error || "Reference search did not finish."); return; }
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
    if (!r.ok) { toast.error(r.error || "Could not switch reference."); return; }
    if (r.reference_path) setSelectedRef(r.reference_path);
    toast.success("Reference updated");
    await load();
  };

  const generate = async () => {
    if (!prompt.trim()) return;
    setGenerating(true); setError(""); setLastResult(null);
    const r = await call<ArtGenReply>("/api/art/generate", {
      project,
      provider: genProvider,
      kind,
      character_art: charArt,
      prompt: prompt.trim(),
      reference_images: selectedRef ? [selectedRef] : undefined,
    });
    setGenerating(false);
    setLastResult(r as ArtGenReply);
    if (!r.ok && !r.refused) { setError(r.error || "Art generation failed."); return; }
    if (r.refused) toast.error(r.moderation_reason || "Generation was refused by moderation.");
    else if (r.pending_review || r.status === "pending_review") toast("Angle review required before saving");
    else toast.success("Art generated");
    await load();
  };

  const approve = async (item: PendingReviewItem, verdict: "pass" | "redo") => {
    const r = await call("/api/art/approve", {
      project,
      candidate_path: item.candidate_path,
      target_path: item.requested_path,
      expected_sha256: item.candidate_sha256,
      reviewer: reviewer.trim() || "owner",
      verdict,
      notes: notes.trim() || undefined,
    });
    if (!r.ok) { toast.error(r.error || "Could not record review."); return; }
    toast.success(verdict === "pass" ? "Approved and promoted" : "Marked for redraw");
    setNotes("");
    await load();
  };

  return (
    <div className="flex flex-col gap-4" data-panel="art-studio">
      {error && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}

      {/* 1. Pending Side-by-Side Pixel Review (gap #14) */}
      {pending.length > 0 && (
        <section aria-label="Pending angle review" className="card-stock flex flex-col gap-3 p-4" data-pending-review>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h3 className="font-heading text-ui font-semibold">Pending angle review</h3>
            <Pill className="bg-pill-warn-bg px-2 py-0.5 text-sm text-pill-warn-fg">
              <span className="font-mono tabular-nums">{pending.length}</span> awaiting review
            </Pill>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Reviewer"><Input value={reviewer} onChange={(e) => setReviewer(e.target.value)} /></Field>
            <Field label="Notes (optional)"><Input value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Eye-level framing matches reference" /></Field>
          </div>
          <ul className="flex flex-col gap-3">
            {pending.map((item) => (
              <li key={item.candidate_path} className="rounded-sm border border-hairline bg-board/50 p-3" data-review-item={item.candidate_path}>
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="flex flex-col gap-1">
                    <span className="text-xs font-medium text-ink-2">Reference</span>
                    {item.reference_images?.[0] || item.reference_urls?.[0] ? (
                      <img src={imgSrc(project, item.reference_images?.[0], item.reference_urls?.[0])} alt="Reference image" className="aspect-square w-full rounded-sm border border-hairline bg-surface object-contain" />
                    ) : (
                      <div className="grid aspect-square place-items-center rounded-sm border border-hairline bg-surface text-sm text-ink-2">No reference attached</div>
                    )}
                  </div>
                  <div className="flex flex-col gap-1">
                    <span className="text-xs font-medium text-ink-2">Candidate</span>
                    <img src={imgSrc(project, item.candidate_path, item.candidate_url)} alt="Generated candidate" className="aspect-square w-full rounded-sm border border-hairline bg-surface object-contain" />
                  </div>
                </div>
                <div className="mt-2.5 flex flex-wrap items-center justify-between gap-2">
                  <code className="min-w-0 truncate font-mono text-xs text-ink-2">{item.requested_path || item.candidate_path}</code>
                  <div className="flex flex-wrap gap-2">
                    <Button variant="outline" size="sm" onClick={() => void approve(item, "redo")} className="max-md:min-h-11">
                      <XCircle className="size-3.5" strokeWidth={1.5} aria-hidden />Redraw
                    </Button>
                    <Button variant="default" size="sm" onClick={() => void approve(item, "pass")} className="max-md:min-h-11">
                      <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden />Approve &amp; promote
                    </Button>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* 2. Reference Search (Shared Camoufox Browser) */}
      <section aria-label="Reference image search" className="card-stock flex flex-col gap-3 p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <h3 className="font-heading text-ui font-semibold">Reference search</h3>
            <p className="text-sm text-ink-2">Finds dynamic action scenes via the shared browser and ranks out static sheets.</p>
          </div>
          <Button variant="ghost" size="icon" aria-label="Reload references" onClick={() => void load()} className="max-md:size-11"><RefreshCw className="size-4" strokeWidth={1.5} aria-hidden /></Button>
        </div>
        <div className="grid gap-2 sm:grid-cols-3">
          <Field label="Character"><Input value={character} onChange={(e) => setCharacter(e.target.value)} placeholder="Gojo Satoru" /></Field>
          <Field label="Show"><Input value={show} onChange={(e) => setShow(e.target.value)} placeholder="Jujutsu Kaisen" /></Field>
          <Field label="Qualifiers (optional)"><Input value={qualifiers} onChange={(e) => setQualifiers(e.target.value)} placeholder="hollow purple battle" /></Field>
        </div>
        <Button variant="default" size="lg" disabled={!character.trim() || !show.trim() || searching} onClick={() => void runSearch()} className="self-start max-md:min-h-11">
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
                  <ul className="grid grid-cols-[repeat(auto-fill,minmax(148px,1fr))] gap-2">
                    {ref.candidates.map((c) => {
                      const active = c.index === ref.chosen_index;
                      return (
                        <li key={c.index} className={`flex flex-col justify-between gap-1.5 rounded-sm border p-2 ${active ? "border-blue bg-blue-soft/40" : "border-hairline bg-board/40"}`}>
                          {c.path || c.url ? (
                            <img src={imgSrc(project, c.path, c.url)} alt={c.title || `Candidate ${c.index}`} loading="lazy" className="aspect-square w-full rounded-xs object-cover" />
                          ) : null}
                          <div className="flex items-center justify-between gap-1 text-xs text-ink-2">
                            <span>score <span className="font-mono tabular-nums">{c.score ?? 0}</span></span>
                            {c.width && c.height ? <span><span className="font-mono tabular-nums">{c.width}</span>×<span className="font-mono tabular-nums">{c.height}</span></span> : null}
                          </div>
                          {c.rejected_reason && <span className="truncate text-xs text-pill-pro-fg">{c.rejected_reason}</span>}
                          <Button variant={active ? "secondary" : "outline"} size="xs" onClick={() => void pickCandidate(ref.slug, c.index)} className="w-full max-md:min-h-11">
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

      {/* 3. Generate Reference-Image Art */}
      <section aria-label="Generate art" className="card-stock flex flex-col gap-3 p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-semibold">Generate card &amp; UI art</h3>
          </div>
          <SegmentedControl aria-label="Art provider" size="sm">
            {(["chatgpt", "gemini", "openai"] as const).map((p) => (
              <SegmentedControlItem key={p} data-checked={genProvider === p ? "" : undefined} aria-pressed={genProvider === p} onClick={() => setGenProvider(p)} className="max-md:!h-11">
                {p === "chatgpt" ? "ChatGPT" : p === "gemini" ? "Gemini" : "OpenAI"}
              </SegmentedControlItem>
            ))}
          </SegmentedControl>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <SegmentedControl aria-label="Art kind" size="sm">
            {(["card", "icon", "frame", "store", "world"] as const).map((k) => (
              <SegmentedControlItem key={k} data-checked={kind === k ? "" : undefined} aria-pressed={kind === k} onClick={() => setKind(k)} className="max-md:!h-11">
                {k}
              </SegmentedControlItem>
            ))}
          </SegmentedControl>
          <Button variant={charArt ? "secondary" : "outline"} size="sm" aria-pressed={charArt} onClick={() => setCharArt((v) => !v)} className="max-md:min-h-11">
            Character framing check
          </Button>
        </div>

        <Field label="Reference image path (optional)">
          <Input value={selectedRef} onChange={(e) => setSelectedRef(e.target.value)} className="font-mono" placeholder="art/references/gojo/chosen.png" />
        </Field>
        <Field label="Composition prompt">
          <Input value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="Eye-level three-quarter action pose, anime cel shading, energy aura" />
        </Field>
        <Button variant="default" size="lg" disabled={!prompt.trim() || generating} onClick={() => void generate()} className="self-start max-md:min-h-11">
          <ImagePlus className="size-4" strokeWidth={1.5} aria-hidden />{generating ? "Generating…" : "Generate art"}
        </Button>

        {lastResult?.refused && (
          <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            Refused by moderation: {lastResult.moderation_reason || "Adjust the prompt and try again."}
          </p>
        )}
        {lastResult?.status === "generated" && (lastResult.path || lastResult.url) && (
          <div className="flex items-center gap-3 rounded-sm border border-hairline bg-board/40 p-2.5">
            <img src={imgSrc(project, lastResult.path, lastResult.url)} alt="Generated art" className="size-16 rounded-xs object-cover" />
            <code className="min-w-0 flex-1 truncate font-mono text-xs text-ink">{lastResult.path}</code>
          </div>
        )}
      </section>
    </div>
  );
}
