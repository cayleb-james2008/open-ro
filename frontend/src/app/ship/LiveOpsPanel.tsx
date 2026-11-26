import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
  Activity,
  AlertTriangle,
  Archive,
  BarChart3,
  Bug,
  Check,
  CloudDownload,
  Code2,
  Database,
  FileCode2,
  Plus,
  Radio,
  RefreshCw,
  Save,
  Send,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import {
  deleteLiveOpsDataStoreEntry,
  generateLiveOpsTelemetryCollector,
  getLiveOpsDataStoreEntry,
  getLiveOpsErrors,
  getLiveOpsRemoteConfig,
  getLiveOpsTelemetrySummary,
  ingestLiveOpsTelemetry,
  listLiveOpsDataStores,
  prettyBytes,
  publishLiveOpsMessage,
  saveLiveOpsRemoteConfig,
  setLiveOpsDataStoreEntry,
  stamp,
  syncLiveOpsTelemetry,
  type LiveErrorAggregate,
  type LiveOpsDataStoreEntry,
  type LiveOpsDataStoreKind,
  type LiveOpsRemoteConfigResponse,
  type LiveOpsTelemetryEvent,
  type LiveOpsTelemetrySummary,
  type TelemetryCodegenMode,
  type TelemetryCodegenResponse,
  type TelemetrySourceFilter,
  type TelemetrySyncSource,
  type TelemetrySyncState,
} from "./api";
import { Field } from "./ProjectsRail";
import type { Ship } from "./useShip";

type RemoteConfigRow = {
  id: string;
  key: string;
  kind: "number" | "boolean" | "string";
  value: string;
};

const LUAU_IDENT_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

function inferRowKind(val: unknown): { kind: RemoteConfigRow["kind"]; value: string } {
  if (typeof val === "boolean") return { kind: "boolean", value: val ? "true" : "false" };
  if (typeof val === "number" && Number.isFinite(val)) return { kind: "number", value: String(val) };
  return { kind: "string", value: typeof val === "string" ? val : JSON.stringify(val ?? "") };
}

function parseRowValue(row: RemoteConfigRow): unknown {
  if (row.kind === "boolean") return row.value === "true";
  if (row.kind === "number") {
    const n = Number(row.value);
    return Number.isFinite(n) ? n : 0;
  }
  return row.value;
}

function formatJsonValue(v: unknown): string {
  if (typeof v === "string") return JSON.stringify(v);
  try {
    return JSON.stringify(v ?? {}, null, 2);
  } catch {
    return "{}";
  }
}

function tryParseJson(raw: string): { ok: true; value: unknown } | { ok: false; error: string } {
  const trimmed = raw.trim();
  if (!trimmed) return { ok: false, error: "JSON payload cannot be empty." };
  try {
    return { ok: true, value: JSON.parse(trimmed) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : "Invalid JSON syntax." };
  }
}

