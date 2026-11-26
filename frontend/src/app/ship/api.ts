// Data layer for the project features (docs/API-V011.md). Every call answers { ok, status, error? } and never throws.
export type Build = {
  build_id: string; engine: "rojo" | "builtin"; path: string; bytes: number; sha256: string;
  instances: number; scripts: number; warnings: string[]; duration_ms: number; at: string | number;
};
export type Product = {
  key: string; kind: "developer_product" | "game_pass"; name: string; description?: string;
  priceRobux: number; forSale?: boolean; icon?: string; id?: number | string | null; priceApproved?: boolean;
};
export type Summary = {
  id: string; name: string; path: string; universeId?: number | null; placeId?: number | null;
  has_manifest: boolean; has_rojo: boolean; last_build: Build | null;
};
export type Detail = Summary & {
  manifest?: { products?: Product[]; assets?: { authorised?: string[] } } & Record<string, unknown>;
  tree?: { instances: number; scripts: number; services: string[] };
  skipped?: string[];
  assets?: { total: number; authorised: number; uploaded: number };
  products?: Product[];
};
export type DiffChange = { op: "create" | "update" | "extra"; path: string; class?: string; reason?: string };
export type Diff = { summary: { create: number; update: number; unchanged: number; extra: number }; changes: DiffChange[]; chunks?: number };
export type SyncResult = {
  summary?: Diff["summary"]; applied?: { created: number; updated: number; deleted: number };
  errors?: string[]; chunks?: number; verify?: { remaining: number };
};
export type OpenCloud = { has_key: boolean; key_masked?: string; creator?: { userId?: number; groupId?: number }; base?: string; scopes_needed?: string[] };
export type AssetFile = {
  path: string; kind: string; bytes: number; sha256: string; authorised: boolean; stale?: boolean;
  locked: { assetId: number | string; state?: string; sha256?: string } | null;
};
export type UploadResult = { path: string; state: "uploaded" | "reused" | "unchanged" | "skipped" | "failed"; assetId?: number | string; error?: string };
export type PlaytestRun = { run_id: string; build_id: string; passed: boolean; at: string | number; checks: number; errors: number };
export type CoreLoopClaim = {
  status: string;
  tested?: boolean;
  passed?: boolean;
  checks?: string[];
};
export type AssertionItem = {
  id: string;
  room: string;
  label: string;
  passed: boolean;
  status: "passed" | "failed" | "skipped" | string;
  detail: string;
  delta?: "fixed" | "regressed" | "new_pass" | "new_fail" | "unchanged_pass" | "unchanged_fail" | string;
};
export type AssertionRoomSummary = {
  room: string;
  passed: number;
  total: number;
  fixed: number;
  regressed: number;
};
export type AssertionDiff = {
  has_previous: boolean;
  passed: number;
  total: number;
  fixed: number;
  regressed: number;
  rooms: AssertionRoomSummary[];
  items: AssertionItem[];
};
export type AssertionFixPlan = {
  assertion_id: string;
  room: string;
  title: string;
  target_file: string;
  auto_fixable: boolean;
  summary: string;
  diff_preview: string;
  steps: string[];
};
export type AssertionFixApplyResult = {
  plan: AssertionFixPlan;
  applied_files: string[];
  rerun: PlaytestReport & { run_id: string; passed: boolean; assertion_diff?: AssertionDiff };
};
export type FixAllAssertionsResult = {
  applied_files: string[];
  fixed_ids: string[];
  build: Build & { ok?: boolean; error?: string };
  ship_pack_integrity: ShipPackIntegrity;
  export_ready: boolean;
  export_urls: { rbxlx: string; ship_pack: string };
  rerun: PlaytestReport & { run_id: string; passed: boolean; assertion_diff?: AssertionDiff };
};
export type EconomyFunnelStage = {
  stage: string;
  rate_pct: number;
  note: string;
};
export type EconomyBlocker = {
  id: string;
  severity: "high" | "medium" | "low" | string;
  title: string;
  impact: string;
  fix_action: string;
};
export type EconomyFunnelReport = {
  schema: string;
  tier_preset: string;
  preset_label: string;
  currency: string;
  place_signals: {
    script_count: number;
    instance_count: number;
    estimated_loop_minutes: number;
    starter_items_count: number;
    loop_systems_count: number;
    game_passes_count: number;
    dev_products_count: number;
    avg_pass_robux: number;
    avg_prod_robux?: number;
    avg_product_robux?: number;
    unapproved_prices: number;
    has_process_receipt: boolean;
    has_daily_retention: boolean;
    has_comeback_hook: boolean;
    has_hud: boolean;
    has_shop: boolean;
  };
  projections: {
    arpdau_robux: number;
    arpdau_usd_est: number;
    d1_retention_pct: number;
    d7_retention_pct: number;
    d1_lift_pct: number;
    d7_lift_pct: number;
    gamepass_conversion_pct: number;
  };
  funnel_stages: EconomyFunnelStage[];
  blockers: EconomyBlocker[];
};
export type DependencyGraphReport = {
  schema: string;
  summary: {
    scripts: number;
    modules: number;
    edges: number;
    remotes: number;
    broken_count: number;
    unused_count: number;
    orphan_count: number;
    healthy: boolean;
  };
  nodes: { id: string; rel_file?: string | null; kind: string }[];
  edges: { from: string; to: string; resolved: boolean; line: number }[];
  broken_requires: { from: string; rel_file?: string | null; line: number; raw_expr: string; resolved_target: string; suggested_target?: string | null }[];
  unused_modules: { path: string; rel_file?: string | null; reason: string }[];
  remotes: { name: string; kind: string; created: boolean; wired: boolean }[];
  orphan_remotes: { name: string; kind: string; script: string; line: number; reason: string }[];
};
export type DependencyPruneResult = {
  pruned_modules: number;
  fixed_requires: number;
  pruned_remotes: number;
  applied_files?: string[];
  applied_scripts?: string[];
  before: DependencyGraphReport;
  after: DependencyGraphReport;
};
export type ProjectSnapshotScriptChange = {
  path: string;
  op: "added" | "modified" | "removed" | string;
  before_lines: number;
  after_lines: number;
};
export type ProjectSnapshotAssertionDelta = {
  passed_delta: number;
  prev_passed_count: number;
  curr_passed_count: number;
  newly_passed: string[];
  newly_failed: string[];
};
export type ProjectSnapshotItem = {
  snapshot_id: string;
  label: string;
  created_at_ms: number;
  scripts_count: number;
  verified: boolean;
  assertion_summary?: {
    passed?: boolean;
    total?: number;
    passed_count?: number;
    failed_count?: number;
    assertions?: { id: string; label?: string; passed: boolean; message?: string }[];
  };
  changed_scripts: ProjectSnapshotScriptChange[];
  assertion_delta: ProjectSnapshotAssertionDelta;
};
export type ProjectSnapshotsResponse = {
  schema?: string;
  latest_verified_id?: string | null;
  snapshots: ProjectSnapshotItem[];
};
export type ProjectSnapshotRestoreResult = {
  restored_id: string;
  label?: string;
  verified?: boolean;
  restored_files?: string[];
  removed_files?: string[];
  rerun?: PlaytestReport;
  snapshots?: ProjectSnapshotItem[];
  latest_verified_id?: string | null;
};
export type HookIntegrityIssue = {
  id: string;
  category: "monetization" | "spawn" | "strict_luau" | "core" | string;
  kind: string;
  file: string;
  label: string;
  detail: string;
};
export type HookIntegrityReport = {
  schema?: string;
  has_baseline: boolean;
  baseline_snapshot_id?: string | null;
  baseline_label?: string | null;
  healthy: boolean;
  missing_count: number;
  preserved_new_scripts: string[];
  missing_hooks: HookIntegrityIssue[];
};
export type HookRestoreResult = {
  restored_id: string;
  label?: string;
  hooks_restored: number;
  restored_files: string[];
  modified_files: string[];
  preserved_new_scripts: string[];
  after?: HookIntegrityReport;
  rerun?: PlaytestReport;
};
export type SyncConflictHunkLine = {
  op: "context" | "add" | "del" | string;
  text: string;
  old_line?: number | null;
  new_line?: number | null;
};
export type SyncConflictHunk = {
  header: string;
  lines: SyncConflictHunkLine[];
};
export type SyncConflictItem = {
  path: string;
  place_path: string;
  status: "modified" | "added" | "removed" | string;
  added_lines: number;
  removed_lines: number;
  local_source: string;
  place_source: string;
  hunks: SyncConflictHunk[];
};
export type SyncConflictsReport = {
  schema?: string;
  has_baseline: boolean;
  baseline_id?: string | null;
  in_sync: boolean;
  conflict_count: number;
  conflicts: SyncConflictItem[];
};
export type SyncConflictResolveResult = {
  action: "accept_local" | "keep_place" | "backup_snapshot" | string;
  path: string;
  backup_snapshot_id?: string | null;
  in_sync: boolean;
  conflict_count: number;
  conflicts: SyncConflictItem[];
  report?: SyncConflictsReport;
};
export type PublishGuardIssue = {
  id: string;
  category: "assets" | "remotes" | "security" | "hooks" | string;
  severity: "error" | "warning" | string;
  title: string;
  detail: string;
  file: string;
  place_path?: string;
  line: number;
  autofixable: boolean;
  fix_action: string;
  fix_hint: string;
};
export type PublishGuardReport = {
  schema?: string;
  ready_to_publish: boolean;
  blocker_count: number;
  warning_count: number;
  autofixable_count: number;
  checks: {
    assets: boolean;
    remotes: boolean;
    security: boolean;
    hooks: boolean;
  };
  issues: PublishGuardIssue[];
};
export type PublishGuardAutofixResult = {
  fixed_count: number;
  modified_files: string[];
  report?: PublishGuardReport;
};
export type MonetizationAuditIssue = {
  id: string;
  category: "process_receipt" | "bind_to_close" | "gamepass_pcall" | "onboarding_retention" | string;
  severity: "error" | "warning" | string;
  title: string;
  detail: string;
  file: string;
  place_path?: string;
  line: number;
  autofixable: boolean;
  fix_action: string;
  fix_hint: string;
  snippet?: string;
};
export type MonetizationAuditReport = {
  schema?: string;
  ready: boolean;
  blocker_count: number;
  warning_count: number;
  autofixable_count: number;
  checks: {
    process_receipt: boolean;
    bind_to_close: boolean;
    gamepass_pcall: boolean;
    onboarding_retention: boolean;
  };
  issues: MonetizationAuditIssue[];
};
export type MonetizationAuditRepairResult = {
  schema?: string;
  fixed_count: number;
  modified_files: string[];
  report?: MonetizationAuditReport;
};
export type EconomySimCheckpoint = {
  minute: number;
  f2p_earned: number;
  f2p_tiers: number;
  f2p_rebirths: number;
  pass_earned: number;
  pass_tiers: number;
  pass_rebirths: number;
};
export type EconomySimIssue = {
  id: string;
  category: string;
  severity: string;
  title: string;
  detail: string;
  file: string;
  place_path?: string;
  line: number;
  fix_hint: string;
};
export type EconomySimPatch = {
  symbol: string;
  before: number;
  after: number;
  file: string;
  line: number;
};
export type EconomySimReport = {
  schema?: string;
  balanced: boolean;
  blocker_count: number;
  constants: {
    earn_per_min: number;
    min_price: number;
    max_price: number;
    rebirth_cost: number;
    rebirth_mult: number;
    gamepass_mult: number;
    tiers_count: number;
  };
  summary: {
    time_to_first_tier_min: number | null;
    time_to_rebirth_min: number | null;
    time_to_exhaustion_min: number | null;
    f2p_completion_pct: number;
    pass_completion_pct: number;
    f2p_total_60m: number;
    pass_total_60m: number;
    f2p_vs_pass_ratio: number;
    inflation_ratio: number;
  };
  curve: EconomySimCheckpoint[];
  issues: EconomySimIssue[];
  suggestion: {
    diff_preview: string;
    target_constants: {
      earn_per_min: number;
      rebirth_cost: number;
      rebirth_mult: number;
      gamepass_mult: number;
      prices: number[];
    };
    projected_summary: {
      time_to_rebirth_min: number | null;
      time_to_exhaustion_min: number | null;
      f2p_completion_pct: number;
      inflation_ratio: number;
    };
    projected_curve: EconomySimCheckpoint[];
    patches: EconomySimPatch[];
  };
};
export type EconomySimPostResult = {
  schema?: string;
  applied: boolean;
  fixed_count: number;
  modified_files: string[];
  report?: EconomySimReport;
};
export type PlaytestReport = {
  run_id: string; passed: boolean; build_id?: string; mode?: string; duration_ms?: number;
  checks?: { name: string; passed: boolean; value?: unknown; error?: string | null; datamodel?: string }[];
  assertions?: AssertionItem[];
  assertion_diff?: AssertionDiff;
  console?: { lines: number; errors: string[]; warnings: string[] };
  screenshots?: { name: string; file: string; bytes?: number; width?: number; height?: number }[];
  core_loop?: {
    move?: CoreLoopClaim;
    act?: CoreLoopClaim;
    hud?: CoreLoopClaim;
    all_core_tested?: boolean;
    all_core_passed?: boolean;
    require_core_loop?: boolean;
  };
  blank_screenshots?: number;
  visual_likeness?: { best_pct?: number | null; best_reference?: string | null; references_compared?: number };
  kept?: boolean;
  cached?: boolean;
};
export type StudyStage = { id: string; label: string; done: boolean };
export type StudyStatus = {
  stage: "study" | "build" | "check" | "ready";
  stages: StudyStage[];
  clean_room: boolean;
  fingerprint?: string;
  evidence?: { total: number; observed: number; derived: number; inferred: number };
  unknowns?: { revision?: number; total: number; open_count: number; critical_open: number; high_open: number };
  mechanics?: {
    found: boolean;
    passed: boolean;
    path?: string;
    counts?: { total: number; sourced: number; estimated: number; unsourced: number };
    warnings?: number;
  };
  checks?: {
    static_check_kept?: boolean;
    playtest_kept?: boolean;
    last_playtest?: {
      core_loop?: PlaytestReport["core_loop"];
      visual_likeness?: PlaytestReport["visual_likeness"];
      blank_screenshots?: number;
    } | null;
  };
};
export type UnknownItem = { id: string; title: string; severity?: string; status: string; domain?: string };
export type CreatorHubExperience = { universeId: string; rootPlaceId?: string; name: string };
export type CreatorHubStatus = {
  cdp_reachable: boolean;
  browser_version?: string;
  logged_in?: boolean;
  user?: { id: string | number; name: string; displayName?: string };
  experiences?: CreatorHubExperience[];
  blocked_reason?: string;
  error?: string;
};
export type CreatorHubSetupResult = {
  status?: "ready" | "action_required";
  has_key?: boolean;
  key_masked?: string;
  creator?: { userId?: number; groupId?: number };
  universeId?: number | string;
  placeId?: number | string;
  experience_name?: string;
  blocked_reason?: string;
  url?: string;
  message?: string;
  error?: string;
};
export type CreatorHubConfigureResult = {
  status?: string;
  configured?: string[];
  manual_steps_remaining?: { step: string; url?: string }[];
  blocked_reason?: string;
  url?: string;
  message?: string;
  error?: string;
};

