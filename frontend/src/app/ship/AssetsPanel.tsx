import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import { toast } from "sonner";
import { Box, Clock, CloudUpload, FolderUp, Image as ImageIcon, Lock, Music, Search, ShieldCheck, ShieldOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { MorphingDialog, MorphingDialogClose, MorphingDialogContainer, MorphingDialogContent, MorphingDialogDescription, MorphingDialogTitle, MorphingDialogTrigger } from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import {
  bytesToBase64,
  call,
  enc,
  importProjectModel,
  inferPrefabFormat,
  prettyBytes,
  projUrl,
  type AssetFile,
  type ImportModelResponse,
  type UploadResult,
} from "./api";
import type { Ship } from "./useShip";

const PREFAB_TARGET_SERVICES = ["Workspace", "ReplicatedStorage", "ServerStorage", "StarterGui"] as const;
const MAX_PREFAB_BYTES = 16 * 1024 * 1024;

type AssetFilter = "all" | "cards" | "weapons" | "audio" | "3d" | "pending";

function isImagePath(p: string): boolean {
  return /\.(png|jpe?g|webp|gif)$/i.test(p);
}

function assetCategory(f: AssetFile): Exclude<AssetFilter, "all" | "pending"> {
  const p = f.path.toLowerCase();
  const k = (f.kind || "").toLowerCase();
  if (p.includes("/weapons/") || p.startsWith("art/weapons") || k.includes("weapon")) return "weapons";
  if (/\.(wav|ogg|mp3|flac)$/i.test(p) || k.includes("audio") || k.includes("sound")) return "audio";
  if (/\.(obj|fbx|glb|gltf|rbxm|rbxmx)$/i.test(p) || k.includes("mesh") || k.includes("model")) return "3d";
  return "cards";
}

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
    <div className={`flex min-w-[88px] flex-1 flex-col rounded-sm px-3 py-2 ${tone}`}>
      <span className="font-mono text-title font-medium tabular-nums"><SlidingNumber value={value} /></span>
      <span className="text-sm">{label}</span>
    </div>
  );
}

