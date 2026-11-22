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
export type PlaytestReport = {
  run_id: string; passed: boolean; build_id?: string; mode?: string; duration_ms?: number;
  checks?: { name: string; passed: boolean; value?: unknown; error?: string | null; datamodel?: string }[];
  console?: { lines: number; errors: string[]; warnings: string[] };
  screenshots?: { name: string; file: string; bytes?: number; width?: number; height?: number }[];
};

export type Res<T> = { ok: boolean; httpStatus: number; error?: string } & Partial<T>;

/** GET when `body` is undefined, else POST JSON. A network or parse failure comes back as ok:false with plain words. */
export async function call<T = Record<string, unknown>>(url: string, body?: unknown): Promise<Res<T>> {
  try {
    const r = await fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    const text = await r.text();
    let data: Record<string, unknown> = {};
    try { data = JSON.parse(text) as Record<string, unknown>; } catch { /* not JSON */ }
    const error = typeof data.error === "string" ? data.error : r.ok ? undefined : r.status === 404 ? "This OpenRO build does not have that feature yet." : `The server answered ${r.status}.`;
    return { ...(data as Partial<T>), ok: r.ok && data.ok !== false, httpStatus: r.status, error };
  } catch {
    return { ok: false, httpStatus: 0, error: "Could not reach the OpenRO server." } as Res<T>;
  }
}

export const enc = encodeURIComponent;
export const projUrl = (id: string, tail = "") => `/api/projects/${enc(id)}${tail}`;
export const prettyBytes = (n: number) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`);
export const stamp = (at: string | number | undefined) => {
  if (at === undefined || at === null) return "";
  const d = new Date(typeof at === "number" && at < 1e12 ? at * 1000 : at);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
};