export type ReadinessCheck = {
  id: string;
  pillar?: string;
  weight?: number;
  earned?: number;
  label: string;
  ok: boolean;
  auto_fixable?: boolean;
  fix_action?: string | null;
  cta_section?: "playtests" | "publish";
  cta_action?: "open_settings";
  detail: string;
};
export type ReadinessDiagnostic = {
  path: string;
  line: number;
  severity: "error" | "warning";
  code: string;
  message: string;
};
export type ShipPackIntegrity = {
  ok: boolean;
  entry_count: number;
  total_bytes: number;
  crc_verified: boolean;
  zip_slip_clean: boolean;
  error?: string;
};
export type MonetizationParity = {
  ok: boolean;
  synced_products: number;
  missing_keys: string[];
  unapproved_prices: number;
  has_product_ids: boolean;
};
export type VerificationTier = "headless_sandbox" | "live_studio" | string;
export type ProjectReadiness = {
  ready_to_publish: boolean;
  score_pct: number;
  passed_count: number;
  total_count: number;
  auto_fixable_count: number;
  fixable_actions: string[];
  verification_tier?: VerificationTier;
  blockers?: ReadinessCheck[];
  pillars?: Record<string, { label: string; earned: number; max: number }>;
  playtest_summary?: {
    tested: boolean;
    passed: boolean;
    blank_screenshots: number;
    move: string;
    act: string;
    hud: string;
    verification_tier?: VerificationTier;
  };
  luau_errors: number;
  luau_warnings: number;
  has_process_receipt: boolean;
  has_daily_retention: boolean;
  locked_assets?: number;
  has_asset_ids?: boolean;
  has_product_ids?: boolean;
  unapproved_prices: number;
  monetization_parity?: MonetizationParity;
  ship_pack_integrity?: ShipPackIntegrity;
  diagnostics?: ReadinessDiagnostic[];
  checks: ReadinessCheck[];
};
export type ReadinessStatus = ProjectReadiness;
export type AutopilotResult = {
  before_score?: number;
  after_score?: number;
  score_delta?: number;
  pillar_deltas?: Record<string, { label: string; before: number; after: number; delta: number; max: number }>;
  actions_taken: { action: string; count?: number; files?: string[]; file?: string; ok?: boolean; error?: string }[];
  remaining_manual: string[];
  readiness?: ProjectReadiness;
};
export type MonetizationCodegenResult = {
  tier_preset?: string;
  comeback_multiplier?: number;
  retention_curve?: number[];
  written: string[];
  seeded_products: boolean;
  product_count: number;
  luau_errors: number;
  luau_warnings: number;
  diagnostics?: ReadinessDiagnostic[];
  readiness?: ProjectReadiness;
};
export type PublishGateFixCard = {
  id: string;
  title: string;
  reason: string;
  fix_action: string;
};
export type PublishGateResult = {
  gate_passed: boolean;
  economy_bindings_ok: boolean;
  retention_ok: boolean;
  build_and_zip_ok: boolean;
  playtest_passed: boolean;
  parity_ok: boolean;
  verification_tier?: VerificationTier;
  checks: { id: string; label: string; ok: boolean; detail: string }[];
  fix_cards: PublishGateFixCard[];
  readiness?: ProjectReadiness;
};

