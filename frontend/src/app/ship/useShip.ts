import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { call, projUrl, type Build, type Detail, type Diff, type OpenCloud, type Summary, type SyncResult } from "./api";

export type Busy = "" | "list" | "build" | "diff" | "sync" | "create" | "open";

export type StudioDeviceProfile = "desktop_1920x1080" | "tablet_1024x768" | "phone_844x390";

export type StageStudioContentResult = {
  ok: boolean;
  error?: string;
  content_dir?: string;
  prefix?: string;
  staged_count?: number;
  skipped_count?: number;
  files?: string[];
};

export type VerifyStudioDeviceResult = {
  ok: boolean;
  error?: string;
  mode?: "live_studio" | "studio_disconnected" | string;
  profile?: StudioDeviceProfile | string;
  checked_buttons?: number;
  violations?: unknown[];
  screenshot_path?: string;
  passed?: boolean;
};

export type VerifyStudioAudioResult = {
  ok: boolean;
  error?: string;
  offline_wav_checks?: unknown[];
  live_studio_connected?: boolean;
  studio_sounds_checked?: number;
  studio_sounds_loaded?: number;
  passed?: boolean;
};

export type PollModerationResult = {
  ok: boolean;
  error?: string;
  checked?: number;
  approved?: number;
  rejected?: number;
  pending?: number;
  codegen_updated?: boolean;
};

