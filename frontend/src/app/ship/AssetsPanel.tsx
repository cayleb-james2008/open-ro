import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { Clock, CloudUpload, Lock, Search, ShieldCheck, ShieldOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Pill } from "@/components/kibo-ui/pill";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { call, prettyBytes, projUrl, type AssetFile, type UploadResult } from "./api";
import type { Ship } from "./useShip";

/** Moderation words from the lock state Roblox returns with an upload. */
export function moderation(state: string | undefined): { label: string; cls: string } {
  const s = (state || "").toLowerCase();
  if (!s) return { label: "no result yet", cls: "bg-pill-inactive-bg text-pill-inactive-fg" };
  if (/(approv|complete|done|ok|pass|accept)/.test(s)) return { label: "approved", cls: "bg-pill-active-bg text-pill-active-fg" };
  if (/(reject|declin|deny|denied|fail|moderat|block)/.test(s)) return { label: "rejected", cls: "bg-pill-pro-bg text-pill-pro-fg" };
  return { label: "in review", cls: "bg-pill-warn-bg text-pill-warn-fg" };
}
const RESULT_WORDS: Record<string, string> = { uploaded: "uploaded", reused: "already on Roblox", unchanged: "unchanged", skipped: "skipped", failed: "failed" };

function Tally({ label, value, tone }: { label: string; value: number; tone: string }) {
  return (
    <div className={`flex min-w-[88px] flex-1 flex-col rounded-card px-3 py-2 ${tone}`}>
      <span className="font-mono text-title font-medium tabular-nums"><SlidingNumber value={value} /></span>
      <span className="text-sm">{label}</span>
    </div>
  );
}