export type Res<T> = { ok: boolean; httpStatus: number; error?: string } & Partial<T>;

/** GET when `body` is undefined, else POST JSON (or explicit `method`). A network, timeout, or parse failure comes back as ok:false with plain words. */
export async function call<T = Record<string, unknown>>(
  url: string,
  body?: unknown,
  options?: { timeoutMs?: number; method?: "GET" | "POST" | "PUT" | "DELETE" },
): Promise<Res<T>> {
  const timeoutMs = options?.timeoutMs ?? 25000;
  const method = options?.method ?? (body === undefined ? "GET" : "POST");
  const controller = typeof AbortController !== "undefined" ? new AbortController() : null;
  const timer = controller && timeoutMs > 0 ? window.setTimeout(() => controller.abort(), timeoutMs) : null;
  try {
    const init: RequestInit =
      method === "GET"
        ? { method: "GET", cache: "no-store", signal: controller?.signal }
        : body === undefined
          ? { method, cache: "no-store", signal: controller?.signal }
          : {
              method,
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(body),
              signal: controller?.signal,
            };
    const r = await fetch(url, init);
    const text = await r.text();
    let data: Record<string, unknown> = {};
    let parseFailed = false;
    if (text.trim().length > 0) {
      try {
        const parsed = JSON.parse(text);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          data = parsed as Record<string, unknown>;
        } else {
          parseFailed = true;
        }
      } catch {
        parseFailed = true;
      }
    }
    if (parseFailed && r.ok) {
      return {
        ok: false,
        httpStatus: r.status,
        error: "Received invalid JSON response from the OpenRO server.",
      } as Res<T>;
    }
    const error =
      typeof data.error === "string"
        ? data.error
        : r.ok
          ? undefined
          : r.status === 404
            ? "This OpenRO build does not have that feature yet."
            : `The server answered ${r.status}.`;
    return { ...(data as Partial<T>), ok: r.ok && data.ok !== false, httpStatus: r.status, error };
  } catch (err) {
    const aborted = err instanceof Error && err.name === "AbortError";
    return {
      ok: false,
      httpStatus: aborted ? 408 : 0,
      error: aborted
        ? "Request timed out while waiting for the OpenRO server."
        : "Could not reach the OpenRO server.",
    } as Res<T>;
  } finally {
    if (timer !== null) window.clearTimeout(timer);
  }
}