/** The projects, the open one's details, the Open Cloud state and the results of the last build / diff / sync. */
export function useShip(enabled: boolean) {
  const [available, setAvailable] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [projects, setProjects] = useState<Summary[]>([]);
  const [roots, setRoots] = useState<string[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [cloud, setCloud] = useState<OpenCloud | null>(null);
  const [busy, setBusy] = useState<Busy>("");
  const [build, setBuild] = useState<Build | null>(null);
  const [diff, setDiff] = useState<Diff | null>(null);
  const [sync, setSync] = useState<SyncResult | null>(null);
  const [error, setError] = useState("");
  const activeRef = useRef<string | null>(null);
  activeRef.current = activeId;

  const loadDetail = useCallback(async (id: string | null) => {
    if (!id) { setDetail(null); return; }
    const r = await call<{ project: Detail }>(projUrl(id));
    if (activeRef.current === id) setDetail(r.ok && r.project ? r.project : null);
  }, []);

  const refreshCloud = useCallback(async () => {
    const r = await call<OpenCloud>("/api/opencloud");
    setCloud(r.ok ? (r as OpenCloud) : { has_key: false });
  }, []);

  const refresh = useCallback(async () => {
    const r = await call<{ roots: string[]; active: string | null; projects: Summary[] }>("/api/projects");
    if (r.httpStatus === 404) { setAvailable(false); setLoaded(true); return; }
    if (!r.ok) { setError(r.error || "Could not read your projects."); setLoaded(true); return; }
    setError("");
    setProjects(r.projects || []);
    setRoots(r.roots || []);
    setActiveId((prev) => (r.active !== undefined && r.active !== prev ? r.active : prev));
    setLoaded(true);
    await loadDetail(r.active ?? activeRef.current);
  }, [loadDetail]);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
    void refreshCloud();
  }, [enabled, refresh, refreshCloud]);

  const run = async <T,>(kind: Busy, fn: () => Promise<T>): Promise<T | undefined> => {
    if (busy) return undefined;
    setBusy(kind);
    try { return await fn(); } finally { setBusy(""); }
  };

  const openProject = (path: string) => run("open", async () => {
    const r = await call<{ project: Summary }>("/api/projects/open", { path });
    if (!r.ok || !r.project) { toast.error(r.error || "Could not open that project."); return false; }
    setActiveId(r.project.id); activeRef.current = r.project.id;
    setBuild(null); setDiff(null); setSync(null);
    await refresh();
    return true;
  });

  const createProject = (body: { name: string; parent?: string; universeId?: number; placeId?: number }) => run("create", async () => {
    const r = await call<{ project: Summary }>("/api/projects/create", body);
    if (!r.ok || !r.project) { toast.error(r.error || "Could not create the project."); return false; }
    toast.success(`Created ${r.project.name}`);
    setActiveId(r.project.id); activeRef.current = r.project.id;
    setBuild(null); setDiff(null); setSync(null);
    await refresh();
    return true;
  });

  const buildNow = () => run("build", async () => {
    if (!activeId) return;
    const r = await call<Build>(projUrl(activeId, "/build"), { engine: "auto" });
    if (!r.ok) { toast.error(r.error || "The build did not finish."); return; }
    setBuild(r as Build);
    toast.success(`Built ${(r as Build).instances} parts and ${(r as Build).scripts} scripts`);
    await refresh();
  });

  const checkChanges = () => run("diff", async () => {
    if (!activeId) return;
    const r = await call<Diff>(projUrl(activeId, "/sync/diff"), {});
    if (!r.ok) { toast.error(r.error || "Could not compare with Studio."); return; }
    setDiff(r as Diff); setSync(null);
  });

  const syncNow = (prune = false) => run("sync", async () => {
    if (!activeId) return;
    const r = await call<SyncResult>(projUrl(activeId, "/sync"), { prune });
    if (!r.ok) { toast.error(r.error || "The sync did not finish."); return; }
    setSync(r as SyncResult);
    setDiff(null);
    if ((r.errors?.length ?? 0) > 0) toast.error(`Synced with ${r.errors!.length} problem(s)`);
    else toast.success("Studio now matches the project");
  });

  /** Writes top-level manifest keys (shallow merge) and reloads the project. */
  const saveManifest = async (patch: Record<string, unknown>) => {
    if (!activeId) return false;
    const r = await call(projUrl(activeId, "/manifest"), patch);
    if (!r.ok) { toast.error(r.error || "Could not save the project file."); return false; }
    await refresh();
    return true;
  };

  const stageStudioContent = useCallback(async (): Promise<StageStudioContentResult> => {
    if (!activeId) return { ok: false, error: "Open a project first." };
    const r = await call<StageStudioContentResult>(projUrl(activeId, "/studio/stage-content"), {});
    if (!r.ok) {
      const msg = r.error || "Could not stage Studio content.";
      toast.error(msg);
      return { ok: false, error: msg };
    }
    toast.success(`Staged ${r.staged_count ?? 0} Studio file(s)`);
    return { ...(r as StageStudioContentResult), ok: true };
  }, [activeId]);

  const verifyStudioDevice = useCallback(
    async (profile: StudioDeviceProfile = "phone_844x390"): Promise<VerifyStudioDeviceResult> => {
      if (!activeId) return { ok: false, passed: false, error: "Open a project first." };
      const r = await call<VerifyStudioDeviceResult>(projUrl(activeId, "/studio/verify-device"), {
        profile,
        capture_screenshot: true,
      });
      if (r.mode === "studio_disconnected") {
        const msg = r.error || "Studio is disconnected — connect Roblox Studio to run live device checks.";
        toast.info(msg);
        return { ...(r as VerifyStudioDeviceResult), ok: Boolean(r.ok), passed: false, error: msg };
      }
      if (!r.ok) {
        const msg = r.error || "Live device verification failed.";
        toast.error(msg);
        return { ...(r as VerifyStudioDeviceResult), ok: false, passed: false, error: msg };
      }
      if (r.passed) {
        toast.success(`Studio ${profile} verified (${r.checked_buttons ?? 0} buttons)`);
      } else {
        toast.warning(`Studio ${profile} reported layout issues`);
      }
      return { ...(r as VerifyStudioDeviceResult), ok: true };
    },
    [activeId],
  );

  const verifyStudioAudio = useCallback(async (): Promise<VerifyStudioAudioResult> => {
    if (!activeId) return { ok: false, passed: false, error: "Open a project first." };
    const r = await call<VerifyStudioAudioResult>(projUrl(activeId, "/studio/verify-audio"), {});
    if (!r.ok) {
      const msg = r.error || "Could not verify Studio audio.";
      toast.error(msg);
      return { ...(r as VerifyStudioAudioResult), ok: false, passed: false, error: msg };
    }
    if (r.passed) {
      toast.success(`Studio audio verified (${r.studio_sounds_loaded ?? 0}/${r.studio_sounds_checked ?? 0} loaded)`);
    } else if (r.live_studio_connected === false) {
      toast.info("Offline WAV checks complete; connect Studio for live Sound preload check.");
    } else {
      toast.warning("Studio audio verification reported issues");
    }
    return { ...(r as VerifyStudioAudioResult), ok: true };
  }, [activeId]);

  const pollModeration = useCallback(async (): Promise<PollModerationResult> => {
    if (!activeId) return { ok: false, error: "Open a project first." };
    const r = await call<PollModerationResult>(projUrl(activeId, "/assets/moderation"), {});
    if (!r.ok) {
      const msg = r.error || "Could not poll Open Cloud moderation status.";
      toast.error(msg);
      return { ok: false, error: msg };
    }
    toast.success(`Moderation checked (${r.approved ?? 0} approved, ${r.pending ?? 0} pending)`);
    await refresh();
    return { ...(r as PollModerationResult), ok: true };
  }, [activeId, refresh]);

  const active = projects.find((p) => p.id === activeId) ?? null;
  return {
    available,
    loaded,
    projects,
    roots,
    active,
    activeId,
    detail,
    cloud,
    busy,
    build,
    diff,
    sync,
    error,
    refresh,
    refreshCloud,
    openProject,
    createProject,
    buildNow,
    checkChanges,
    syncNow,
    saveManifest,
    stageStudioContent,
    verifyStudioDevice,
    verifyStudioAudio,
    pollModeration,
  };
}
export type Ship = ReturnType<typeof useShip>;