export function AssetsPanel({
  ship,
  onOpenSettings,
  onImportedModel,
  onOpenViewport,
}: {
  ship: Ship;
  onOpenSettings: () => void;
  onImportedModel?: () => Promise<void> | void;
  onOpenViewport?: () => void;
}) {
  const { active, cloud } = ship;
  const [files, setFiles] = useState<AssetFile[] | null>(null);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<AssetFilter>("all");
  const [brokenImgs, setBrokenImgs] = useState<Record<string, boolean>>({});
  const [uploading, setUploading] = useState(false);
  const [pollingModeration, setPollingModeration] = useState(false);
  const [results, setResults] = useState<UploadResult[] | null>(null);
  const [note, setNote] = useState("");
  const [importingModel, setImportingModel] = useState(false);
  const [importTargetService, setImportTargetService] = useState<(typeof PREFAB_TARGET_SERVICES)[number]>("Workspace");
  const [importError, setImportError] = useState("");
  const [importResult, setImportResult] = useState<ImportModelResponse | null>(null);
  const modelFileInputRef = useRef<HTMLInputElement>(null);
  const poll = useRef<number | null>(null);
  const id = active?.id;
  const activeIdRef = useRef(id);
  activeIdRef.current = id;
  const loadSeqRef = useRef(0);

  const load = useCallback(async () => {
    if (!id) return;
    const reqSeq = ++loadSeqRef.current;
    const reqId = id;
    const r = await call<{ files: AssetFile[] }>(projUrl(id, "/assets"));
    if (reqSeq !== loadSeqRef.current || activeIdRef.current !== reqId) return;
    if (!r.ok) { setError(r.error || "Could not read the project's files."); setFiles((f) => f ?? []); return; }
    setError(""); setFiles(r.files ?? []);
  }, [id]);
  useEffect(() => {
    loadSeqRef.current++;
    setFiles(null);
    setResults(null);
    setNote("");
    setError("");
    setBrokenImgs({});
    setImportError("");
    setImportResult(null);
    void load();
  }, [load]);
  useEffect(() => () => { if (poll.current) window.clearInterval(poll.current); }, []);

  const list = files ?? [];
  const hasKey = !!cloud?.has_key;
  const queue = list.filter((f) => f.authorised && (!f.locked || f.stale));
  const shown = useMemo(
    () =>
      list.filter((f) => {
        if (query.trim() && !f.path.toLowerCase().includes(query.trim().toLowerCase())) return false;
        if (filter === "all") return true;
        if (filter === "pending") return !f.locked || Boolean(f.stale);
        return assetCategory(f) === filter;
      }),
    [list, query, filter],
  );
  const tally = { files: list.length, allowed: list.filter((f) => f.authorised).length, uploaded: list.filter((f) => f.locked).length, waiting: queue.length };
  const progressDenominator = tally.allowed > 0 ? tally.allowed : tally.files;
  const uploadPct = progressDenominator > 0 ? Math.min(100, Math.round((tally.uploaded / progressDenominator) * 100)) : 0;

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
  const codegen = async () => {
    if (!id) return;
    const r = await call<{ path?: string; count?: number }>(projUrl(id, "/assets/codegen"), {});
    if (!r.ok) { toast.error(r.error || "Could not write AssetIds.luau."); return; }
    toast.success(`Wrote ${r.path || "src/shared/AssetIds.luau"} (${r.count ?? 0} ids)`);
  };
  const runPollModeration = async () => {
    if (!id || pollingModeration) return;
    setPollingModeration(true);
    try {
      const r = await ship.pollModeration();
      if (r.ok) {
        setNote(`Moderation poll: ${r.approved ?? 0} approved, ${r.pending ?? 0} pending, ${r.rejected ?? 0} rejected`);
        await load();
      } else {
        setNote(r.error || "Moderation poll did not complete.");
      }
    } finally {
      setPollingModeration(false);
    }
  };
  const setAuthorised = async (paths: string[], revoke: boolean) => {
    if (!id) return;
    const r = await call(projUrl(id, "/assets/authorise"), { paths, revoke });
    if (!r.ok) { toast.error(r.error || "Could not change the allowed list."); return; }
    await load(); await ship.refresh();
  };

  const handleModelFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file || !id) return;
    const format = inferPrefabFormat(file.name);
    if (!format) {
      const msg = "Select a .rbxmx, .rbxm, or .rbxlx file.";
      setImportError(msg);
      toast.error(msg);
      return;
    }
    if (file.size === 0 || file.size > MAX_PREFAB_BYTES) {
      const msg = "Model file must be between 1 byte and 16 MB.";
      setImportError(msg);
      toast.error(msg);
      return;
    }
    setImportingModel(true);
    setImportError("");
    try {
      const reqId = id;
      const buf = await file.arrayBuffer();
      const bytes_base64 = bytesToBase64(new Uint8Array(buf));
      const res = await importProjectModel(id, {
        file_name: file.name,
        format,
        bytes_base64,
        target_service: importTargetService,
      });
      if (activeIdRef.current !== reqId) return;
      if (!res.ok) {
        const msg = res.error || "Could not import prefab model.";
        setImportError(msg);
        toast.error(msg);
        return;
      }
      const imported: ImportModelResponse = {
        ok: true,
        model_path: res.model_path || "",
        extracted_scripts: Array.isArray(res.extracted_scripts) ? res.extracted_scripts : [],
        wired_service: res.wired_service || importTargetService,
        warnings: Array.isArray(res.warnings) ? res.warnings : [],
      };
      setImportResult(imported);
      toast.success(`Imported ${imported.model_path} into ${imported.wired_service}`);
      await load();
      await ship.refresh();
      await onImportedModel?.();
    } catch {
      const msg = "Could not read selected model file.";
      setImportError(msg);
      toast.error(msg);
    } finally {
      setImportingModel(false);
    }
  };

  if (!active) return null;
  return (
    <div className="flex flex-col gap-4" data-panel="assets">
      <section
        aria-label="Prefab Model Import"
        data-prefab-import-card
        className="card-stock flex flex-col gap-3 p-4"
      >
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <Box className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
              <h3 className="font-heading text-ui font-medium text-ink">
                Prefab Model Import (.rbxmx / .rbxm / .rbxlx)
              </h3>
            </div>
            <p className="mt-0.5 text-xs text-ink-2">
              Decompose Roblox model or place files into reusable scene/models/*.model.json prefabs and extracted Luau scripts, wired into default.project.json.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input
              ref={modelFileInputRef}
              type="file"
              accept=".rbxmx,.rbxm,.rbxlx"
              hidden
              data-import-model-input
              onChange={(ev) => void handleModelFileChange(ev)}
            />
            <Button
              variant="default"
              size="sm"
              loading={importingModel}
              onClick={() => modelFileInputRef.current?.click()}
              data-import-model-btn
              className="max-md:min-h-11"
            >
              <FolderUp className="size-3.5" strokeWidth={1.5} aria-hidden />
              Import Model
            </Button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs font-medium text-ink-2">Target service:</span>
          <SegmentedControl aria-label="Target service for imported model" size="sm" data-import-target-service className="max-w-full flex-wrap">
            {PREFAB_TARGET_SERVICES.map((svc) => (
              <SegmentedControlItem
                key={svc}
                data-checked={importTargetService === svc ? "" : undefined}
                aria-pressed={importTargetService === svc}
                onClick={() => setImportTargetService(svc)}
                className="max-md:!h-11 max-md:px-2"
              >
                {svc}
              </SegmentedControlItem>
            ))}
          </SegmentedControl>
        </div>

        {importError && (
          <p role="alert" data-import-model-error className="rounded-sm bg-pill-pro-bg px-3 py-2 text-xs text-pill-pro-fg [overflow-wrap:anywhere]">
            {importError}
          </p>
        )}

        {importResult && (
          <div
            data-import-model-result
            className="flex min-w-0 flex-col gap-1.5 rounded-sm border border-hairline bg-board/45 px-3 py-2 text-xs"
          >
            <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                <Pill className="shrink-0 bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">Imported</Pill>
                <code className="min-w-0 break-all font-mono text-xs text-ink">{importResult.model_path}</code>
                <span className="text-ink-2">wired to {importResult.wired_service}</span>
              </div>
              {onOpenViewport && (
                <Button
                  variant="outline"
                  size="xs"
                  data-import-open-viewport
                  onClick={onOpenViewport}
                  className="shrink-0 bg-surface max-md:min-h-11"
                >
                  View in 3D
                </Button>
              )}
            </div>
            {importResult.extracted_scripts.length > 0 && (
              <div className="min-w-0 text-ink-2 [overflow-wrap:anywhere]">
                Extracted scripts:{" "}
                <span className="break-all font-mono text-ink">{importResult.extracted_scripts.join(", ")}</span>
              </div>
            )}
            {importResult.warnings.length > 0 && (
              <ul className="flex flex-col gap-0.5 text-pill-warn-fg">
                {importResult.warnings.map((w, wi) => (
                  <li key={wi}>{w}</li>
                ))}
              </ul>
            )}
          </div>
        )}
      </section>

      <section aria-label="Upload queue" className="card-stock flex flex-col gap-3 p-4">
        <div className="flex flex-wrap gap-2">
          <Tally label="files" value={tally.files} tone="bg-pill-inactive-bg text-pill-inactive-fg" />
          <Tally label="allowed to upload" value={tally.allowed} tone="bg-tag-sky-bg text-tag-sky-fg" />
          <Tally label="waiting" value={tally.waiting} tone="bg-pill-warn-bg text-pill-warn-fg" />
          <Tally label="on Roblox" value={tally.uploaded} tone="bg-pill-active-bg text-pill-active-fg" />
        </div>
        {tally.files > 0 && (
          <div className="flex items-center gap-2.5 text-xs text-ink-2" data-upload-progress>
            <ProgressCircle value={uploadPct} radius={12} strokeWidth={3} className="size-7 flex-none">
              <span className="font-mono text-xs tabular-nums text-ink">{uploadPct}</span>
            </ProgressCircle>
            <span>
              <span className="font-mono tabular-nums text-ink">{tally.uploaded}/{progressDenominator}</span> uploaded (<span className="font-mono tabular-nums text-ink">{uploadPct}%</span>)
            </span>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="default" size="lg" disabled={!hasKey || uploading || tally.waiting === 0} onClick={() => void upload({ authorised: true })} data-upload className="active:opacity-80 max-md:min-h-11"><CloudUpload className="size-4" strokeWidth={1.5} aria-hidden />{uploading ? "Uploading…" : `Upload ${tally.waiting} allowed`}</Button>
          <Button variant="outline" size="lg" disabled={!hasKey || uploading} onClick={() => void upload({ authorised: true, dry_run: true })} className="active:opacity-80 max-md:min-h-11">Preview only</Button>
          <Button variant="outline" size="lg" disabled={uploading} onClick={() => void codegen()} className="active:opacity-80 max-md:min-h-11">Write AssetIds.luau</Button>
          <Button variant="outline" size="lg" loading={pollingModeration} disabled={uploading || pollingModeration} onClick={() => void runPollModeration()} data-poll-moderation className="active:opacity-80 max-md:min-h-11"><ShieldCheck className="size-4" strokeWidth={1.5} aria-hidden />{pollingModeration ? "Polling…" : "Poll Moderation"}</Button>
          <MorphingDialog transition={dialogSpring}>
            <MorphingDialogTrigger blocked={!hasKey || uploading} className="inline-flex min-h-11 items-center justify-center gap-2 rounded-lg border border-hairline bg-surface px-3 text-sm font-medium text-pill-pro-fg outline-none hover:bg-pill-pro-bg active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus disabled:border-hairline disabled:bg-track disabled:text-ink-3" thing="upload-everything">Upload everything…</MorphingDialogTrigger>
            <MorphingDialogContainer>
              <MorphingDialogContent className="relative mx-3 flex w-full max-w-[440px] flex-col gap-4 rounded-dialog border border-hairline bg-surface p-5 text-ink shadow-lift">
                <div>
                  <MorphingDialogTitle className="font-heading text-title font-semibold">Upload every file?</MorphingDialogTitle>
                  <MorphingDialogDescription className="mt-1 text-sm text-ink-2">Sends all {tally.files} files, including {tally.files - tally.allowed} unapproved drafts.</MorphingDialogDescription>
                </div>
                <div className="flex flex-wrap justify-end gap-2">
                  <MorphingDialogClose aria-label="Cancel" className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Cancel</MorphingDialogClose>
                  <MorphingDialogClose aria-label="Yes, upload everything" onClick={() => void upload({ everything: true })} className="static inline-flex min-h-11 cursor-pointer items-center rounded-lg border border-primary bg-primary px-4 text-sm font-medium text-primary-foreground outline-none hover:bg-primary/90 focus-visible:ring-2 focus-visible:ring-focus">Yes, upload everything</MorphingDialogClose>
                </div>
              </MorphingDialogContent>
            </MorphingDialogContainer>
          </MorphingDialog>
        </div>
        {!hasKey && <p className="text-sm text-ink-2">Save an Open Cloud key in <Button variant="link" size="sm" onClick={onOpenSettings} className="inline h-auto p-0 underline max-md:min-h-11">Settings</Button> to upload.</p>}
        {error && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{error}</p>}
        {note && <p className="rounded-sm bg-board px-3 py-2 text-sm text-ink-2">{note}</p>}
        {results && (
          <ul className="flex max-h-48 flex-col gap-1 overflow-y-auto text-sm" aria-label="Upload results" data-upload-results>
            {results.map((x) => (
              <li key={x.path} className="flex flex-wrap items-baseline justify-between gap-2"><code className="min-w-0 break-all font-mono">{x.path}</code><span className={x.state === "failed" ? "text-pill-pro-fg" : "text-ink-2"}>{x.error || RESULT_WORDS[x.state] || x.state}{x.assetId ? <span className="ml-2 font-mono">{x.assetId}</span> : null}</span></li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Files" className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <SegmentedControl aria-label="Asset type filter" size="sm" data-asset-filters className="max-w-full flex-wrap">
            {(
              [
                { k: "all", label: "All" },
                { k: "cards", label: "Cards" },
                { k: "weapons", label: "Weapons" },
                { k: "audio", label: "Audio" },
                { k: "3d", label: "3D" },
                { k: "pending", label: "Pending" },
              ] as const
            ).map((opt) => (
              <SegmentedControlItem
                key={opt.k}
                data-checked={filter === opt.k ? "" : undefined}
                aria-pressed={filter === opt.k}
                onClick={() => setFilter(opt.k)}
                className="max-md:!h-11 max-md:px-2.5"
              >
                {opt.label}
              </SegmentedControlItem>
            ))}
          </SegmentedControl>
          <InputGroup className="min-w-[200px] flex-1 sm:max-w-xs">
            <InputGroupAddon align="inline-start">
              <Search className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
            </InputGroupAddon>
            <InputGroupInput type="search" aria-label="Search files" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filter assets" />
          </InputGroup>
        </div>

        {files === null ? (
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3" aria-hidden>
            <span className="h-44 rounded-card bg-track" />
            <span className="h-44 rounded-card bg-track" />
            <span className="h-44 rounded-card bg-track" />
          </div>
        ) : shown.length === 0 ? (
          <p className="card-stock p-4 text-sm text-ink-2">{list.length === 0 ? "No images, audio, or 3D assets in this project yet." : "No matching assets."}</p>
        ) : (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(216px,1fr))] gap-3" data-asset-gallery>
            {shown.map((f, idx) => {
              const m = f.locked ? moderation(f.locked.state) : null;
              const isImg = isImagePath(f.path);
              const cat = assetCategory(f);
              const imgUrl = id && isImg ? `/api/art/image?project=${enc(id)}&path=${enc(f.path)}` : "";
              const fileName = f.path.split("/").pop() || f.path;
              const imgBroken = !imgUrl || Boolean(brokenImgs[f.path]);

              return (
                <li key={f.path} className="card-stock flex flex-col justify-between gap-2 overflow-hidden p-2.5" data-asset={f.path}>
                  {isImg && !imgBroken ? (
                    <MorphingDialog transition={dialogSpring}>
                      <MorphingDialogTrigger
                        aria-label={`Preview ${fileName}`}
                        className="block w-full overflow-hidden rounded-sm border border-hairline bg-board outline-none active:opacity-80 focus-visible:ring-2 focus-visible:ring-focus"
                      >
                        <img
                          src={imgUrl}
                          alt={fileName}
                          loading={idx === 0 ? "eager" : "lazy"}
                          fetchPriority={idx === 0 ? "high" : undefined}
                          onError={() => setBrokenImgs((p) => ({ ...p, [f.path]: true }))}
                          className="aspect-[4/3] w-full object-cover"
                        />
                      </MorphingDialogTrigger>
                      <MorphingDialogContainer>
                        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[780px] flex-col gap-3 overflow-hidden rounded-dialog border border-hairline bg-surface p-4 text-ink shadow-lift">
                          <MorphingDialogTitle className="truncate font-heading text-ui font-semibold">{fileName}</MorphingDialogTitle>
                          <img src={imgUrl} alt={fileName} className="min-h-0 w-full flex-1 rounded-sm object-contain" />
                          <code className="truncate font-mono text-xs text-ink-3">{f.path}</code>
                          <MorphingDialogClose aria-label="Close preview" className="static inline-flex min-h-11 cursor-pointer items-center justify-center self-end rounded-lg border border-hairline bg-surface px-4 text-sm font-medium text-ink outline-none hover:bg-board focus-visible:ring-2 focus-visible:ring-focus">Close</MorphingDialogClose>
                        </MorphingDialogContent>
                      </MorphingDialogContainer>
                    </MorphingDialog>
                  ) : (
                    <div className="flex aspect-[4/3] w-full flex-col items-center justify-center gap-1.5 rounded-sm border border-hairline bg-board/60 text-ink-2">
                      {cat === "audio" ? (
                        <Music className="size-7 text-blue" strokeWidth={1.5} aria-hidden />
                      ) : cat === "3d" ? (
                        <Box className="size-7 text-blue" strokeWidth={1.5} aria-hidden />
                      ) : (
                        <ImageIcon className="size-7 text-ink-3" strokeWidth={1.5} aria-hidden />
                      )}
                      <span className="text-xs text-ink-3">{f.kind}</span>
                    </div>
                  )}

                  <div className="flex min-w-0 flex-col gap-1">
                    <span className="truncate text-sm font-medium text-ink" title={f.path}>{fileName}</span>
                    <div className="flex items-center justify-between gap-2 text-xs text-ink-2">
                      <span className="truncate font-mono">{f.path}</span>
                      <span className="shrink-0 font-mono tabular-nums">{prettyBytes(f.bytes)}</span>
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center justify-between gap-1.5 border-t border-hairline pt-2">
                    <div className="flex min-w-0 flex-wrap items-center gap-1">
                      {f.locked ? (
                        <>
                          <Pill className="max-w-full truncate bg-pill-inactive-bg px-1.5 py-0 font-mono text-xs text-pill-inactive-fg"><Lock className="size-3 shrink-0" strokeWidth={1.5} aria-hidden /><span className="truncate">{f.locked.assetId}</span></Pill>
                          {m && <Pill className={`px-1.5 py-0 text-xs ${m.cls}`}>{m.label}</Pill>}
                          {f.stale && <Pill className="bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg">changed</Pill>}
                        </>
                      ) : (
                        <Pill className="bg-pill-inactive-bg px-1.5 py-0 text-xs text-pill-inactive-fg"><Clock className="size-3" strokeWidth={1.5} aria-hidden />pending</Pill>
                      )}
                    </div>
                    <Button variant={f.authorised ? "secondary" : "outline"} size="xs" aria-pressed={f.authorised} aria-label={`${f.authorised ? "Stop allowing" : "Allow"} ${f.path}`} onClick={() => void setAuthorised([f.path], f.authorised)} className="active:opacity-80 max-md:min-h-11">
                      {f.authorised ? <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden /> : <ShieldOff className="size-3.5" strokeWidth={1.5} aria-hidden />}{f.authorised ? "Allowed" : "Allow"}
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