export const enc = encodeURIComponent;
export const projUrl = (id: string, tail = "") => `/api/projects/${enc(id)}${tail}`;

export type WorkspaceScriptEntry = {
  source: string;
  service_path?: string;
  class_name?: string;
  sha256?: string;
  lines?: number;
};

export type WorkspaceInstanceEntry = {
  class_name?: string;
  properties?: Record<string, unknown>;
  source_file?: string;
};

export type WorkspaceMeshEntry = {
  name: string;
  obj_path?: string;
  texture_path?: string | null;
  obj_url: string;
  texture_url?: string | null;
  size?: [number, number, number] | number;
  tris?: number;
};

export type WorkspaceAtmosphere = {
  density?: number;
  offset?: number;
  color?: string;
  decay?: string;
  glare?: number;
  haze?: number;
};

export type WorkspaceLighting = {
  clock_time?: number;
  brightness?: number;
  ambient?: string;
  outdoor_ambient?: string;
  fog_end?: number;
  fog_start?: number;
  fog_color?: string;
  technology?: string;
  atmosphere?: WorkspaceAtmosphere | null;
};

export type WorkspaceDiagnostic = {
  severity: "error" | "warning" | "info" | string;
  file?: string | null;
  service_path?: string;
  line: number;
  col?: number | null;
  message: string;
  code: string;
};

export type WorkspaceBundle = {
  project_id: string;
  project_name?: string;
  mode: string;
  scene_revision?: number;
  can_undo?: boolean;
  can_redo?: boolean;
  scripts: Record<string, WorkspaceScriptEntry>;
  instances: Record<string, WorkspaceInstanceEntry>;
  meshes: WorkspaceMeshEntry[];
  lighting: WorkspaceLighting;
  terrain?: unknown;
  lights?: unknown[];
  diagnostics: WorkspaceDiagnostic[];
  skipped?: string[];
  tree_error?: string | null;
};