export function LiveOpsPanel({
  ship,
  onOpenCodeFile,
}: {
  ship: Ship;
  onOpenCodeFile?: (scriptPath: string) => void;
}) {
  const projectId = ship.active?.id ?? null;
  const isBrowserEngine =
    typeof window !== "undefined" &&
    Boolean((window as Window & { __OPENRO_WEB_ENGINE__?: boolean }).__OPENRO_WEB_ENGINE__);
  const openCloudReady = isBrowserEngine || Boolean(ship.cloud?.has_key && ship.active?.universeId);

  // 1) DataStores Inspector & Safe Editor
  const [dsStore, setDsStore] = useState("PlayerData");
  const [dsScope, setDsScope] = useState("global");
  const [dsPrefix, setDsPrefix] = useState("");
  const [dsKind, setDsKind] = useState<LiveOpsDataStoreKind>("standard");
  const [dsEntries, setDsEntries] = useState<LiveOpsDataStoreEntry[]>([]);
  const [dsSource, setDsSource] = useState("");
  const [dsBusy, setDsBusy] = useState(false);
  const [dsMutating, setDsMutating] = useState(false);
  const [dsError, setDsError] = useState("");
  const [dsConflict, setDsConflict] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string>("");
  const [selectedRevision, setSelectedRevision] = useState<string | null>(null);
  const [selectedUpdatedAt, setSelectedUpdatedAt] = useState<string | null>(null);
  const [dsValueText, setDsValueText] = useState('{\n  "coins": 100\n}');
  const [dsNewKey, setDsNewKey] = useState("");
  const [dsConfirm, setDsConfirm] = useState(false);
  const [dsBackupPath, setDsBackupPath] = useState<string | null>(null);

  // 2) MessagingService Broadcaster
  const [msgTopic, setMsgTopic] = useState("ServerAnnouncement");
  const [msgPayloadText, setMsgPayloadText] = useState('{\n  "title": "Double Coins Weekend",\n  "multiplier": 2\n}');
  const [msgConfirm, setMsgConfirm] = useState(false);
  const [msgBusy, setMsgBusy] = useState(false);
  const [msgError, setMsgError] = useState("");
  const [msgReceipt, setMsgReceipt] = useState<{ topic: string; at: string; target: string } | null>(null);

  // 3) Remote Config Editor
  const [rcRows, setRcRows] = useState<RemoteConfigRow[]>([]);
  const [rcBusy, setRcBusy] = useState(false);
  const [rcSaving, setRcSaving] = useState(false);
  const [rcError, setRcError] = useState("");
  const [rcPushDs, setRcPushDs] = useState(false);
  const [rcGenLuau, setRcGenLuau] = useState(true);
  const [rcSavedInfo, setRcSavedInfo] = useState<LiveOpsRemoteConfigResponse | null>(null);

  // 4) Player Telemetry, Collector Codegen, Production Sync & Live Server Errors
  const [telemetry, setTelemetry] = useState<LiveOpsTelemetrySummary | null>(null);
  const [telBusy, setTelBusy] = useState(false);
  const [telIngesting, setTelIngesting] = useState(false);
  const [telError, setTelError] = useState("");
  const [telSource, setTelSource] = useState<TelemetrySourceFilter>("all");

  const [syncSource, setSyncSource] = useState<TelemetrySyncSource>("opencloud_datastore");
  const [syncDays, setSyncDays] = useState("7");
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");
  const [syncState, setSyncState] = useState<TelemetrySyncState | null>(null);
  const [lastImportedDelta, setLastImportedDelta] = useState<number | null>(null);

  const [collectorMode, setCollectorMode] = useState<TelemetryCodegenMode>("opencloud_datastore");
  const [collectorStore, setCollectorStore] = useState("OpenRO_Telemetry");
  const [collectorFlushSecs, setCollectorFlushSecs] = useState("30");
  const [collectorRelayUrl, setCollectorRelayUrl] = useState("");
  const [collectorBusy, setCollectorBusy] = useState(false);
  const [collectorError, setCollectorError] = useState("");
  const [collectorResult, setCollectorResult] = useState<TelemetryCodegenResponse | null>(null);

  const [liveErrors, setLiveErrors] = useState<LiveErrorAggregate[]>([]);
  const [errorsBusy, setErrorsBusy] = useState(false);
  const [errorsError, setErrorsError] = useState("");

  const dsMutationLockRef = useRef(false);
  const msgPublishLockRef = useRef(false);
  const rcSaveLockRef = useRef(false);
  const telIngestLockRef = useRef(false);
  const telSyncLockRef = useRef(false);
  const collectorLockRef = useRef(false);

  const inspectEntry = useCallback(
    async (storeName: string, scopeName: string, keyName: string, kindVal: LiveOpsDataStoreKind) => {
      if (!projectId || !storeName.trim() || !keyName.trim()) return;
      setDsError("");
      setDsConflict(false);
      const r = await getLiveOpsDataStoreEntry(projectId, {
        store: storeName.trim(),
        scope: scopeName.trim() || "global",
        key: keyName.trim(),
        kind: kindVal,
      });
      if (!r.ok || !r.entry) {
        setDsError(r.error || "Could not load entry revision.");
        return;
      }
      setSelectedKey(r.entry.key);
      setSelectedRevision(r.entry.revision ?? null);
      setSelectedUpdatedAt(r.entry.updated_at ?? null);
      setDsValueText(formatJsonValue(r.entry.value));
      if (r.backups && r.backups.length > 0) {
        setDsBackupPath(r.backups[0].path);
      }
    },
    [projectId],
  );

  const loadDataStores = useCallback(async () => {
    if (!projectId) return;
    setDsBusy(true);
    setDsError("");
    setDsConflict(false);
    const r = await listLiveOpsDataStores(projectId, {
      store: dsStore.trim() || undefined,
      scope: dsScope.trim() || undefined,
      prefix: dsPrefix.trim() || undefined,
      kind: dsKind,
    });
    setDsBusy(false);
    if (!r.ok) {
      setDsError(r.error || "Could not list DataStore entries.");
      return;
    }
    const entries = Array.isArray(r.entries) ? r.entries : [];
    setDsEntries(entries);
    setDsSource(r.source || "local");
    if (entries.length > 0) {
      const current = entries.find((e) => e.key === selectedKey) ?? entries[0];
      setSelectedKey(current.key);
      setSelectedRevision(current.revision ?? null);
      setSelectedUpdatedAt(current.updated_at ?? null);
      if (current.value !== undefined && current.value !== null) {
        setDsValueText(formatJsonValue(current.value));
      } else {
        void inspectEntry(current.store, current.scope, current.key, dsKind);
      }
    }
  }, [dsKind, dsPrefix, dsScope, dsStore, inspectEntry, projectId, selectedKey]);

  const loadRemoteConfig = useCallback(async () => {
    if (!projectId) return;
    setRcBusy(true);
    setRcError("");
    const r = await getLiveOpsRemoteConfig(projectId);
    setRcBusy(false);
    if (!r.ok) {
      setRcError(r.error || "Could not load Remote Config.");
      return;
    }
    const cfg = r.config && typeof r.config === "object" && !Array.isArray(r.config) ? r.config : {};
    const rows = Object.entries(cfg).map(([k, v], idx) => {
      const inferred = inferRowKind(v);
      return { id: `rc-${idx}-${k}`, key: k, kind: inferred.kind, value: inferred.value };
    });
    setRcRows(rows);
    setRcSavedInfo(r as LiveOpsRemoteConfigResponse);
  }, [projectId]);

  const loadTelemetry = useCallback(
    async (sourceOverride?: TelemetrySourceFilter) => {
      if (!projectId) return;
      const activeSource = sourceOverride ?? telSource;
      setTelBusy(true);
      setTelError("");
      const r = await getLiveOpsTelemetrySummary(projectId, activeSource);
      setTelBusy(false);
      if (!r.ok) {
        setTelError(r.error || "Could not load player telemetry summary.");
        return;
      }
      setTelemetry({
        dau_sample: typeof r.dau_sample === "number" ? r.dau_sample : 0,
        total_events: typeof r.total_events === "number" ? r.total_events : 0,
        funnel_steps: Array.isArray(r.funnel_steps) ? r.funnel_steps : [],
        arpdau_robux: typeof r.arpdau_robux === "number" ? r.arpdau_robux : 0,
        top_sinks: Array.isArray(r.top_sinks) ? r.top_sinks : [],
        alerts: Array.isArray(r.alerts) ? r.alerts : [],
        updated_at: r.updated_at ?? null,
        source: r.source,
        sync_state: r.sync_state,
      });
      if (r.sync_state) {
        setSyncState(r.sync_state);
      }
    },
    [projectId, telSource],
  );

  const loadErrors = useCallback(
    async (sourceOverride?: TelemetrySourceFilter) => {
      if (!projectId) return;
      const activeSource = sourceOverride ?? telSource;
      setErrorsBusy(true);
      setErrorsError("");
      const r = await getLiveOpsErrors(projectId, activeSource);
      setErrorsBusy(false);
      if (!r.ok) {
        setErrorsError(r.error || "Could not load live server errors.");
        return;
      }
      setLiveErrors(Array.isArray(r.errors) ? r.errors : []);
      if (r.sync_state) {
        setSyncState(r.sync_state);
      }
    },
    [projectId, telSource],
  );

  useEffect(() => {
    if (!projectId) return;
    setDsConfirm(false);
    setMsgConfirm(false);
    setDsBackupPath(null);
    setMsgReceipt(null);
    setDsError("");
    setDsConflict(false);
    setMsgError("");
    setRcError("");
    setTelError("");
    setSyncError("");
    setSyncState(null);
    setLastImportedDelta(null);
    setCollectorError("");
    setCollectorResult(null);
    setErrorsError("");
    if (openCloudReady) {
      void loadDataStores();
    } else {
      setDsEntries([]);
    }
    void loadRemoteConfig();
    void loadTelemetry(telSource);
    void loadErrors(telSource);
  }, [projectId, openCloudReady]); // eslint-disable-line react-hooks/exhaustive-deps

  const dsParsed = useMemo(() => tryParseJson(dsValueText), [dsValueText]);
  const msgParsed = useMemo(() => tryParseJson(msgPayloadText), [msgPayloadText]);

  const rcObject = useMemo(() => {
    const out: Record<string, unknown> = {};
    for (const row of rcRows) {
      const k = row.key.trim();
      if (k) out[k] = parseRowValue(row);
    }
    return out;
  }, [rcRows]);

  const rcBytes = useMemo(() => new Blob([JSON.stringify(rcObject, null, 2)]).size, [rcObject]);
  const rcInvalidKey = useMemo(
    () => rcRows.find((r) => !LUAU_IDENT_RE.test(r.key.trim()))?.key ?? "",
    [rcRows],
  );
  const rcDuplicateKey = useMemo(() => {
    const seen = new Set<string>();
    for (const r of rcRows) {
      const k = r.key.trim();
      if (k && seen.has(k)) return k;
      seen.add(k);
    }
    return "";
  }, [rcRows]);

  if (!projectId || !ship.active) return null;

  const createNewKey = () => {
    const trimmed = dsNewKey.trim();
    if (!trimmed) return;
    setSelectedKey(trimmed);
    setSelectedRevision(null);
    setSelectedUpdatedAt(null);
    setDsValueText(dsKind === "ordered" ? "100" : '{\n  "coins": 100\n}');
    setDsNewKey("");
    setDsError("");
    setDsConflict(false);
  };

  const handleSaveEntry = async () => {
    if (dsMutationLockRef.current || !dsConfirm || !selectedKey.trim() || !dsStore.trim()) return;
    const parsed = tryParseJson(dsValueText);
    if (!parsed.ok) {
      setDsError(parsed.error);
      return;
    }
    dsMutationLockRef.current = true;
    setDsMutating(true);
    setDsError("");
    setDsConflict(false);
    try {
      const r = await setLiveOpsDataStoreEntry(projectId, {
        store: dsStore.trim(),
        scope: dsScope.trim() || "global",
        key: selectedKey.trim(),
        value: parsed.value,
        expected_revision: selectedRevision,
        confirm: true,
        kind: dsKind,
      });
      if (!r.ok) {
        if (r.httpStatus === 409) setDsConflict(true);
        setDsError(r.error || "Could not update DataStore entry.");
        return;
      }
      setDsConfirm(false);
      if (r.backup_path) setDsBackupPath(r.backup_path);
      if (r.entry) {
        setSelectedRevision(r.entry.revision ?? null);
        setSelectedUpdatedAt(r.entry.updated_at ?? null);
        setDsValueText(formatJsonValue(r.entry.value));
      }
      toast.success(`Saved ${selectedKey.trim()} to ${dsStore.trim()}`);
      await loadDataStores();
    } finally {
      dsMutationLockRef.current = false;
      setDsMutating(false);
    }
  };

  const handleDeleteEntry = async () => {
    if (dsMutationLockRef.current || !dsConfirm || !selectedKey.trim() || !dsStore.trim()) return;
    dsMutationLockRef.current = true;
    setDsMutating(true);
    setDsError("");
    setDsConflict(false);
    try {
      const r = await deleteLiveOpsDataStoreEntry(projectId, {
        store: dsStore.trim(),
        scope: dsScope.trim() || "global",
        key: selectedKey.trim(),
        expected_revision: selectedRevision,
        confirm: true,
        kind: dsKind,
      });
      if (!r.ok) {
        if (r.httpStatus === 409) setDsConflict(true);
        setDsError(r.error || "Could not delete DataStore entry.");
        return;
      }
      setDsConfirm(false);
      if (r.backup_path) setDsBackupPath(r.backup_path);
      setSelectedKey("");
      setSelectedRevision(null);
      toast.success(`Deleted ${selectedKey.trim()} (backup saved)`);
      await loadDataStores();
    } finally {
      dsMutationLockRef.current = false;
      setDsMutating(false);
    }
  };

  const handlePublishMessage = async () => {
    if (msgPublishLockRef.current || !msgConfirm || !msgTopic.trim()) return;
    const parsed = tryParseJson(msgPayloadText);
    if (!parsed.ok) {
      setMsgError(parsed.error);
      return;
    }
    msgPublishLockRef.current = true;
    setMsgBusy(true);
    setMsgError("");
    try {
      const r = await publishLiveOpsMessage(projectId, {
        topic: msgTopic.trim(),
        message: parsed.value,
        confirm: true,
      });
      if (!r.ok) {
        setMsgError(r.error || "MessagingService publish failed.");
        return;
      }
      setMsgConfirm(false);
      setMsgReceipt({
        topic: r.topic || msgTopic.trim(),
        at: r.published_at || new Date().toISOString(),
        target: r.target || "live_servers",
      });
      toast.success(`Broadcast sent to topic ${msgTopic.trim()}`);
    } finally {
      msgPublishLockRef.current = false;
      setMsgBusy(false);
    }
  };

  const handleAddRcRow = () => {
    setRcRows((prev) => [
      ...prev,
      { id: `rc-${Date.now()}-${prev.length}`, key: `Setting${prev.length + 1}`, kind: "number", value: "1" },
    ]);
  };

  const handleSaveRemoteConfig = async () => {
    if (rcSaveLockRef.current || rcInvalidKey || rcDuplicateKey || rcBytes > 65536) return;
    rcSaveLockRef.current = true;
    setRcSaving(true);
    setRcError("");
    try {
      const r = await saveLiveOpsRemoteConfig(projectId, {
        config: rcObject,
        push_to_datastore: rcPushDs,
        generate_luau: rcGenLuau,
      });
      if (!r.ok) {
        setRcError(r.error || "Could not save Remote Config.");
        return;
      }
      setRcSavedInfo(r as LiveOpsRemoteConfigResponse);
      toast.success(
        r.luau_written
          ? "Saved Remote Config and generated src/shared/LiveConfig.luau"
          : "Saved Remote Config",
      );
      if (rcPushDs) {
        void loadDataStores();
      }
    } finally {
      rcSaveLockRef.current = false;
      setRcSaving(false);
    }
  };

  const handleIngestSampleTelemetry = async () => {
    if (telIngestLockRef.current) return;
    telIngestLockRef.current = true;
    setTelIngesting(true);
    setTelError("");
    try {
      const uidBase = Math.floor(Date.now() / 1000) % 10000;
      const nowIso = new Date().toISOString();
      const sample: LiveOpsTelemetryEvent[] = [
        { kind: "onboarding", user_id: `u_${uidBase}_1`, step: "spawned", ts: nowIso },
        { kind: "funnel", user_id: `u_${uidBase}_1`, step: "first_pickup", ts: nowIso },
        { kind: "funnel", user_id: `u_${uidBase}_1`, step: "shop_opened", ts: nowIso },
        { kind: "economy", user_id: `u_${uidBase}_1`, currency: "Robux", amount: 149, step: "VIPPass", ts: nowIso },
        { kind: "onboarding", user_id: `u_${uidBase}_2`, step: "spawned", ts: nowIso },
        { kind: "funnel", user_id: `u_${uidBase}_2`, step: "first_pickup", ts: nowIso },
        { kind: "economy", user_id: `u_${uidBase}_2`, currency: "Coins", amount: 500, step: "PetEgg", ts: nowIso },
        { kind: "onboarding", user_id: `u_${uidBase}_3`, step: "spawned", ts: nowIso },
      ];
      const r = await ingestLiveOpsTelemetry(projectId, sample);
      if (!r.ok) {
        setTelError(r.error || "Failed to ingest telemetry events.");
        return;
      }
      toast.success(`Ingested ${r.ingested ?? sample.length} telemetry event(s)`);
      await loadTelemetry(telSource);
      await loadErrors(telSource);
    } finally {
      telIngestLockRef.current = false;
      setTelIngesting(false);
    }
  };

  const handleTelemetrySourceChange = (next: TelemetrySourceFilter) => {
    setTelSource(next);
    void loadTelemetry(next);
    void loadErrors(next);
  };

  const handleGenerateCollector = async () => {
    if (collectorLockRef.current) return;
    const trimmedRelay = collectorRelayUrl.trim();
    if (collectorMode === "webhook_relay" && !trimmedRelay.startsWith("https://")) {
      setCollectorError("Webhook relay mode requires an https:// relay URL.");
      return;
    }
    const parsedFlush = Number(collectorFlushSecs);
    const flushSecs = Number.isFinite(parsedFlush) ? Math.max(5, Math.min(300, Math.round(parsedFlush))) : 30;
    collectorLockRef.current = true;
    setCollectorBusy(true);
    setCollectorError("");
    try {
      const r = await generateLiveOpsTelemetryCollector(projectId, {
        mode: collectorMode,
        store_name: collectorStore.trim() || "OpenRO_Telemetry",
        flush_interval_secs: flushSecs,
        ...(collectorMode === "webhook_relay" ? { relay_url: trimmedRelay } : {}),
      });
      if (!r.ok) {
        setCollectorError(r.error || "Failed to generate telemetry collector scripts.");
        return;
      }
      const files = Array.isArray(r.files) ? r.files : [];
      const warnings = Array.isArray(r.warnings) ? r.warnings : [];
      setCollectorResult({
        ok: true,
        mode: r.mode || collectorMode,
        store_name: r.store_name || collectorStore.trim() || "OpenRO_Telemetry",
        files,
        warnings,
      });
      toast.success(`Generated ${files.length} strict Luau collector file(s)`);
    } finally {
      collectorLockRef.current = false;
      setCollectorBusy(false);
    }
  };

  const handleSyncTelemetry = async () => {
    if (telSyncLockRef.current) return;
    const parsedDays = Number(syncDays);
    const days = Number.isFinite(parsedDays) ? Math.max(1, Math.min(90, Math.round(parsedDays))) : 7;
    telSyncLockRef.current = true;
    setSyncBusy(true);
    setSyncError("");
    try {
      const r = await syncLiveOpsTelemetry(projectId, {
        source: syncSource,
        days,
        store_name: collectorStore.trim() || "OpenRO_Telemetry",
      });
      if (!r.ok) {
        setSyncError(r.error || "Could not sync production telemetry.");
        return;
      }
      const imported = typeof r.imported_count === "number" ? r.imported_count : 0;
      setLastImportedDelta(imported);
      if (r.sync_state) {
        setSyncState(r.sync_state);
      }
      if (telSource !== "sandbox" && r.summary) {
        setTelemetry({
          dau_sample: typeof r.summary.dau_sample === "number" ? r.summary.dau_sample : 0,
          total_events: typeof r.summary.total_events === "number" ? r.summary.total_events : 0,
          funnel_steps: Array.isArray(r.summary.funnel_steps) ? r.summary.funnel_steps : [],
          arpdau_robux: typeof r.summary.arpdau_robux === "number" ? r.summary.arpdau_robux : 0,
          top_sinks: Array.isArray(r.summary.top_sinks) ? r.summary.top_sinks : [],
          alerts: Array.isArray(r.summary.alerts) ? r.summary.alerts : [],
          updated_at: r.summary.updated_at ?? null,
          source: r.summary.source,
          sync_state: r.sync_state,
        });
      } else {
        await loadTelemetry(telSource);
      }
      if (telSource !== "sandbox" && Array.isArray(r.top_errors)) {
        setLiveErrors(r.top_errors);
      } else {
        await loadErrors(telSource);
      }
      toast.success(`Synced ${imported} live telemetry event(s)`);
    } finally {
      telSyncLockRef.current = false;
      setSyncBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4" data-panel="liveops" data-liveops-panel>
      {/* 1. DataStores Inspector & Safe Editor */}
      <section aria-label="DataStores Inspector and Safe Editor" className="card-stock flex flex-col gap-3 p-4" data-liveops-datastores>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <Database className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">DataStores Inspector &amp; Safe Editor</h3>
            {dsSource && (
              <Pill className="bg-tag-ops-bg px-2 py-0.5 text-xs text-tag-ops-fg">
                {dsSource === "opencloud" ? "Open Cloud v2" : dsSource === "web_engine" ? "Browser store" : "Local store"}
              </Pill>
            )}
          </div>
          <SegmentedControl aria-label="DataStore kind" size="sm">
            <SegmentedControlItem
              data-checked={dsKind === "standard" ? "" : undefined}
              aria-pressed={dsKind === "standard"}
              onClick={() => setDsKind("standard")}
            >
              Standard
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={dsKind === "ordered" ? "" : undefined}
              aria-pressed={dsKind === "ordered"}
              onClick={() => setDsKind("ordered")}
            >
              Ordered
            </SegmentedControlItem>
          </SegmentedControl>
        </div>

        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 2xl:grid-cols-4">
          <Field label="DataStore">
            <Input
              value={dsStore}
              onChange={(e) => setDsStore(e.target.value)}
              placeholder="PlayerData"
              className="font-mono text-xs"
              data-ds-store
            />
          </Field>
          <Field label="Scope">
            <Input
              value={dsScope}
              onChange={(e) => setDsScope(e.target.value)}
              placeholder="global"
              className="font-mono text-xs"
              data-ds-scope
            />
          </Field>
          <Field label="Key prefix">
            <Input
              value={dsPrefix}
              onChange={(e) => setDsPrefix(e.target.value)}
              placeholder="Filter prefix…"
              className="font-mono text-xs"
              data-ds-prefix
            />
          </Field>
          <div className="flex items-end">
            <Button
              variant="outline"
              size="default"
              loading={dsBusy}
              onClick={() => void loadDataStores()}
              data-ds-refresh
              className="w-full max-md:min-h-11"
            >
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
              {dsBusy ? "Loading…" : "Load entries"}
            </Button>
          </div>
        </div>

        {!openCloudReady && (
          <p className="rounded-sm border border-hairline bg-board/50 px-3 py-2 text-sm text-ink-2">
            Configure an Open Cloud API key in Settings and set a Universe ID in Publish to query live Roblox DataStores.
          </p>
        )}

        {dsError && (
          <div role="alert" data-ds-error className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            <span className="min-w-0 flex-1 break-words">{dsError}</span>
            <div className="flex shrink-0 items-center gap-1.5">
              {dsConflict && selectedKey && (
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => void inspectEntry(dsStore, dsScope, selectedKey, dsKind)}
                  data-ds-reload
                >
                  Reload latest revision
                </Button>
              )}
              <Button
                variant="ghost"
                size="xs"
                onClick={() => {
                  setDsError("");
                  setDsConflict(false);
                }}
              >
                Dismiss
              </Button>
            </div>
          </div>
        )}

        <div className="grid grid-cols-1 gap-4 border-t border-hairline pt-3 2xl:grid-cols-5">
          <div className="flex min-w-0 flex-col gap-2 2xl:col-span-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-xs font-medium text-ink-2">
                Entries <span className="font-mono tabular-nums text-ink">{dsEntries.length}</span>
              </span>
              <div className="flex min-w-0 flex-1 items-center justify-end gap-1.5">
                <Input
                  value={dsNewKey}
                  onChange={(e) => setDsNewKey(e.target.value)}
                  placeholder="New key…"
                  className="h-8 min-w-0 flex-1 font-mono text-xs sm:max-w-44"
                  data-ds-new-key
                />
                <Button variant="outline" size="xs" onClick={createNewKey} disabled={!dsNewKey.trim()} className="shrink-0" data-ds-new-btn>
                  <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Add
                </Button>
              </div>
            </div>

            {dsEntries.length === 0 ? (
              <p className="rounded-sm border border-hairline bg-board/40 p-3 text-sm text-ink-2">
                No entries matched this filter. Add a key or clear the prefix filter.
              </p>
            ) : (
              <ul className="divide-y divide-hairline rounded-sm border border-hairline bg-board/30" aria-label="DataStore entries">
                {dsEntries.map((entry) => {
                  const active = entry.key === selectedKey && entry.scope === (dsScope || "global");
                  return (
                    <li key={`${entry.store}:${entry.scope}:${entry.key}`}>
                      <button
                        type="button"
                        onClick={() => {
                          setSelectedKey(entry.key);
                          setSelectedRevision(entry.revision ?? null);
                          setSelectedUpdatedAt(entry.updated_at ?? null);
                          setDsError("");
                          setDsConflict(false);
                          if (entry.value !== undefined && entry.value !== null) {
                            setDsValueText(formatJsonValue(entry.value));
                          } else {
                            void inspectEntry(entry.store, entry.scope, entry.key, dsKind);
                          }
                        }}
                        data-ds-entry={entry.key}
                        className={`flex min-h-10 w-full cursor-pointer items-center justify-between gap-2 px-3 py-2 text-left text-sm transition-[background-color] duration-(--dur-press) ease-(--ease-house) ${
                          active ? "bg-blue-soft text-ink" : "text-ink-2 hover:bg-board hover:text-ink"
                        }`}
                      >
                        <span className="min-w-0 truncate font-mono text-xs font-medium text-ink">{entry.key}</span>
                        <span className="shrink-0 font-mono text-xs tabular-nums text-ink-3">
                          {entry.revision ?? "unversioned"}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="flex min-w-0 flex-col gap-2.5 2xl:col-span-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-mono text-xs font-medium text-ink">
                  {selectedKey ? `${dsStore}/${dsScope || "global"}/${selectedKey}` : "Select or add a key"}
                </span>
                {selectedRevision && (
                  <Pill className="bg-track px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2" data-ds-revision>
                    {selectedRevision}
                  </Pill>
                )}
                {selectedUpdatedAt && (
                  <span className="font-mono text-xs tabular-nums text-ink-3">{stamp(selectedUpdatedAt)}</span>
                )}
              </div>
              <Pill
                className={
                  dsBackupPath
                    ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                    : "bg-tag-neutral-bg px-2 py-0.5 text-xs text-tag-neutral-fg"
                }
                data-ds-backup
              >
                <Archive className="size-3" strokeWidth={1.5} aria-hidden />
                <span>{dsBackupPath ? `Backup: ${dsBackupPath}` : "Pre-write backup active"}</span>
              </Pill>
            </div>

            <Textarea
              value={dsValueText}
              onChange={(e) => {
                setDsValueText(e.target.value);
                if (dsError) setDsError("");
                if (dsConflict) setDsConflict(false);
              }}
              rows={6}
              spellCheck={false}
              aria-label="DataStore entry JSON value"
              className="font-mono text-xs"
              data-ds-value
            />
            {!dsParsed.ok && (
              <p role="alert" data-ds-json-error className="text-xs text-pill-pro-fg">
                {dsParsed.error}
              </p>
            )}

            <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
              <label className="flex min-h-9 cursor-pointer items-center gap-2 text-sm text-ink max-md:min-h-11">
                <input
                  type="checkbox"
                  checked={dsConfirm}
                  onChange={(e) => setDsConfirm(e.target.checked)}
                  className="size-4 shrink-0 rounded-sm accent-blue"
                  data-ds-confirm
                />
                <span>Confirm live DataStore mutation</span>
              </label>

              <div className="flex flex-wrap items-center gap-2">
                <Button
                  variant="destructive-outline"
                  size="sm"
                  disabled={!dsConfirm || !selectedKey.trim() || dsMutating}
                  onClick={() => void handleDeleteEntry()}
                  data-ds-delete
                  className="max-md:min-h-11"
                >
                  <Trash2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Delete entry
                </Button>
                <Button
                  variant="default"
                  size="sm"
                  loading={dsMutating}
                  disabled={!dsConfirm || !dsParsed.ok || !selectedKey.trim() || dsMutating}
                  onClick={() => void handleSaveEntry()}
                  data-ds-save
                  className="max-md:min-h-11"
                >
                  <Save className="size-3.5" strokeWidth={1.5} aria-hidden />
                  {dsMutating ? "Saving…" : "Save entry"}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* 2. MessagingService Broadcaster */}
      <section aria-label="MessagingService Broadcaster" className="card-stock flex flex-col gap-3 p-4" data-liveops-messaging>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <Radio className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">MessagingService Broadcaster</h3>
          </div>
          <div className="flex flex-wrap items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                setMsgTopic("LiveEvent");
                setMsgPayloadText('{\n  "event": "double_coins",\n  "duration_mins": 30\n}');
                if (msgError) setMsgError("");
              }}
            >
              Double Coins preset
            </Button>
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                setMsgTopic("ServerAnnouncement");
                setMsgPayloadText('{\n  "title": "Server Maintenance",\n  "countdown_secs": 120\n}');
                if (msgError) setMsgError("");
              }}
            >
              Maintenance preset
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-3 2xl:grid-cols-3">
          <Field label="Topic">
            <Input
              value={msgTopic}
              onChange={(e) => {
                setMsgTopic(e.target.value);
                if (msgError) setMsgError("");
              }}
              placeholder="ServerAnnouncement"
              className="font-mono text-xs"
              data-msg-topic
            />
          </Field>
          <div className="flex flex-col gap-1 2xl:col-span-2">
            <span className="text-sm font-medium text-ink-2">JSON payload</span>
            <Textarea
              value={msgPayloadText}
              onChange={(e) => {
                setMsgPayloadText(e.target.value);
                if (msgError) setMsgError("");
              }}
              rows={3}
              spellCheck={false}
              aria-label="MessagingService JSON payload"
              className="font-mono text-xs"
              data-msg-payload
            />
            {!msgParsed.ok && (
              <p role="alert" data-msg-json-error className="text-xs text-pill-pro-fg">
                {msgParsed.error}
              </p>
            )}
          </div>
        </div>

        {msgError && (
          <div role="alert" data-msg-error className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            <span className="min-w-0 flex-1 break-words">{msgError}</span>
            <Button variant="ghost" size="xs" onClick={() => setMsgError("")}>
              Dismiss
            </Button>
          </div>
        )}

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-hairline pt-3">
          <label className="flex min-h-9 cursor-pointer items-center gap-2 text-sm text-ink max-md:min-h-11">
            <input
              type="checkbox"
              checked={msgConfirm}
              onChange={(e) => setMsgConfirm(e.target.checked)}
              className="size-4 shrink-0 rounded-sm accent-blue"
              data-msg-confirm
            />
            <span>Confirm live broadcast to all active servers</span>
          </label>
          <Button
            variant="default"
            size="sm"
            loading={msgBusy}
            disabled={!msgConfirm || !msgParsed.ok || !msgTopic.trim() || msgBusy}
            onClick={() => void handlePublishMessage()}
            data-msg-publish
            className="max-md:min-h-11"
          >
            <Send className="size-3.5" strokeWidth={1.5} aria-hidden />
            {msgBusy ? "Publishing…" : "Broadcast message"}
          </Button>
        </div>

        {msgReceipt && (
          <div
            role="status"
            data-msg-receipt
            className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-pill-active-bg px-3 py-2 text-sm text-pill-active-fg"
          >
            <span className="inline-flex items-center gap-1.5">
              <Check className="size-3.5" strokeWidth={2} aria-hidden />
              Published to <span className="font-mono font-medium">{msgReceipt.topic}</span>
            </span>
            <span className="font-mono text-xs tabular-nums">
              {msgReceipt.target} {stamp(msgReceipt.at)}
            </span>
          </div>
        )}
      </section>

      {/* 3. Remote Config Editor */}
      <section aria-label="Remote Config Editor" className="card-stock flex flex-col gap-3 p-4" data-liveops-remote-config>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <SlidersHorizontal className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">Remote Config Editor</h3>
            <Pill className="bg-track px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
              {prettyBytes(rcBytes)} / 64.0 KB
            </Pill>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="xs" onClick={handleAddRcRow} data-rc-add>
              <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
              Add key
            </Button>
            <Button variant="outline" size="xs" loading={rcBusy} onClick={() => void loadRemoteConfig()} data-rc-refresh>
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
              Reload
            </Button>
          </div>
        </div>

        {rcError && (
          <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            {rcError}
          </p>
        )}
        {rcInvalidKey && (
          <p role="alert" className="rounded-sm bg-pill-warn-bg px-3 py-2 text-sm text-pill-warn-fg">
            Key <span className="font-mono">{rcInvalidKey}</span> must be a valid Luau identifier (letters, digits, underscores, not starting with a digit).
          </p>
        )}
        {rcDuplicateKey && (
          <p role="alert" className="rounded-sm bg-pill-warn-bg px-3 py-2 text-sm text-pill-warn-fg">
            Duplicate Remote Config key: <span className="font-mono">{rcDuplicateKey}</span>.
          </p>
        )}
        {rcBytes > 65536 && (
          <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            Remote Config exceeds the 64 KiB DataStore entry budget.
          </p>
        )}

        {rcRows.length === 0 ? (
          <p className="rounded-sm border border-hairline bg-board/40 p-3 text-sm text-ink-2">
            No Remote Config keys defined yet. Add a key to generate typed Luau defaults.
          </p>
        ) : (
          <div className="overflow-x-auto rounded-sm border border-hairline" data-rc-rows>
            <table className="w-full border-collapse text-left text-sm">
              <thead>
                <tr className="border-b border-hairline bg-board/50 text-xs font-medium text-ink-2">
                  <th className="px-3 py-2">Luau key</th>
                  <th className="px-3 py-2">Type</th>
                  <th className="px-3 py-2">Value</th>
                  <th className="px-3 py-2 text-right">Remove</th>
                </tr>
              </thead>
              <tbody>
                {rcRows.map((row) => (
                  <tr key={row.id} className="border-b border-hairline/70 last:border-b-0" data-rc-row={row.key}>
                    <td className="px-3 py-2">
                      <Input
                        value={row.key}
                        onChange={(e) =>
                          setRcRows((prev) => prev.map((x) => (x.id === row.id ? { ...x, key: e.target.value } : x)))
                        }
                        aria-label="Remote config key"
                        className="h-8 font-mono text-xs"
                        data-rc-key-input
                      />
                    </td>
                    <td className="px-3 py-2">
                      <select
                        value={row.kind}
                        onChange={(e) => {
                          const nextKind = e.target.value as RemoteConfigRow["kind"];
                          setRcRows((prev) =>
                            prev.map((x) =>
                              x.id === row.id
                                ? {
                                    ...x,
                                    kind: nextKind,
                                    value: nextKind === "boolean" ? "false" : nextKind === "number" ? "0" : x.value,
                                  }
                                : x,
                            ),
                          );
                        }}
                        aria-label={`Type for ${row.key}`}
                        className="h-8 rounded-lg border border-[var(--field-border)] bg-surface px-2 text-xs text-ink focus-visible:outline-2 focus-visible:outline-focus"
                      >
                        <option value="number">number</option>
                        <option value="boolean">boolean</option>
                        <option value="string">string</option>
                      </select>
                    </td>
                    <td className="px-3 py-2">
                      {row.kind === "boolean" ? (
                        <select
                          value={row.value}
                          onChange={(e) =>
                            setRcRows((prev) => prev.map((x) => (x.id === row.id ? { ...x, value: e.target.value } : x)))
                          }
                          aria-label={`Value for ${row.key}`}
                          data-rc-value={row.key}
                          className="h-8 w-full rounded-lg border border-[var(--field-border)] bg-surface px-2 text-xs text-ink focus-visible:outline-2 focus-visible:outline-focus"
                        >
                          <option value="true">true</option>
                          <option value="false">false</option>
                        </select>
                      ) : (
                        <Input
                          type={row.kind === "number" ? "number" : "text"}
                          step={row.kind === "number" ? "any" : undefined}
                          value={row.value}
                          onChange={(e) =>
                            setRcRows((prev) => prev.map((x) => (x.id === row.id ? { ...x, value: e.target.value } : x)))
                          }
                          aria-label={`Value for ${row.key}`}
                          data-rc-value={row.key}
                          className="h-8 font-mono text-xs tabular-nums"
                        />
                      )}
                    </td>
                    <td className="px-3 py-2 text-right">
                      <Button
                        variant="ghost"
                        size="xs"
                        aria-label={`Remove ${row.key}`}
                        onClick={() => setRcRows((prev) => prev.filter((x) => x.id !== row.id))}
                      >
                        <Trash2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-hairline pt-3">
          <div className="flex flex-wrap items-center gap-4">
            <label className="flex min-h-9 cursor-pointer items-center gap-2 text-sm text-ink max-md:min-h-11">
              <input
                type="checkbox"
                checked={rcPushDs}
                onChange={(e) => setRcPushDs(e.target.checked)}
                className="size-4 shrink-0 rounded-sm accent-blue"
                data-rc-push-ds
              />
              <span>Push to DataStore</span>
            </label>
            <label className="flex min-h-9 cursor-pointer items-center gap-2 text-sm text-ink max-md:min-h-11">
              <input
                type="checkbox"
                checked={rcGenLuau}
                onChange={(e) => setRcGenLuau(e.target.checked)}
                className="size-4 shrink-0 rounded-sm accent-blue"
                data-rc-gen-luau
              />
              <span>Generate src/shared/LiveConfig.luau</span>
            </label>
          </div>
          <Button
            variant="default"
            size="sm"
            loading={rcSaving}
            disabled={rcSaving || Boolean(rcInvalidKey) || Boolean(rcDuplicateKey) || rcBytes > 65536}
            onClick={() => void handleSaveRemoteConfig()}
            data-rc-save
            className="max-md:min-h-11"
          >
            <Save className="size-3.5" strokeWidth={1.5} aria-hidden />
            {rcSaving ? "Saving…" : "Save Remote Config"}
          </Button>
        </div>

        {rcSavedInfo && (
          <div
            data-rc-status
            className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-3 py-2 text-xs text-ink-2"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono text-ink">{rcSavedInfo.path || "liveops/remote-config.json"}</span>
              {(rcSavedInfo.luau_written || rcSavedInfo.luau_exists) && (
                <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                  <FileCode2 className="size-3" strokeWidth={1.5} aria-hidden />
                  <span>{rcSavedInfo.luau_path || "src/shared/LiveConfig.luau"}</span>
                </Pill>
              )}
              {rcSavedInfo.datastore_pushed && (
                <Pill className="bg-tag-ops-bg px-2 py-0.5 text-xs text-tag-ops-fg">DataStore synced</Pill>
              )}
            </div>
            {(rcSavedInfo.luau_written || rcSavedInfo.luau_exists) && onOpenCodeFile && (
              <Button
                variant="ghost"
                size="xs"
                onClick={() => onOpenCodeFile(rcSavedInfo.luau_path || "src/shared/LiveConfig.luau")}
              >
                Open LiveConfig.luau
              </Button>
            )}
          </div>
        )}
      </section>

      {/* 4. Live Player Telemetry, Collector Codegen & Production Sync */}
      <section aria-label="Live Player Telemetry and Funnel Cohort Summary" className="card-stock flex flex-col gap-3 p-4" data-liveops-telemetry>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <BarChart3 className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">Player Telemetry &amp; Funnel Cohort</h3>
          </div>
          <SegmentedControl aria-label="Telemetry Source" size="sm" className="max-w-full flex-wrap" data-telemetry-source-filter>
            <SegmentedControlItem
              data-checked={telSource === "all" ? "" : undefined}
              aria-pressed={telSource === "all"}
              onClick={() => handleTelemetrySourceChange("all")}
              data-telemetry-source="all"
            >
              All
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={telSource === "live_opencloud" ? "" : undefined}
              aria-pressed={telSource === "live_opencloud"}
              onClick={() => handleTelemetrySourceChange("live_opencloud")}
              data-telemetry-source="live_opencloud"
            >
              Live Production
            </SegmentedControlItem>
            <SegmentedControlItem
              data-checked={telSource === "sandbox" ? "" : undefined}
              aria-pressed={telSource === "sandbox"}
              onClick={() => handleTelemetrySourceChange("sandbox")}
              data-telemetry-source="sandbox"
            >
              Headless Sandbox
            </SegmentedControlItem>
          </SegmentedControl>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/30 px-3 py-2" data-telemetry-sync-status>
          <div className="flex flex-wrap items-center gap-2 text-xs text-ink-2">
            <span>
              Last sync:{" "}
              <span className="font-mono tabular-nums text-ink">
                {syncState?.last_synced_at || syncState?.last_sync_at
                  ? stamp(syncState.last_synced_at || syncState.last_sync_at || "")
                  : "Never synced"}
              </span>
            </span>
            <Pill className="bg-track px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2" data-telemetry-sync-cursor>
              {syncState?.shard_cursor || syncState?.cursor
                ? `Cursor ${syncState.shard_cursor || syncState.cursor}`
                : syncState?.status || "Cursor idle"}
            </Pill>
            {(lastImportedDelta !== null ||
              (Boolean(syncState?.last_synced_at || syncState?.last_sync_at) &&
                typeof syncState?.last_imported_count === "number")) && (
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg" data-telemetry-sync-delta>
                <span className="font-mono tabular-nums">+{lastImportedDelta ?? syncState?.last_imported_count ?? 0}</span>
                <span>imported</span>
              </Pill>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <select
              value={syncSource}
              onChange={(e) => setSyncSource(e.target.value as TelemetrySyncSource)}
              aria-label="Production telemetry sync source"
              data-telemetry-sync-source
              className="h-8 rounded-lg border border-[var(--field-border)] bg-surface px-2 text-xs text-ink focus-visible:outline-2 focus-visible:outline-focus max-md:min-h-11"
            >
              <option value="opencloud_datastore">Open Cloud DataStore</option>
              <option value="creator_analytics">Creator Analytics</option>
            </select>
            <Input
              type="number"
              min={1}
              max={90}
              value={syncDays}
              onChange={(e) => setSyncDays(e.target.value)}
              aria-label="Sync lookback days"
              data-telemetry-sync-days
              className="h-8 w-16 font-mono text-xs tabular-nums"
            />
            <Button
              variant="default"
              size="sm"
              loading={syncBusy}
              onClick={() => void handleSyncTelemetry()}
              data-telemetry-sync-btn
              className="max-md:min-h-11"
            >
              <CloudDownload className="size-3.5" strokeWidth={1.5} aria-hidden />
              {syncBusy ? "Syncing…" : "Sync Live Production Telemetry"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              loading={telIngesting}
              onClick={() => void handleIngestSampleTelemetry()}
              data-telemetry-ingest-sample
              className="max-md:min-h-11"
            >
              <Activity className="size-3.5" strokeWidth={1.5} aria-hidden />
              {telIngesting ? "Ingesting…" : "Ingest sample batch"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              loading={telBusy}
              onClick={() => {
                void loadTelemetry(telSource);
                void loadErrors(telSource);
              }}
              data-telemetry-refresh
              className="max-md:min-h-11"
            >
              <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
              Refresh
            </Button>
          </div>
        </div>

        {syncError && (
          <p role="alert" data-telemetry-sync-error className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            {syncError}
          </p>
        )}

        {telError && (
          <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            {telError}
          </p>
        )}

        <div className="flex flex-col gap-3 rounded-sm border border-hairline bg-board/30 p-3" data-telemetry-collector>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Code2 className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
              <h4 className="text-sm font-medium text-ink">Install Live Telemetry Collector</h4>
            </div>
            <SegmentedControl aria-label="Collector mode" size="sm" className="max-w-full flex-wrap">
              <SegmentedControlItem
                data-checked={collectorMode === "opencloud_datastore" ? "" : undefined}
                aria-pressed={collectorMode === "opencloud_datastore"}
                onClick={() => {
                  setCollectorMode("opencloud_datastore");
                  if (collectorError) setCollectorError("");
                }}
                data-collector-mode="opencloud_datastore"
              >
                Open Cloud DataStore
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={collectorMode === "webhook_relay" ? "" : undefined}
                aria-pressed={collectorMode === "webhook_relay"}
                onClick={() => {
                  setCollectorMode("webhook_relay");
                  if (collectorError) setCollectorError("");
                }}
                data-collector-mode="webhook_relay"
              >
                HTTPS Webhook Relay
              </SegmentedControlItem>
            </SegmentedControl>
          </div>

          <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
            <Field label="DataStore name">
              <Input
                value={collectorStore}
                onChange={(e) => setCollectorStore(e.target.value)}
                placeholder="OpenRO_Telemetry"
                className="font-mono text-xs"
                data-collector-store
              />
            </Field>
            <Field label="Flush interval (secs)">
              <Input
                type="number"
                min={5}
                max={300}
                value={collectorFlushSecs}
                onChange={(e) => setCollectorFlushSecs(e.target.value)}
                placeholder="30"
                className="font-mono text-xs tabular-nums"
                data-collector-interval
              />
            </Field>
            {collectorMode === "webhook_relay" ? (
              <Field label="HTTPS relay URL">
                <Input
                  value={collectorRelayUrl}
                  onChange={(e) => {
                    setCollectorRelayUrl(e.target.value);
                    if (collectorError) setCollectorError("");
                  }}
                  placeholder="https://telemetry.example.com/ingest"
                  className="font-mono text-xs"
                  data-collector-relay-url
                />
              </Field>
            ) : (
              <div className="flex items-end">
                <Button
                  variant="outline"
                  size="default"
                  loading={collectorBusy}
                  onClick={() => void handleGenerateCollector()}
                  data-telemetry-codegen-btn
                  className="w-full max-md:min-h-11"
                >
                  <FileCode2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                  {collectorBusy ? "Generating…" : "Install Live Telemetry Collector"}
                </Button>
              </div>
            )}
          </div>

          {collectorMode === "webhook_relay" && (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-xs text-ink-2">
                Generates strict Luau client and server hooks flushing batches over HttpService:PostAsync.
              </span>
              <Button
                variant="outline"
                size="sm"
                loading={collectorBusy}
                onClick={() => void handleGenerateCollector()}
                data-telemetry-codegen-btn
                className="max-md:min-h-11"
              >
                <FileCode2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                {collectorBusy ? "Generating…" : "Install Live Telemetry Collector"}
              </Button>
            </div>
          )}

          {collectorError && (
            <p role="alert" data-telemetry-codegen-error className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
              {collectorError}
            </p>
          )}

          {collectorResult && (
            <div
              data-telemetry-codegen-result
              className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="font-medium text-ink">
                  Generated strict Luau collector ({collectorResult.mode})
                </span>
                {collectorResult.store_name && (
                  <Pill className="bg-tag-ops-bg px-2 py-0.5 font-mono text-xs text-tag-ops-fg">
                    {collectorResult.store_name}
                  </Pill>
                )}
              </div>
              {collectorResult.files.length > 0 && (
                <ul className="flex flex-col gap-1">
                  {collectorResult.files.map((file) => (
                    <li
                      key={file}
                      data-telemetry-codegen-file={file}
                      className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-board/40 px-2 py-1"
                    >
                      <span className="min-w-0 truncate font-mono text-ink">{file}</span>
                      {onOpenCodeFile && (
                        <Button variant="ghost" size="xs" onClick={() => onOpenCodeFile(file)}>
                          Open script
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {collectorResult.warnings.length > 0 && (
                <ul className="flex flex-col gap-1">
                  {collectorResult.warnings.map((w, idx) => (
                    <li key={idx} className="rounded-sm bg-pill-warn-bg px-2 py-1 text-pill-warn-fg">
                      {w}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>

        <div className="grid grid-cols-1 divide-y divide-hairline rounded-sm border border-hairline bg-board/30 sm:grid-cols-3 sm:divide-x sm:divide-y-0">
          <div className="flex flex-col gap-0.5 px-3 py-2.5">
            <span className="text-xs text-ink-2">DAU sample</span>
            <span className="font-mono text-title font-medium tabular-nums text-ink" data-telemetry-dau>
              {telemetry?.dau_sample ?? 0}
            </span>
          </div>
          <div className="flex flex-col gap-0.5 px-3 py-2.5">
            <span className="text-xs text-ink-2">ARPDAU (Robux)</span>
            <span className="font-mono text-title font-medium tabular-nums text-ink" data-telemetry-arpdau>
              R$ {(telemetry?.arpdau_robux ?? 0).toFixed(2)}
            </span>
          </div>
          <div className="flex flex-col gap-0.5 px-3 py-2.5">
            <span className="text-xs text-ink-2">Events ingested</span>
            <span className="font-mono text-title font-medium tabular-nums text-ink" data-telemetry-events>
              {telemetry?.total_events ?? 0}
            </span>
          </div>
        </div>

        <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
          <div className="flex flex-col gap-2" data-telemetry-funnel>
            <h4 className="text-xs font-medium text-ink-2">Onboarding &amp; Funnel Drop-off</h4>
            {(telemetry?.funnel_steps.length ?? 0) === 0 ? (
              <p className="rounded-sm border border-hairline bg-board/40 p-3 text-sm text-ink-2">
                No funnel steps recorded yet. Trigger a sample batch or sync live production telemetry.
              </p>
            ) : (
              <ul className="flex flex-col gap-2">
                {telemetry!.funnel_steps.map((step) => {
                  const pct = Math.max(0, Math.min(100, Math.round(step.pct)));
                  const drop = Math.max(0, Math.round(step.drop_off_pct ?? 0));
                  return (
                    <li key={step.step} className="flex flex-col gap-1">
                      <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                        <span className="font-mono font-medium text-ink">{step.step}</span>
                        <div className="flex items-center gap-2">
                          <span className="font-mono tabular-nums text-ink-2">
                            {step.users} users ({pct}%)
                          </span>
                          {drop > 0 && (
                            <Pill className={drop >= 40 ? "bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg" : "bg-track px-1.5 py-0 text-xs text-ink-2"}>
                              -{drop}%
                            </Pill>
                          )}
                        </div>
                      </div>
                      <div className="h-2 w-full overflow-hidden rounded-full bg-track">
                        <div className="h-full rounded-full bg-blue" style={{ width: `${pct}%` }} />
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="flex flex-col gap-2" data-telemetry-sinks>
            <h4 className="text-xs font-medium text-ink-2">Top Currency Sinks</h4>
            {(telemetry?.top_sinks.length ?? 0) === 0 ? (
              <p className="rounded-sm border border-hairline bg-board/40 p-3 text-sm text-ink-2">
                No currency spend events recorded yet.
              </p>
            ) : (
              <ul className="divide-y divide-hairline rounded-sm border border-hairline bg-board/30">
                {telemetry!.top_sinks.map((sink, idx) => {
                  const name = sink.label || [sink.currency, sink.item].filter(Boolean).join(":") || `sink_${idx + 1}`;
                  return (
                    <li key={name} className="flex items-center justify-between gap-2 px-3 py-2 text-xs">
                      <span className="min-w-0 truncate font-mono text-ink">{name}</span>
                      <div className="flex shrink-0 items-center gap-2 font-mono tabular-nums text-ink-2">
                        <span className="font-medium text-ink">{sink.amount.toLocaleString()}</span>
                        <span>({sink.events} tx)</span>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>

        <div className="flex flex-col gap-1.5" data-telemetry-alerts>
          <h4 className="text-xs font-medium text-ink-2">Telemetry Alerts</h4>
          {(telemetry?.alerts.length ?? 0) === 0 ? (
            <p className="rounded-sm border border-hairline bg-board/40 px-3 py-2 text-sm text-ink-2">
              No active funnel or economy alerts.
            </p>
          ) : (
            <ul className="flex flex-col gap-1.5">
              {telemetry!.alerts.map((al, idx) => {
                const high = al.severity === "high" || al.severity === "error";
                const warn = al.severity === "warn" || al.severity === "warning" || al.severity === "medium";
                return (
                  <li
                    key={`${al.title}-${idx}`}
                    className={`flex items-start gap-2 rounded-sm px-3 py-2 text-sm ${
                      high
                        ? "bg-pill-pro-bg text-pill-pro-fg"
                        : warn
                          ? "bg-pill-warn-bg text-pill-warn-fg"
                          : "bg-board text-ink-2"
                    }`}
                  >
                    <AlertTriangle className="mt-0.5 size-4 shrink-0" strokeWidth={1.5} aria-hidden />
                    <div className="min-w-0">
                      <p className="font-medium">{al.title}</p>
                      <p className="text-xs">{al.detail}</p>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      {/* 5. Live Server Errors */}
      <section aria-label="Live Server Errors" className="card-stock flex flex-col gap-3 p-4" data-liveops-errors>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <Bug className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">Live Server Errors</h3>
            <Pill className="bg-track px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2" data-errors-count>
              {liveErrors.length}
            </Pill>
            <Pill className="bg-tag-ops-bg px-2 py-0.5 text-xs text-tag-ops-fg">
              {telSource === "live_opencloud"
                ? "Live Production"
                : telSource === "sandbox"
                  ? "Headless Sandbox"
                  : "All sources"}
            </Pill>
          </div>
          <Button
            variant="outline"
            size="sm"
            loading={errorsBusy}
            onClick={() => void loadErrors(telSource)}
            data-errors-refresh
            className="max-md:min-h-11"
          >
            <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Refresh
          </Button>
        </div>

        {errorsError && (
          <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
            {errorsError}
          </p>
        )}

        {liveErrors.length === 0 ? (
          <p className="rounded-sm border border-hairline bg-board/40 p-3 text-sm text-ink-2" data-errors-empty>
            No server script errors recorded for this telemetry source.
          </p>
        ) : (
          <ul className="divide-y divide-hairline rounded-sm border border-hairline bg-board/30" data-errors-list>
            {liveErrors.map((err) => (
              <li
                key={err.fingerprint}
                data-error-item={err.fingerprint}
                className="flex flex-col gap-2 px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0 flex-1 space-y-1">
                  <p className="break-words text-sm font-medium text-ink">{err.message}</p>
                  <div className="flex flex-wrap items-center gap-2 text-xs text-ink-3">
                    <span className="font-mono">{err.fingerprint}</span>
                    <span>
                      Last seen{" "}
                      <span className="font-mono tabular-nums">{stamp(err.last_seen) || err.last_seen}</span>
                    </span>
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-2 sm:shrink-0">
                  <Pill
                    className="bg-pill-pro-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-pro-fg"
                    data-error-count
                  >
                    {err.count} {err.count === 1 ? "event" : "events"}
                  </Pill>
                  {err.script_path &&
                    (onOpenCodeFile ? (
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={() => onOpenCodeFile(err.script_path!)}
                        data-error-jump={err.script_path}
                        className="max-w-full font-mono max-md:min-h-11"
                      >
                        <FileCode2 className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden />
                        <span className="truncate">{err.script_path}</span>
                      </Button>
                    ) : (
                      <Pill className="bg-track px-2 py-0.5 font-mono text-xs text-ink-2">
                        {err.script_path}
                      </Pill>
                    ))}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