export function AssetsPanel({ ship, onOpenSettings }: { ship: Ship; onOpenSettings: () => void }) {
  const { active, cloud } = ship;
  const [files, setFiles] = useState<AssetFile[] | null>(null);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [uploading, setUploading] = useState(false);
  const [results, setResults] = useState<UploadResult[] | null>(null);
  const [note, setNote] = useState("");
  const poll = useRef<number | null>(null);
  const id = active?.id;

  const load = useCallback(async () => {
    if (!id) return;
    const r = await call<{ files: AssetFile[] }>(projUrl(id, "/assets"));
    if (!r.ok) { setError(r.error || "Could not read the project's files."); setFiles((f) => f ?? []); return; }
    setError(""); setFiles(r.files ?? []);
  }, [id]);
  useEffect(() => { setFiles(null); setResults(null); setNote(""); void load(); }, [load]);
  useEffect(() => () => { if (poll.current) window.clearInterval(poll.current); }, []);

  const list = files ?? [];
  const hasKey = !!cloud?.has_key;
  const queue = list.filter((f) => f.authorised && (!f.locked || f.stale));
  const shown = useMemo(() => list.filter((f) => !query.trim() || f.path.toLowerCase().includes(query.trim().toLowerCase())), [list, query]);
  const tally = { files: list.length, allowed: list.filter((f) => f.authorised).length, uploaded: list.filter((f) => f.locked).length, waiting: queue.length };

  const upload = async (body: Record<string, unknown>) => {
    if (!id) return;
    setUploading(true); setResults(null); setNote("");
    poll.current = window.setInterval(() => void load(), 2500);
    const r = await call<{ results: UploadResult[]; summary?: { note?: string } }>(projUrl(id, "/assets/upload"), body);
    if (poll.current) { window.clearInterval(poll.current); poll.current = null; }
    setUploading(false);
    if (!r.ok) { setError(r.error || "The upload did not finish."); return; }
    setResults(r.results ?? []);
    setNote(r.summary?.note ?? "");
    const failed = (r.results ?? []).filter((x) => x.state === "failed").length;
    if (failed) toast.error(`${failed} file(s) failed to upload`); else toast.success("Upload finished");
    await load(); await ship.refresh();
  };
  const setAuthorised = async (paths: string[], revoke: boolean) => {
    if (!id) return;
    const r = await call(projUrl(id, "/assets/authorise"), { paths, revoke });
    if (!r.ok) { toast.error(r.error || "Could not change the allowed list."); return; }
    await load(); await ship.refresh();
  };

  if (!active) return null;
  return (
    <div className="flex flex-col gap-4" data-panel="assets">
      <section aria-label="Upload queue" className="card-stock flex flex-col gap-3 p-4">
        <div className="flex flex-wrap gap-2">
          <Tally label="files" value={tally.files} tone="bg-pill-inactive-bg text-pill-inactive-fg" />
          <Tally label="allowed to upload" value={tally.allowed} tone="bg-tag-sky-bg text-tag-sky-fg" />
          <Tally label="waiting" value={tally.waiting} tone="bg-pill-warn-bg text-pill-warn-fg" />
          <Tally label="on Roblox" value={tally.uploaded} tone="bg-pill-active-bg text-pill-active-fg" />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="default" size="lg" disabled={!hasKey || uploading || tally.waiting === 0} onClick={() => void upload({ authorised: true })} data-upload className="max-md:min-h-11"><CloudUpload className="size-4" strokeWidth={1.5} aria-hidden />{uploading ? "Uploading…" : `Upload ${tally.waiting} allowed`}</Button>
          <Button variant="outline" size="lg" disabled={!hasKey || uploading} onClick={() => void upload({ authorised: true, dry_run: true })} className="max-md:min-h-11">Preview only</Button>
          <MorphingDialog transition={dialogSpring}>
            <MorphingDialogTrigger blocked={!hasKey || uploading} className="inline-flex min-h-11 items-center justify-center gap-2 rounded-lg border border-hairline bg-surface px-3 text-sm font-medium text-pill-pro-fg outline-none hover:bg-pill-pro-bg focus-visible:ring-2 focus-visible:ring-focus disabled:border-hairline disabled:bg-track disabled:text-ink-3" thing="upload-everything">Upload everything…</MorphingDialogTrigger>
            <MorphingDialogContainer>
              <MorphingDialogContent className="relative mx-3 flex w-full max-w-[440px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                <div>
                  <MorphingDialogTitle className="font-heading text-title font-semibold">Upload every file?</MorphingDialogTitle>
                  <MorphingDialogDescription className="mt-1 text-sm text-ink-2">This sends all {tally.files} uploadable files, including the {tally.files - tally.allowed} you have not allowed. Reference images and drafts may be among them. Normally you allow files first.</MorphingDialogDescription>
                </div>
                <div className="flex flex-wrap justify-end gap-2">
                  <MorphingDialogClose aria-label="Cancel" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Cancel</MorphingDialogClose>
                  <MorphingDialogClose aria-label="Yes, upload everything" onClick={() => void upload({ everything: true })} className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-primary bg-primary px-4 text-sm font-medium text-primary-foreground outline-none hover:bg-primary/90 focus-visible:ring-2 focus-visible:ring-focus">Yes, upload everything</MorphingDialogClose>
                </div>
              </MorphingDialogContent>
            </MorphingDialogContainer>
          </MorphingDialog>
        </div>
        {!hasKey && <p className="text-sm text-ink-2">Uploads need an Open Cloud key. <Button variant="link" size="sm" onClick={onOpenSettings} className="inline h-auto p-0 underline max-md:min-h-11">Add it in Settings</Button></p>}
        {error && <p role="alert" className="rounded-card bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
        {note && <p className="rounded-card bg-board px-3 py-2 text-sm text-ink-2">{note}</p>}
        {results && (
          <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto text-sm" aria-label="Upload results" data-upload-results>
            {results.map((x) => (
              <li key={x.path} className="flex flex-wrap items-baseline justify-between gap-2"><code className="min-w-0 break-all font-mono">{x.path}</code><span className={x.state === "failed" ? "text-pill-pro-fg" : "text-ink-2"}>{x.error || RESULT_WORDS[x.state] || x.state}{x.assetId ? <span className="ml-2 font-mono">{x.assetId}</span> : null}</span></li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Files" className="flex flex-col gap-2">
        <InputGroup>
          <InputGroupAddon align="inline-start">
            <Search className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
          </InputGroupAddon>
          <InputGroupInput type="search" aria-label="Search files" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search files" />
        </InputGroup>
        {files === null ? (
          <div className="grid gap-2" aria-hidden><span className="h-14 rounded-card bg-track" /><span className="h-14 rounded-card bg-track" /></div>
        ) : shown.length === 0 ? (
          <p className="card-stock p-4 text-sm text-ink-2">{list.length === 0 ? "No images, audio or models in this project yet." : "No file matches that search."}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {shown.map((f) => {
              const m = f.locked ? moderation(f.locked.state) : null;
              return (
                <li key={f.path} className="card-stock flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2.5" data-asset={f.path}>
                  <div className="min-w-0 flex-1">
                    <code className="block truncate font-mono text-sm text-ink">{f.path}</code>
                    <span className="text-sm text-ink-2">{f.kind} · {prettyBytes(f.bytes)}</span>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    {f.locked ? (
                      <>
                        <Pill className="bg-pill-inactive-bg px-2 py-0.5 font-mono text-sm text-pill-inactive-fg"><Lock className="size-3.5" strokeWidth={1.5} aria-hidden />{f.locked.assetId}</Pill>
                        {m && <Pill className={`px-2 py-0.5 text-sm ${m.cls}`}>{m.label}</Pill>}
                        {f.stale && <Pill className="bg-pill-warn-bg px-2 py-0.5 text-sm text-pill-warn-fg">changed since upload</Pill>}
                      </>
                    ) : (
                      <Pill className="bg-pill-inactive-bg px-2 py-0.5 text-sm text-pill-inactive-fg"><Clock className="size-3.5" strokeWidth={1.5} aria-hidden />not uploaded</Pill>
                    )}
                    <Button variant={f.authorised ? "secondary" : "outline"} size="sm" aria-pressed={f.authorised} aria-label={`${f.authorised ? "Stop allowing" : "Allow"} ${f.path}`} onClick={() => void setAuthorised([f.path], f.authorised)} className="max-md:min-h-11">
                      {f.authorised ? <ShieldCheck className="size-4" strokeWidth={1.5} aria-hidden /> : <ShieldOff className="size-4" strokeWidth={1.5} aria-hidden />}{f.authorised ? "Allowed" : "Allow"}
                    </Button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