export type SceneMutateOp = {
  op: "upsert" | "delete" | "duplicate" | "reparent" | "set_lighting" | "set_terrain" | "undo" | "redo";
  path?: string;
  new_parent?: string;
  class_name?: string;
  properties?: Record<string, unknown>;
  offset?: [number, number, number];
  new_path?: string;
  name?: string;
  enabled?: boolean;
  clear?: boolean;
  generator?: Record<string, unknown>;
  regions?: Record<string, unknown>[];
  paint_box?: Record<string, unknown>;
};

export type SceneMutateResult = {
  project_id?: string;
  scene_revision?: number;
  can_undo?: boolean;
  can_redo?: boolean;
  undone?: boolean;
  redone?: boolean;
  instances?: Record<string, WorkspaceInstanceEntry>;
  lighting?: WorkspaceLighting;
  lights?: unknown[];
  terrain?: unknown;
  modified_files?: string[];
};

export function meshAssetUrl(projectId: string, relPath: string): string {
  return projUrl(projectId, `/mesh-asset?path=${enc(relPath)}`);
}

export async function fetchProjectWorkspace(projectId: string): Promise<Res<WorkspaceBundle>> {
  return call<WorkspaceBundle>(projUrl(projectId, "/workspace"));
}

export async function mutateProjectScene(projectId: string, ops: SceneMutateOp[]): Promise<Res<SceneMutateResult>> {
  return call<SceneMutateResult>(projUrl(projectId, "/scene/mutate"), { ops });
}

export async function writeProjectFile(projectId: string, path: string, content: string, expectSha256?: string): Promise<Res<{ path: string; created: boolean; bytes: number; sha256: string }>> {
  return call(projUrl(projectId, "/file"), {
    path,
    content,
    ...(expectSha256 ? { expect_sha256: expectSha256 } : {}),
  });
}

export async function deleteProjectFile(projectId: string, path: string): Promise<Res<{ path: string; deleted: boolean }>> {
  return call(projUrl(projectId, "/file/delete"), { path });
}

export type SandboxPlayerConfig = {
  user_id: number;
  name: string;
  owns_passes?: number[];
};

export type SandboxJourneyOp =
  | "wait"
  | "fire_server"
  | "invoke_server"
  | "purchase_receipt"
  | "click_gui"
  | "rejoin"
  | "assert_luau"
  | "move_player"
  | "trigger_prompt"
  | "click_detector"
  | "key_down"
  | "key_up";

export type SandboxJourneyStep = {
  op: SandboxJourneyOp;
  ms?: number;
  remote?: string;
  player_id?: number;
  args?: unknown[];
  product_id?: number;
  purchase_id?: string;
  replay_duplicate?: boolean;
  gui_path?: string;
  name?: string;
  luau?: string;
  expect?: Record<string, unknown>;
  to?: [number, number, number];
  path?: string;
  key?: string;
};

export type SandboxHttpMock = {
  url: string;
  method?: string;
  status?: number;
  body?: unknown;
  headers?: Record<string, string>;
};

export type SandboxRunRequest = {
  timeout_secs?: number;
  players?: SandboxPlayerConfig[];
  journey?: SandboxJourneyStep[];
  http_mocks?: SandboxHttpMock[] | Record<string, unknown>;
};

export type SandboxStepResult = {
  index: number;
  op: string;
  passed: boolean;
  duration_ms: number;
  result?: unknown;
  error?: string;
};

export type SandboxAssertionResult = {
  name: string;
  passed: boolean;
  value?: unknown;
  error?: string;
};

export type SandboxConsoleOutput = {
  lines: string[];
  errors: string[];
  warnings: string[];
};

export type SandboxReceiptEntry =
  | string
  | {
      purchase_id?: string;
      product_id?: number;
      player_id?: number;
      decision?: string;
      duplicate?: boolean;
      idempotent?: boolean;
    };

export type SandboxStateSnapshot = {
  leaderstats: Record<string, unknown>;
  datastores: Record<string, unknown>;
  receipts_processed: SandboxReceiptEntry[];
  remotes_fired: number;
  player_gui_tree: unknown[];
};

export type SandboxCoreLoop = {
  move: CoreLoopClaim | string;
  act: CoreLoopClaim | string;
  hud: CoreLoopClaim | string;
  all_passed?: boolean;
};

export type SandboxRunSummary = {
  run_id: string;
  at_ms: number;
  passed: boolean;
  duration_ms: number;
  error_count: number;
  core_loop?: SandboxCoreLoop;
  engine?: string;
};

export type SandboxRunResponse = {
  ok: boolean;
  run_id?: string;
  at_ms?: number;
  project_fingerprint?: string;
  error_count?: number;
  engine: "lune_roblox_shim" | "browser_sim" | "lune_missing";
  passed: boolean;
  duration_ms: number;
  steps: SandboxStepResult[];
  assertions: SandboxAssertionResult[];
  console: SandboxConsoleOutput;
  state_snapshot: SandboxStateSnapshot;
  core_loop: SandboxCoreLoop;
  diagnostic?: string;
  diagnostics?: {
    lune_found?: boolean;
    searched_paths?: string[];
  };
};

export async function runSandbox(req: SandboxRunRequest = {}, projectId?: string | null): Promise<Res<SandboxRunResponse>> {
  const url = projectId ? projUrl(projectId, "/sandbox/run") : "/api/studio/sandbox/run";
  const timeoutMs = Math.max(8000, ((req.timeout_secs ?? 15) + 5) * 1000);
  return call<SandboxRunResponse>(url, req, { timeoutMs });
}

export async function getSandboxRuns(projectId?: string | null): Promise<Res<{ runs: SandboxRunSummary[] }>> {
  const url = projectId ? projUrl(projectId, "/sandbox/runs") : "/api/studio/sandbox/runs";
  return call<{ runs: SandboxRunSummary[] }>(url);
}

export async function getSandboxLatest(projectId?: string | null): Promise<Res<SandboxRunResponse>> {
  const url = projectId ? projUrl(projectId, "/sandbox/latest") : "/api/studio/sandbox/latest";
  return call<SandboxRunResponse>(url);
}

export type {
  GuiClassName,
  GuiDeviceId,
  GuiNode,
  GuiNodeProps,
  GuiPresetId,
  SafeAreaWarning,
  SafeAreaWarningKind,
  ScreenDoc,
  ScreenDocTheme,
  UDim2Value,
} from "../guiDesignerEngine";
import type {
  GuiDeviceId,
  SafeAreaWarning,
  SafeAreaWarningKind,
  ScreenDoc,
} from "../guiDesignerEngine";

function normalizeSafeAreaWarnings(raw: unknown): SafeAreaWarning[] {
  if (!Array.isArray(raw)) return [];
  const asNum = (v: unknown, fb: number) => (typeof v === "number" && Number.isFinite(v) ? v : fb);
  return raw
    .filter((item): item is Record<string, unknown> => Boolean(item && typeof item === "object"))
    .map((w) => {
      const rawDev = String(w.device ?? "desktop");
      const device: GuiDeviceId =
        rawDev === "mobile_landscape" || rawDev === "mobile"
          ? "mobile"
          : rawDev === "tablet"
            ? "tablet"
            : "desktop";
      const rawKind = String(w.kind ?? w.code ?? "offscreen_bounds");
      const kind: SafeAreaWarningKind =
        rawKind === "offscreen"
          ? "offscreen_bounds"
          : rawKind === "topbar_overlap" ||
              rawKind === "thumbstick_overlap" ||
              rawKind === "offscreen_bounds" ||
              rawKind === "tiny_tap_target"
            ? rawKind
            : "offscreen_bounds";
      const box = (w.rect ?? w.bounds ?? {}) as Record<string, unknown>;
      return {
        device,
        node_id: String(w.node_id ?? ""),
        node_name: String(w.node_name ?? ""),
        kind,
        severity: w.severity === "error" ? "error" : "warning",
        message: String(w.message ?? ""),
        rect: {
          x: asNum(box.x, 0),
          y: asNum(box.y, 0),
          w: asNum(box.w, 0),
          h: asNum(box.h, 0),
        },
      };
    });
}

export type GuiScreenSummary = {
  slug: string;
  name: string;
  path?: string;
  updated_at?: string | number;
};

export type GuiListResponse = {
  screens: GuiScreenSummary[];
  presets?: Array<string | { id: string; name: string; description?: string }>;
};

export type GuiLoadResponse = {
  slug: string;
  doc: ScreenDoc;
  safe_area_warnings: SafeAreaWarning[];
};

export type GuiSaveResponse = {
  slug: string;
  path?: string;
  doc: ScreenDoc;
  safe_area_warnings: SafeAreaWarning[];
};

export type GuiCompileDiagnostic = {
  severity: "error" | "warning" | "info" | string;
  line: number;
  message: string;
  code: string;
};

export type GuiCompileResponse = {
  slug: string;
  luau_source: string;
  model_json?: string;
  diagnostics: GuiCompileDiagnostic[];
  safe_area_warnings: SafeAreaWarning[];
  luau_errors?: number;
  luau_warnings?: number;
  saved_path?: string;
};

export async function listGuiScreens(projectId?: string | null): Promise<Res<GuiListResponse>> {
  const url = projectId ? projUrl(projectId, "/gui") : "/api/studio/gui";
  return call<GuiListResponse>(url);
}

export async function loadGuiScreen(slug: string, projectId?: string | null): Promise<Res<GuiLoadResponse>> {
  const url = projectId ? projUrl(projectId, `/gui/${enc(slug)}`) : `/api/studio/gui?slug=${enc(slug)}`;
  const res = await call<GuiLoadResponse>(url);
  if (!res.ok) return res;
  return {
    ...res,
    safe_area_warnings: normalizeSafeAreaWarnings(res.safe_area_warnings),
  };
}

export async function saveGuiScreen(slug: string, doc: ScreenDoc, projectId?: string | null): Promise<Res<GuiSaveResponse>> {
  const url = projectId ? projUrl(projectId, `/gui/${enc(slug)}`) : "/api/studio/gui";
  const res = await call<GuiSaveResponse>(url, { slug, doc });
  if (!res.ok) return res;
  return {
    ...res,
    safe_area_warnings: normalizeSafeAreaWarnings(res.safe_area_warnings),
  };
}

export async function compileGuiScreen(
  slug: string,
  doc: ScreenDoc,
  projectId?: string | null,
  options?: { save_luau?: boolean },
): Promise<Res<GuiCompileResponse>> {
  const url = projectId ? projUrl(projectId, `/gui/${enc(slug)}/compile`) : "/api/studio/gui/compile";
  const res = await call<Record<string, unknown>>(url, {
    slug,
    doc,
    write: options?.save_luau ?? true,
    ...(options ?? {}),
  });
  if (!res.ok) return res as unknown as Res<GuiCompileResponse>;

  const luauSource =
    typeof res.luau_source === "string"
      ? res.luau_source
      : typeof res.luau === "string"
        ? res.luau
        : "";
  const modelJson =
    typeof res.model_json === "string"
      ? res.model_json
      : res.model_json && typeof res.model_json === "object"
        ? JSON.stringify(res.model_json, null, 2)
        : undefined;
  const luauCheck = (res.luau_check && typeof res.luau_check === "object"
    ? res.luau_check
    : null) as Record<string, unknown> | null;
  const rawDiagnostics = Array.isArray(res.diagnostics)
    ? res.diagnostics
    : Array.isArray(luauCheck?.diagnostics)
      ? luauCheck.diagnostics
      : [];
  const diagnostics: GuiCompileDiagnostic[] = rawDiagnostics
    .filter((d): d is Record<string, unknown> => Boolean(d && typeof d === "object"))
    .map((d) => ({
      severity: String(d.severity ?? "info"),
      line: typeof d.line === "number" ? d.line : Number(d.line ?? 0) || 0,
      message: String(d.message ?? ""),
      code: String(d.code ?? "luau_check"),
    }));
  const savedPath =
    typeof res.saved_path === "string"
      ? res.saved_path
      : typeof res.luau_path === "string"
        ? res.luau_path
        : undefined;

  return {
    ok: true,
    httpStatus: res.httpStatus,
    slug: typeof res.slug === "string" ? res.slug : slug,
    luau_source: luauSource,
    model_json: modelJson,
    diagnostics,
    safe_area_warnings: normalizeSafeAreaWarnings(res.safe_area_warnings),
    luau_errors:
      typeof res.luau_errors === "number"
        ? res.luau_errors
        : typeof luauCheck?.error_count === "number"
          ? luauCheck.error_count
          : diagnostics.filter((d) => d.severity === "error").length,
    luau_warnings:
      typeof res.luau_warnings === "number"
        ? res.luau_warnings
        : typeof luauCheck?.warning_count === "number"
          ? luauCheck.warning_count
          : diagnostics.filter((d) => d.severity === "warning").length,
    saved_path: savedPath,
  };
}

export const prettyBytes = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`);
export const stamp = (at: string | number | undefined) => {
  if (at === undefined || at === null) return "";
  const d = new Date(typeof at === "number" && at < 1e12 ? at * 1000 : at);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
};

export type LiveOpsDataStoreKind = "standard" | "ordered";

export type LiveOpsDataStoreEntry = {
  store: string;
  scope: string;
  key: string;
  kind?: LiveOpsDataStoreKind | string;
  revision?: string | null;
  value?: unknown;
  updated_at?: string | null;
  size_bytes?: number;
};

export type LiveOpsDataStoreBackup = {
  path: string;
  created_at?: string;
  revision?: string | null;
};

export type LiveOpsDataStoreListResponse = {
  stores?: string[];
  entries: LiveOpsDataStoreEntry[];
  next_cursor?: string | null;
  source?: string;
};

export type LiveOpsDataStoreEntryResponse = {
  entry?: LiveOpsDataStoreEntry;
  backups?: LiveOpsDataStoreBackup[];
  backup_path?: string | null;
  backup_created?: boolean;
  deleted?: boolean;
  revision?: string | null;
  source?: string;
};

export type LiveOpsMessagingPublishResponse = {
  topic: string;
  published: boolean;
  published_at?: string;
  target?: string;
};

export type LiveOpsRemoteConfigResponse = {
  config: Record<string, unknown>;
  path?: string;
  luau_path?: string;
  luau_exists?: boolean;
  luau_written?: boolean;
  datastore_pushed?: boolean;
  bytes?: number;
  updated_at?: string | null;
};

export type LiveOpsTelemetryEvent = {
  kind: "onboarding" | "funnel" | "economy" | "custom" | string;
  user_id: string | number;
  step?: string;
  currency?: string;
  amount?: number;
  ts?: number | string;
};

export type LiveOpsFunnelStep = {
  step: string;
  users: number;
  pct: number;
  drop_off_pct?: number;
};

export type LiveOpsCurrencySink = {
  label?: string;
  currency?: string;
  item?: string;
  amount: number;
  events: number;
  share_pct?: number;
};

export type LiveOpsTelemetryAlert = {
  severity: "high" | "medium" | "low" | "warn" | "info" | string;
  title: string;
  detail: string;
};

export type LiveOpsTelemetrySummary = {
  dau_sample: number;
  total_events?: number;
  funnel_steps: LiveOpsFunnelStep[];
  arpdau_robux: number;
  top_sinks: LiveOpsCurrencySink[];
  alerts: LiveOpsTelemetryAlert[];
  updated_at?: string | null;
};

function buildLiveOpsQuery(params: Record<string, string | number | undefined>): string {
  const sp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && String(v).trim() !== "") {
      sp.set(k, String(v));
    }
  }
  const qs = sp.toString();
  return qs ? `?${qs}` : "";
}

export async function listLiveOpsDataStores(
  projectId: string,
  params: { store?: string; scope?: string; prefix?: string; kind?: LiveOpsDataStoreKind; limit?: number } = {},
): Promise<Res<LiveOpsDataStoreListResponse>> {
  const qs = buildLiveOpsQuery({
    store: params.store,
    scope: params.scope,
    prefix: params.prefix,
    kind: params.kind,
    limit: params.limit,
  });
  return call<LiveOpsDataStoreListResponse>(projUrl(projectId, `/liveops/datastores${qs}`));
}

export async function getLiveOpsDataStoreEntry(
  projectId: string,
  params: { store: string; scope?: string; key: string; kind?: LiveOpsDataStoreKind },
): Promise<Res<LiveOpsDataStoreEntryResponse>> {
  const qs = buildLiveOpsQuery({
    store: params.store,
    scope: params.scope || "global",
    key: params.key,
    kind: params.kind,
  });
  return call<LiveOpsDataStoreEntryResponse>(projUrl(projectId, `/liveops/datastores/entry${qs}`));
}

export async function setLiveOpsDataStoreEntry(
  projectId: string,
  payload: {
    store: string;
    scope?: string;
    key: string;
    value: unknown;
    expected_revision?: string | null;
    confirm: boolean;
    kind?: LiveOpsDataStoreKind;
  },
): Promise<Res<LiveOpsDataStoreEntryResponse>> {
  return call<LiveOpsDataStoreEntryResponse>(projUrl(projectId, "/liveops/datastores/entry"), {
    store: payload.store,
    scope: payload.scope || "global",
    key: payload.key,
    value: payload.value,
    ...(payload.expected_revision ? { expected_revision: payload.expected_revision } : {}),
    ...(payload.kind ? { kind: payload.kind } : {}),
    confirm: payload.confirm,
  });
}

export async function deleteLiveOpsDataStoreEntry(
  projectId: string,
  payload: {
    store: string;
    scope?: string;
    key: string;
    expected_revision?: string | null;
    confirm: boolean;
    kind?: LiveOpsDataStoreKind;
  },
): Promise<Res<LiveOpsDataStoreEntryResponse>> {
  const qs = buildLiveOpsQuery({
    store: payload.store,
    scope: payload.scope || "global",
    key: payload.key,
    kind: payload.kind,
  });
  return call<LiveOpsDataStoreEntryResponse>(
    projUrl(projectId, `/liveops/datastores/entry${qs}`),
    {
      store: payload.store,
      scope: payload.scope || "global",
      key: payload.key,
      ...(payload.expected_revision ? { expected_revision: payload.expected_revision } : {}),
      ...(payload.kind ? { kind: payload.kind } : {}),
      confirm: payload.confirm,
    },
    { method: "DELETE" },
  );
}

export async function publishLiveOpsMessage(
  projectId: string,
  payload: { topic: string; message: unknown; confirm: boolean },
): Promise<Res<LiveOpsMessagingPublishResponse>> {
  return call<LiveOpsMessagingPublishResponse>(projUrl(projectId, "/liveops/messaging/publish"), payload);
}

export async function getLiveOpsRemoteConfig(projectId: string): Promise<Res<LiveOpsRemoteConfigResponse>> {
  return call<LiveOpsRemoteConfigResponse>(projUrl(projectId, "/liveops/remote-config"));
}

export async function saveLiveOpsRemoteConfig(
  projectId: string,
  payload: {
    config: Record<string, unknown>;
    push_to_datastore?: boolean;
    generate_luau?: boolean;
    datastore_name?: string;
    datastore_key?: string;
  },
): Promise<Res<LiveOpsRemoteConfigResponse>> {
  return call<LiveOpsRemoteConfigResponse>(projUrl(projectId, "/liveops/remote-config"), {
    config: payload.config,
    push_to_datastore: payload.push_to_datastore,
    generate_luau: payload.generate_luau,
    ...(payload.datastore_name ? { datastore: payload.datastore_name, datastore_name: payload.datastore_name } : {}),
    ...(payload.datastore_key ? { key: payload.datastore_key, datastore_key: payload.datastore_key } : {}),
  });
}

export async function ingestLiveOpsTelemetry(
  projectId: string,
  events: LiveOpsTelemetryEvent[],
): Promise<Res<{ ingested: number; total_events?: number }>> {
  return call<{ ingested: number; total_events?: number }>(projUrl(projectId, "/liveops/telemetry/ingest"), { events });
}

export async function getLiveOpsTelemetrySummary(projectId: string): Promise<Res<LiveOpsTelemetrySummary>> {
  return call<LiveOpsTelemetrySummary>(projUrl(projectId, "/liveops/telemetry/summary"));
}

export type StudioPullChangeKind = "script" | "instance" | "lighting";
export type StudioPullChangeStatus = "studio_only" | "studio_modified" | "conflict" | "local_only";
export type StudioPullStrategy = "take_studio" | "keep_local" | "merge_non_conflicting";

export type StudioPullHunk =
  | SyncConflictHunk
  | {
      header?: string;
      lines?: SyncConflictHunkLine[];
      op?: string;
      text?: string;
      before?: unknown;
      after?: unknown;
    }
  | string;

export type StudioPullChange = {
  path: string;
  kind: StudioPullChangeKind | string;
  status: StudioPullChangeStatus | string;
  local_hash?: number | null;
  studio_hash?: number | null;
  baseline_hash?: number | null;
  hunks: StudioPullHunk[];
};

export type StudioPullResponse = {
  ok: boolean;
  snapshot_id: string;
  changes: StudioPullChange[];
  applied_count: number;
  conflict_count: number;
  applied_paths?: string[];
  skipped_paths?: string[];
};

export type PrefabModelFormat = "rbxmx" | "rbxm" | "rbxlx";

export type ImportModelRequest = {
  file_name: string;
  format: PrefabModelFormat;
  bytes_base64: string;
  target_service?: string;
};

export type ImportModelResponse = {
  ok: boolean;
  model_path: string;
  extracted_scripts: string[];
  wired_service: string;
  warnings: string[];
};

export function bytesToBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let bin = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

export function inferPrefabFormat(fileName: string): PrefabModelFormat | null {
  const ext = fileName.trim().toLowerCase().split(".").pop();
  if (ext === "rbxmx" || ext === "rbxm" || ext === "rbxlx") return ext;
  return null;
}

export async function pullSyncDiff(projectId: string): Promise<Res<StudioPullResponse>> {
  return call<StudioPullResponse>(projUrl(projectId, "/sync/pull-diff"), {});
}

export async function pullSyncApply(
  projectId: string,
  payload: {
    paths?: string[];
    strategy: StudioPullStrategy;
    confirm_overwrite?: boolean;
  },
): Promise<Res<StudioPullResponse>> {
  return call<StudioPullResponse>(projUrl(projectId, "/sync/pull"), payload);
}

export async function importProjectModel(
  projectId: string,
  payload: ImportModelRequest,
): Promise<Res<ImportModelResponse>> {
  return call<ImportModelResponse>(projUrl(projectId, "/import-model"), payload, { timeoutMs: 45000 });
}
