import {
  memo,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  CheckCircle2,
  Code2,
  Copy,
  CopyPlus,
  Eye,
  EyeOff,
  FileCode2,
  Gamepad2,
  Image as ImageIcon,
  Layers,
  Laptop,
  Package,
  Plus,
  Save,
  ShieldAlert,
  Smartphone,
  Sparkles,
  Tablet,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import type { Place, ProjectAssetRecord } from "./data";
import {
  call,
  compileGuiScreen,
  fetchProjectTeardown,
  listGuiScreens,
  loadGuiScreen,
  projUrl,
  saveGuiScreen,
  teardownScreenshotPreviewUrl,
  type AssetFile,
  type GuiCompileDiagnostic,
  type GuiScreenSummary,
  type Product,
} from "./ship/api";
import {
  GUI_CLASS_ALLOWLIST,
  GUI_DEVICES,
  GUI_MODIFIER_CLASSES,
  GUI_PRESET_META,
  GUI_VISUAL_CLASSES,
  UI_KIT_LAYOUT_META,
  UI_KIT_SHEET_META,
  UI_KIT_SKIN_META,
  analyzeSafeArea,
  buildGuiPreset,
  buildUiKitScreenDoc,
  cloneGuiNodeWithNewIds,
  compileScreenDocToLuau,
  computeNodeRects,
  deleteGuiNode,
  findGuiNode,
  insertGuiChild,
  isValidGuiSlug,
  normalizeScreenDoc,
  sanitizeGuiSlug,
  udim2,
  updateGuiNode,
  type ComputedNodeRect,
  type GuiClassName,
  type GuiDeviceId,
  type GuiNode,
  type GuiNodeProps,
  type GuiPresetId,
  type SafeAreaWarning,
  type ScreenDoc,
  type UDim2Value,
  type UiKitLayoutId,
  type UiKitSheetId,
  type UiKitSkinId,
} from "./guiDesignerEngine";

type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
type InspectorTab = "props" | "bind" | "luau";

const HANDLE_CURSORS: Record<ResizeHandle, string> = {
  nw: "nwse-resize",
  n: "ns-resize",
  ne: "nesw-resize",
  e: "ew-resize",
  se: "nwse-resize",
  s: "ns-resize",
  sw: "nesw-resize",
  w: "ew-resize",
};

const HANDLE_POSITIONS: Record<ResizeHandle, string> = {
  nw: "-left-1 -top-1",
  n: "left-1/2 -top-1 -translate-x-1/2",
  ne: "-right-1 -top-1",
  e: "-right-1 top-1/2 -translate-y-1/2",
  se: "-right-1 -bottom-1",
  s: "left-1/2 -bottom-1 -translate-x-1/2",
  sw: "-left-1 -bottom-1",
  w: "-left-1 top-1/2 -translate-y-1/2",
};

const DEFAULT_ADD_CLASS_PROPS: Record<GuiClassName, GuiNodeProps> = {
  Frame: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 220, 0, 120),
    BackgroundColor3: "#181D27",
    BackgroundTransparency: 0.1,
    Visible: true,
    ZIndex: 2,
  },
  TextLabel: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 180, 0, 44),
    BackgroundColor3: "#181D27",
    BackgroundTransparency: 0.15,
    Text: "Label",
    TextColor3: "#F3F4F6",
    TextSize: 16,
    Font: "GothamMedium",
    Visible: true,
    ZIndex: 3,
  },
  TextButton: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 160, 0, 48),
    BackgroundColor3: "#1562FC",
    BackgroundTransparency: 0,
    Text: "Action",
    TextColor3: "#FFFFFF",
    TextSize: 15,
    Font: "GothamBold",
    CallbackName: "OnAction",
    Visible: true,
    ZIndex: 3,
  },
  ImageLabel: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 120, 0, 120),
    BackgroundColor3: "#181D27",
    BackgroundTransparency: 0.2,
    Image: "art/store/icon.png",
    ScaleType: "Fit",
    Visible: true,
    ZIndex: 2,
  },
  ImageButton: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 64, 0, 64),
    BackgroundColor3: "#232938",
    BackgroundTransparency: 0,
    Image: "art/store/button.png",
    ScaleType: "Fit",
    CallbackName: "OnImageClick",
    Visible: true,
    ZIndex: 3,
  },
  ScrollingFrame: {
    Position: udim2(0, 32, 0, 80),
    Size: udim2(0, 240, 0, 180),
    BackgroundColor3: "#141820",
    BackgroundTransparency: 0.12,
    CanvasSize: udim2(0, 0, 1.5, 0),
    ScrollBarThickness: 4,
    ClipsDescendants: true,
    Visible: true,
    ZIndex: 2,
  },
  UICorner: { CornerRadius: [0, 10] },
  UIStroke: { Color: "#334155", Thickness: 1.5, Transparency: 0.2, ApplyStrokeMode: "Border" },
  UIListLayout: { FillDirection: "Vertical", Padding: [0, 8], HorizontalAlignment: "Center", VerticalAlignment: "Top" },
  UIGridLayout: { CellSize: udim2(0, 84, 0, 84), CellPadding: udim2(0, 8, 0, 8) },
  UIAspectRatioConstraint: { AspectRatio: 1 },
  UIPadding: { PaddingTop: [0, 8], PaddingBottom: [0, 8], PaddingLeft: [0, 8], PaddingRight: [0, 8] },
};

function resolveAssetPreviewUrl(
  projectId: string | null | undefined,
  path: string,
  projectArtFiles: AssetFile[],
  assets?: ProjectAssetRecord[],
): string | null {
  if (!path) return null;
  if (/^(https?:|data:|blob:)/i.test(path)) return path;
  const assetMatch = assets?.find((a) => a.file === path || a.image_url === path);
  if (assetMatch?.image_url) return assetMatch.image_url;
  if (projectId && path.startsWith("art/") && projectArtFiles.some((f) => f.path === path)) {
    return `/api/art/image?project=${encodeURIComponent(projectId)}&path=${encodeURIComponent(path)}`;
  }
  return null;
}

function colorWithAlpha(hex: unknown, transparency: number | undefined): string {
  const raw = typeof hex === "string" ? hex.trim().replace("#", "") : "141820";
  const validHex = /^[0-9a-fA-F]{6}$/.test(raw) ? raw : "141820";
  const r = parseInt(validHex.slice(0, 2), 16);
  const g = parseInt(validHex.slice(2, 4), 16);
  const b = parseInt(validHex.slice(4, 6), 16);
  const t = typeof transparency === "number" && Number.isFinite(transparency) ? transparency : 0;
  const alpha = Math.max(0, Math.min(1, 1 - t));
  return `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(3)})`;
}

function clampGuiNum(val: number, min: number, max: number): number {
  if (!Number.isFinite(val)) return min;
  return Math.max(min, Math.min(max, val));
}

export const GuiDesignerWorkbench = memo(function GuiDesignerWorkbench({
  projectId,
  place,
  assets,
  products,
  guiSyncSeq,
  onSavedScreenOrLuau,
}: {
  projectId?: string | null;
  place?: Place | null;
  assets?: ProjectAssetRecord[];
  products?: Product[];
  guiSyncSeq?: number;
  onSavedScreenOrLuau?: () => Promise<void> | void;
}) {
  const [doc, setDoc] = useState<ScreenDoc>(() => buildGuiPreset("rpg"));
  const [screens, setScreens] = useState<GuiScreenSummary[]>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string>("rpg-vitals");
  const [device, setDevice] = useState<GuiDeviceId>("desktop");
  const [showTopBarOverlay, setShowTopBarOverlay] = useState(true);
  const [showThumbstickOverlay, setShowThumbstickOverlay] = useState(true);
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("props");
  const [addClass, setAddClass] = useState<GuiClassName>("TextButton");
  const [saving, setSaving] = useState(false);
  const [compiling, setCompiling] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [serverWarnings, setServerWarnings] = useState<SafeAreaWarning[] | null>(null);
  const [compiledLuau, setCompiledLuau] = useState<string>("");
  const [compiledModelJson, setCompiledModelJson] = useState<string>("");
  const [compileDiagnostics, setCompileDiagnostics] = useState<GuiCompileDiagnostic[]>([]);
  const [compileOutputMode, setCompileOutputMode] = useState<"luau" | "model">("luau");
  const [projectArtFiles, setProjectArtFiles] = useState<AssetFile[]>([]);
  const [brokenImgUrls, setBrokenImgUrls] = useState<Set<string>>(() => new Set());
  const [assetQuery, setAssetQuery] = useState("");
  const [newSlugDraft, setNewSlugDraft] = useState("");
  const [slugError, setSlugError] = useState("");
  const [guiError, setGuiError] = useState("");
  const [showNewSlugRow, setShowNewSlugRow] = useState(false);
  const [uiKitLayout, setUiKitLayout] = useState<UiKitLayoutId>("skybridge-crest");
  const [uiKitSkin, setUiKitSkin] = useState<UiKitSkinId>("obsidian-prism");
  const [uiKitSheet, setUiKitSheet] = useState<UiKitSheetId>("chrome_sheet");
  const [uiKitBusy, setUiKitBusy] = useState(false);
  const [uiKitSliceCount, setUiKitSliceCount] = useState<number | null>(null);
  const [teardownRefs, setTeardownRefs] = useState<string[]>([]);
  const [selectedTeardownRefs, setSelectedTeardownRefs] = useState<string[]>([]);
  const [useTeardownReference, setUseTeardownReference] = useState(true);
  const [variantTheme, setVariantTheme] = useState("");
  const [uiKitVariantFeedback, setUiKitVariantFeedback] = useState<{
    variantMode?: string;
    referenceCount: number;
    referenceGuided?: boolean;
  } | null>(null);
  const useTeardownCheckboxId = useId();

  const stageWrapRef = useRef<HTMLDivElement>(null);
  const [stageWidth, setStageWidth] = useState(640);

  const dragRef = useRef<{
    kind: "move" | ResizeHandle;
    nodeId: string;
    startClientX: number;
    startClientY: number;
    origPos: UDim2Value;
    origSize: UDim2Value;
    origRect: { x: number; y: number; w: number; h: number };
    scale: number;
  } | null>(null);

  const deviceSpec = GUI_DEVICES[device] ?? GUI_DEVICES.desktop;

  useEffect(() => {
    const el = stageWrapRef.current;
    if (!el) return;
    const update = () => {
      const w = el.clientWidth;
      if (w > 80) setStageWidth(w);
    };
    update();
    if (typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver(update);
      ro.observe(el);
      return () => ro.disconnect();
    }
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  const refreshScreensAndArt = useCallback(async () => {
    try {
      setGuiError("");
      const res = await listGuiScreens(projectId);
      if (res.ok && Array.isArray(res.screens)) {
        setScreens(res.screens);
        if (res.screens.length > 0) {
          const firstSlug = res.screens[0].slug;
          const loaded = await loadGuiScreen(firstSlug, projectId);
          if (loaded.ok && loaded.doc) {
            const norm = normalizeScreenDoc(loaded.doc, firstSlug);
            setDoc(norm);
            setServerWarnings(Array.isArray(loaded.safe_area_warnings) ? loaded.safe_area_warnings : null);
            const firstChild = norm.root.children.find((c) => GUI_VISUAL_CLASSES.has(c.class_name));
            setSelectedNodeId(firstChild?.id ?? norm.root.id);
            setDirty(false);
          } else if (!loaded.ok) {
            setGuiError(loaded.error || `Could not load ${firstSlug}. Using starter layout.`);
          }
        }
      } else if (!res.ok) {
        setGuiError(res.error || "Could not load saved screens. Using starter layout.");
      }
      if (projectId) {
        const aRes = await call<{ files: AssetFile[] }>(projUrl(projectId, "/assets"));
        if (aRes.ok && Array.isArray(aRes.files)) {
          setProjectArtFiles(aRes.files);
        }
        const tdRes = await fetchProjectTeardown(projectId);
        if (tdRes.ok && tdRes.has_teardown !== false) {
          const rec = Array.isArray(tdRes.recommended_ui_reference_screenshots)
            ? tdRes.recommended_ui_reference_screenshots.filter(
                (p): p is string => typeof p === "string" && p.trim().length > 0,
              )
            : [];
          const fallbackShots = Array.isArray(tdRes.screenshots)
            ? tdRes.screenshots
                .map((s) => (s && typeof s.path === "string" ? s.path.trim() : ""))
                .filter(Boolean)
            : [];
          const refs = Array.from(new Set(rec.length > 0 ? rec : fallbackShots)).slice(0, 6);
          setTeardownRefs(refs);
          setSelectedTeardownRefs(refs);
        } else {
          setTeardownRefs([]);
          setSelectedTeardownRefs([]);
        }
      } else {
        setProjectArtFiles([]);
        setTeardownRefs([]);
        setSelectedTeardownRefs([]);
      }
    } catch {
      setGuiError("Failed to refresh UI screens. Using local starter layout.");
    }
  }, [projectId]);

  useEffect(() => {
    void refreshScreensAndArt();
  }, [refreshScreensAndArt, guiSyncSeq]);

  const liveWarnings = useMemo(() => analyzeSafeArea(doc), [doc]);
  const activeWarnings = serverWarnings && !dirty ? serverWarnings : liveWarnings;
  const deviceWarnings = useMemo(
    () => activeWarnings.filter((w) => w.device === device),
    [activeWarnings, device],
  );

  const rects = useMemo(
    () => computeNodeRects(doc, deviceSpec.width, deviceSpec.height),
    [doc, deviceSpec.height, deviceSpec.width],
  );

  const selectedNode = useMemo(
    () => findGuiNode(doc.root, selectedNodeId) ?? doc.root,
    [doc.root, selectedNodeId],
  );

  const selectedRect = useMemo(
    () => rects.find((r) => r.node.id === selectedNode.id) ?? null,
    [rects, selectedNode.id],
  );

  const scale = useMemo(() => {
    const availW = Math.max(160, stageWidth - 20);
    const maxH = 310;
    return Math.min(availW / deviceSpec.width, maxH / deviceSpec.height, 1);
  }, [deviceSpec.height, deviceSpec.width, stageWidth]);

  const canvasCssWidth = Math.round(deviceSpec.width * scale);
  const canvasCssHeight = Math.round(deviceSpec.height * scale);

  const updateSelectedNodeProps = useCallback(
    (patch: Partial<GuiNodeProps>) => {
      setDoc((prev) => ({
        ...prev,
        root: updateGuiNode(prev.root, selectedNode.id, (n) => ({
          ...n,
          props: { ...n.props, ...patch },
        })),
      }));
      setDirty(true);
    },
    [selectedNode.id],
  );

  const updateSelectedNodeName = useCallback(
    (name: string) => {
      setDoc((prev) => ({
        ...prev,
        root: updateGuiNode(prev.root, selectedNode.id, (n) => ({ ...n, name })),
      }));
      setDirty(true);
    },
    [selectedNode.id],
  );

  const handleLoadPreset = (preset: GuiPresetId) => {
    const next = buildGuiPreset(preset);
    setDoc(next);
    const firstVis = next.root.children.find((c) => GUI_VISUAL_CLASSES.has(c.class_name));
    setSelectedNodeId(firstVis?.id ?? next.root.id);
    setDirty(true);
    setServerWarnings(null);
    setGuiError("");
    toast.success(`Loaded ${preset.toUpperCase()} starter HUD`);
  };

  const handleSelectSavedScreen = async (slug: string) => {
    setGuiError("");
    const res = await loadGuiScreen(slug, projectId);
    if (res.ok && res.doc) {
      const norm = normalizeScreenDoc(res.doc, slug);
      setDoc(norm);
      const firstVis = norm.root.children.find((c) => GUI_VISUAL_CLASSES.has(c.class_name));
      setSelectedNodeId(firstVis?.id ?? norm.root.id);
      setServerWarnings(Array.isArray(res.safe_area_warnings) ? res.safe_area_warnings : null);
      setDirty(false);
    } else {
      const msg = res.error || `Could not open ${slug}`;
      setGuiError(msg);
      toast.error(msg);
    }
  };

  const handleCreateSlugScreen = () => {
    const raw = newSlugDraft.trim();
    if (!raw || !isValidGuiSlug(raw)) {
      setSlugError("Invalid screen slug: use 1 to 64 letters, numbers, hyphens, or underscores (start with letter or number).");
      return;
    }
    setSlugError("");
    setGuiError("");
    const next = buildGuiPreset("rpg");
    next.slug = raw;
    next.name =
      raw
        .split(/[-_]+/)
        .filter(Boolean)
        .map((s) => s.charAt(0).toUpperCase() + s.slice(1))
        .join("") || "CustomHud";
    setDoc(next);
    setSelectedNodeId(next.root.children[0]?.id ?? next.root.id);
    setDirty(true);
    setShowNewSlugRow(false);
    setNewSlugDraft("");
    toast.success(`Created screen draft ${raw}`);
  };

  const handleSave = async () => {
    if (!isValidGuiSlug(doc.slug)) {
      const msg = "Invalid screen slug: fix the screen slug before saving.";
      setGuiError(msg);
      toast.error(msg);
      return;
    }
    setSaving(true);
    setGuiError("");
    try {
      const slug = doc.slug.trim();
      const nextDoc = { ...doc, slug };
      const res = await saveGuiScreen(slug, nextDoc, projectId);
      if (res.ok) {
        const savedDoc = res.doc ? normalizeScreenDoc(res.doc, slug) : nextDoc;
        setDoc(savedDoc);
        setDirty(false);
        setServerWarnings(Array.isArray(res.safe_area_warnings) ? res.safe_area_warnings : analyzeSafeArea(savedDoc));
        setScreens((prev) => {
          const exists = prev.some((s) => s.slug === slug);
          const item: GuiScreenSummary = { slug, name: savedDoc.name, path: res.path || `ui/${slug}.screen.json` };
          return exists ? prev.map((s) => (s.slug === slug ? item : s)) : [...prev, item];
        });
        toast.success(`Saved ${slug}.screen.json`);
        await onSavedScreenOrLuau?.();
      } else {
        const msg = res.error || "Could not save screen";
        setGuiError(msg);
        toast.error(msg);
      }
    } finally {
      setSaving(false);
    }
  };

  const handleApplyUiKitLayout = (nextLayout: UiKitLayoutId = uiKitLayout, nextSkin: UiKitSkinId = uiKitSkin) => {
    const next = buildUiKitScreenDoc(nextLayout, nextSkin);
    setDoc(next);
    setSelectedNodeId(next.root.children[0]?.id ?? next.root.id);
    setDirty(true);
    setServerWarnings(null);
    setCompiledLuau("");
    setGuiError("");
    toast.success(`Applied ${nextLayout} (${nextSkin})`);
  };

  const handleGenerateUiKitSheet = async () => {
    setUiKitBusy(true);
    try {
      const url = projectId
        ? projUrl(projectId, "/ui-kit/generate")
        : "/api/studio/ui-kit/generate";
      const activeRefs = useTeardownReference
        ? (selectedTeardownRefs.length > 0 ? selectedTeardownRefs : teardownRefs)
        : [];
      const trimmedTheme = variantTheme.trim();
      const res = await call<{
        sliced_count?: number;
        screen_doc?: unknown;
        variant_mode?: string;
        reference_images?: string[];
        reference_guided?: boolean;
      }>(url, {
        layout: uiKitLayout,
        skin: uiKitSkin,
        sheet: uiKitSheet,
        use_teardown_reference: useTeardownReference,
        ...(activeRefs.length > 0 ? { reference_screenshots: activeRefs } : {}),
        ...(trimmedTheme ? { variant_theme: trimmedTheme } : {}),
      });
      if (!res.ok) {
        toast.error(res.error || "Could not generate UI Kit sheet.");
        return;
      }
      const count = typeof res.sliced_count === "number" ? res.sliced_count : 0;
      setUiKitSliceCount(count);
      const refImages = Array.isArray(res.reference_images) ? res.reference_images : [];
      if (res.variant_mode || refImages.length > 0 || res.reference_guided !== undefined) {
        setUiKitVariantFeedback({
          variantMode: res.variant_mode,
          referenceCount: refImages.length,
          referenceGuided: res.reference_guided,
        });
      }
      if (res.screen_doc) {
        const next = normalizeScreenDoc(res.screen_doc, `${uiKitLayout}-${uiKitSkin}`);
        setDoc(next);
        setSelectedNodeId(next.root.children[0]?.id ?? next.root.id);
        setDirty(true);
      }
      const refSuffix = refImages.length > 0 ? `, ${refImages.length} ref` : "";
      toast.success(`Sliced ${count} UI assets (${uiKitSkin}${refSuffix})`);
      await onSavedScreenOrLuau?.();
    } finally {
      setUiKitBusy(false);
    }
  };

  const handleCompileToLuau = async () => {
    setCompiling(true);
    try {
      const rawSlug = doc.slug.trim();
      const slug = isValidGuiSlug(rawSlug) ? rawSlug : sanitizeGuiSlug(rawSlug);
      const nextDoc = { ...doc, slug };
      const res = await compileGuiScreen(slug, nextDoc, projectId, { save_luau: true });
      if (res.ok && typeof res.luau_source === "string" && res.luau_source.length > 0) {
        setCompiledLuau(res.luau_source);
        setCompiledModelJson(res.model_json || "");
        setCompileDiagnostics(Array.isArray(res.diagnostics) ? res.diagnostics : []);
        if (Array.isArray(res.safe_area_warnings)) {
          setServerWarnings(res.safe_area_warnings);
        }
        setDirty(false);
        setInspectorTab("luau");
        setCompileOutputMode("luau");
        toast.success(`Compiled ${nextDoc.name} to strict Luau`);
        await onSavedScreenOrLuau?.();
      } else {
        const fallback = compileScreenDocToLuau(nextDoc);
        setCompiledLuau(fallback.luau_source);
        setCompiledModelJson(fallback.model_json);
        setCompileDiagnostics(fallback.diagnostics);
        setInspectorTab("luau");
        toast.info("Compiled locally in browser preview");
      }
    } finally {
      setCompiling(false);
    }
  };

  const handleAddNode = () => {
    const targetParent = GUI_MODIFIER_CLASSES.has(selectedNode.class_name) ? doc.root : selectedNode;
    const id = `node-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 5)}`;
    const newNode: GuiNode = {
      id,
      name: `${addClass}${targetParent.children.length + 1}`,
      class_name: addClass,
      props: JSON.parse(JSON.stringify(DEFAULT_ADD_CLASS_PROPS[addClass])) as GuiNodeProps,
      children: [],
    };
    setDoc((prev) => ({
      ...prev,
      root: insertGuiChild(prev.root, targetParent.id, newNode),
    }));
    setSelectedNodeId(id);
    setDirty(true);
  };

  const handleDuplicateNode = () => {
    if (selectedNode.id === doc.root.id) return;
    const findParent = (cur: GuiNode): GuiNode | null => {
      if (cur.children.some((c) => c.id === selectedNode.id)) return cur;
      for (const c of cur.children) {
        const p = findParent(c);
        if (p) return p;
      }
      return null;
    };
    const parent = findParent(doc.root) ?? doc.root;
    const clone = cloneGuiNodeWithNewIds(selectedNode, Date.now().toString(36));
    setDoc((prev) => ({
      ...prev,
      root: insertGuiChild(prev.root, parent.id, clone),
    }));
    setSelectedNodeId(clone.id);
    setDirty(true);
  };

  const handleDeleteNode = () => {
    if (selectedNode.id === doc.root.id) return;
    setDoc((prev) => ({
      ...prev,
      root: deleteGuiNode(prev.root, selectedNode.id),
    }));
    setSelectedNodeId(doc.root.id);
    setDirty(true);
  };

  const startDrag = (
    e: ReactPointerEvent,
    kind: "move" | ResizeHandle,
    rectItem: ComputedNodeRect,
  ) => {
    e.stopPropagation();
    if (rectItem.node.id === doc.root.id) {
      setSelectedNodeId(rectItem.node.id);
      return;
    }
    setSelectedNodeId(rectItem.node.id);
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    dragRef.current = {
      kind,
      nodeId: rectItem.node.id,
      startClientX: e.clientX,
      startClientY: e.clientY,
      origPos: { ...(rectItem.node.props.Position ?? udim2(0, 0, 0, 0)) },
      origSize: { ...(rectItem.node.props.Size ?? udim2(0, 120, 0, 48)) },
      origRect: { x: rectItem.x, y: rectItem.y, w: rectItem.w, h: rectItem.h },
      scale,
    };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    const rawDx = Math.round((e.clientX - d.startClientX) / Math.max(0.1, d.scale));
    const rawDy = Math.round((e.clientY - d.startClientY) / Math.max(0.1, d.scale));

    if (d.kind === "move") {
      const minXDelta = Math.min(0, -d.origRect.x);
      const maxXDelta = Math.max(0, deviceSpec.width - d.origRect.w - d.origRect.x);
      const minYDelta = Math.min(0, -d.origRect.y);
      const maxYDelta = Math.max(0, deviceSpec.height - d.origRect.h - d.origRect.y);
      const dx = Math.max(minXDelta, Math.min(maxXDelta, rawDx));
      const dy = Math.max(minYDelta, Math.min(maxYDelta, rawDy));
      setDoc((prev) => ({
        ...prev,
        root: updateGuiNode(prev.root, d.nodeId, (n) => ({
          ...n,
          props: {
            ...n.props,
            Position: {
              ...d.origPos,
              x_offset: Math.round(d.origPos.x_offset + dx),
              y_offset: Math.round(d.origPos.y_offset + dy),
            },
          },
        })),
      }));
      setDirty(true);
      return;
    }

    let nextWOffset = d.origSize.x_offset;
    let nextHOffset = d.origSize.y_offset;
    let nextXOffset = d.origPos.x_offset;
    let nextYOffset = d.origPos.y_offset;

    const maxWidthGrow = Math.max(0, deviceSpec.width - (d.origRect.x + d.origRect.w));
    const maxHeightGrow = Math.max(0, deviceSpec.height - (d.origRect.y + d.origRect.h));

    if (d.kind.includes("e")) {
      const clampedDx = Math.max(16 - d.origRect.w, Math.min(maxWidthGrow, rawDx));
      nextWOffset = d.origSize.x_offset + clampedDx;
    }
    if (d.kind.includes("s")) {
      const clampedDy = Math.max(16 - d.origRect.h, Math.min(maxHeightGrow, rawDy));
      nextHOffset = d.origSize.y_offset + clampedDy;
    }
    if (d.kind.includes("w")) {
      const minLeftShift = Math.min(0, -d.origRect.x);
      const clampedDx = Math.max(minLeftShift, Math.min(rawDx, d.origRect.w - 16));
      nextWOffset = d.origSize.x_offset - clampedDx;
      nextXOffset = d.origPos.x_offset + clampedDx;
    }
    if (d.kind.includes("n")) {
      const minTopShift = Math.min(0, -d.origRect.y);
      const clampedDy = Math.max(minTopShift, Math.min(rawDy, d.origRect.h - 16));
      nextHOffset = d.origSize.y_offset - clampedDy;
      nextYOffset = d.origPos.y_offset + clampedDy;
    }

    setDoc((prev) => ({
      ...prev,
      root: updateGuiNode(prev.root, d.nodeId, (n) => ({
        ...n,
        props: {
          ...n.props,
          Position: {
            ...d.origPos,
            x_offset: Math.round(nextXOffset),
            y_offset: Math.round(nextYOffset),
          },
          Size: {
            ...d.origSize,
            x_offset: Math.round(nextWOffset),
            y_offset: Math.round(nextHOffset),
          },
        },
      })),
    }));
    setDirty(true);
  };

  const nudgeSelectedBy = (dx: number, dy: number) => {
    if (selectedNode.id === doc.root.id || !selectedRect) return;
    const minDx = Math.min(0, -selectedRect.x);
    const maxDx = Math.max(0, deviceSpec.width - selectedRect.w - selectedRect.x);
    const minDy = Math.min(0, -selectedRect.y);
    const maxDy = Math.max(0, deviceSpec.height - selectedRect.h - selectedRect.y);
    const clampedDx = Math.max(minDx, Math.min(maxDx, dx));
    const clampedDy = Math.max(minDy, Math.min(maxDy, dy));
    const curPos = selectedNode.props.Position ?? udim2(0, 0, 0, 0);
    updateSelectedNodeProps({
      Position: {
        ...curPos,
        x_offset: Math.round(curPos.x_offset + clampedDx),
        y_offset: Math.round(curPos.y_offset + clampedDy),
      },
    });
  };

  const endDrag = () => {
    dragRef.current = null;
  };

  // Merge art/** files & ProjectAssets for the picker
  const availableAssets = useMemo(() => {
    const items: Array<{ label: string; path: string; assetId?: string | number | null }> = [];
    const seen = new Set<string>();
    for (const f of projectArtFiles) {
      if (!seen.has(f.path)) {
        seen.add(f.path);
        items.push({ label: f.path.split("/").pop() || f.path, path: f.path, assetId: f.locked?.assetId ?? null });
      }
    }
    for (const a of assets || []) {
      const p = a.file || a.image_url || `art/${a.title.toLowerCase().replace(/[^a-z0-9]+/g, "_")}.png`;
      if (!seen.has(p)) {
        seen.add(p);
        items.push({ label: a.title || p.split("/").pop() || p, path: p, assetId: null });
      }
    }
    for (const fallback of [
      { label: "Banner Frame (9-slice)", path: "art/store/banner_frame.png" },
      { label: "Potion Icon", path: "art/store/potion_icon.png" },
      { label: "Coin Icon", path: "art/store/coin_icon.png" },
    ]) {
      if (!seen.has(fallback.path)) {
        seen.add(fallback.path);
        items.push(fallback);
      }
    }
    const q = assetQuery.trim().toLowerCase();
    return q ? items.filter((i) => i.label.toLowerCase().includes(q) || i.path.toLowerCase().includes(q)) : items;
  }, [assetQuery, assets, projectArtFiles]);

  const availableProducts = useMemo<Product[]>(() => {
    if (products && products.length > 0) return products;
    return [
      { key: "skip_stage", kind: "developer_product", name: "Skip Stage", priceRobux: 25, id: 3736007337 },
      { key: "double_cash", kind: "game_pass", name: "2x Cash Boost", priceRobux: 99, id: 3736007339 },
      { key: "ten_roll", kind: "developer_product", name: "Roll x10 Bundle", priceRobux: 199, id: 3736007342 },
    ];
  }, [products]);

  const warningCountsByNode = useMemo(() => {
    const map = new Map<string, number>();
    for (const w of activeWarnings) {
      map.set(w.node_id, (map.get(w.node_id) ?? 0) + 1);
    }
    return map;
  }, [activeWarnings]);

  const renderTree = (node: GuiNode, depth = 0) => {
    const active = node.id === selectedNode.id;
    const warnCount = warningCountsByNode.get(node.id) ?? 0;
    return (
      <div key={node.id}>
        <button
          type="button"
          data-gui-tree-node={node.id}
          onClick={() => setSelectedNodeId(node.id)}
          style={{ paddingLeft: `${8 + depth * 12}px` }}
          className={`flex w-full items-center justify-between gap-1.5 rounded-md py-1 pr-2 text-left text-xs transition duration-(--dur-press) ease-(--ease-house) ${
            active
              ? "bg-blue-soft font-medium text-blue"
              : "text-ink hover:bg-board"
          }`}
        >
          <span className="flex min-w-0 items-center gap-1.5">
            <span className="truncate">{node.name}</span>
          </span>
          <span className="flex shrink-0 items-center gap-1">
            {warnCount > 0 && (
              <span className="rounded-chip bg-pill-warn-bg px-1 font-mono text-xs tabular-nums text-pill-warn-fg">
                {warnCount}
              </span>
            )}
            <span className="font-mono text-xs text-ink-3">{node.class_name}</span>
          </span>
        </button>
        {node.children.map((ch) => renderTree(ch, depth + 1))}
      </div>
    );
  };

  const renderUDim2Editor = (label: string, propKey: "Position" | "Size" | "CanvasSize" | "CellSize" | "CellPadding", val: UDim2Value) => (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between text-xs text-ink-2">
        <span>{label}</span>
        <span className="font-mono text-xs tabular-nums text-ink-3">
          {`{${val.x_scale}, ${Math.round(val.x_offset)}, ${val.y_scale}, ${Math.round(val.y_offset)}}`}
        </span>
      </div>
      <div className="grid grid-cols-4 gap-1">
        {(
          [
            ["x_scale", "XS", 0.05],
            ["x_offset", "XO", 2],
            ["y_scale", "YS", 0.05],
            ["y_offset", "YO", 2],
          ] as const
        ).map(([field, tag, step]) => (
          <label key={field} className="flex flex-col gap-0.5">
            <span className="font-mono text-xs text-ink-3">{tag}</span>
            <Input
              type="number"
              step={step}
              data-gui-prop={`${propKey}.${field}`}
              value={val[field]}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (!Number.isFinite(n)) return;
                const clamped =
                  field === "x_scale" || field === "y_scale"
                    ? clampGuiNum(n, -10, 10)
                    : clampGuiNum(n, -16384, 16384);
                updateSelectedNodeProps({
                  [propKey]: { ...val, [field]: clamped },
                });
              }}
              className="h-7 px-1.5 font-mono text-xs tabular-nums"
            />
          </label>
        ))}
      </div>
    </div>
  );

  const isVisual = GUI_VISUAL_CLASSES.has(selectedNode.class_name);
  const posVal = selectedNode.props.Position ?? udim2(0, 0, 0, 0);
  const sizeVal = selectedNode.props.Size ?? udim2(0, 120, 0, 48);
  const anchorVal = selectedNode.props.AnchorPoint ?? [0, 0];

  return (
    <div
      data-gui-designer
      className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto overflow-x-hidden pr-0.5"
    >
      {/* Top Designer Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-board/60 px-2.5 py-2">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          <Pill className="max-w-full min-w-0 bg-surface px-2 py-0.5 text-xs text-ink">
            <Layers className="size-3.5 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
            <span className="truncate font-medium">{doc.name}</span>
            <span className="truncate font-mono text-ink-3">{doc.slug}.screen.json</span>
          </Pill>

          {screens.length > 0 && (
            <select
              aria-label="Select saved screen"
              value={screens.some((s) => s.slug === doc.slug) ? doc.slug : ""}
              onChange={(e) => {
                if (e.target.value) void handleSelectSavedScreen(e.target.value);
              }}
              className="h-8 max-w-full rounded-lg border border-hairline bg-surface px-2 text-xs text-ink"
            >
              <option value="" disabled>
                Screens ({screens.length})
              </option>
              {screens.map((s) => (
                <option key={s.slug} value={s.slug}>
                  {s.name} ({s.slug})
                </option>
              ))}
            </select>
          )}

          <div className="flex flex-wrap items-center gap-1" aria-label="Starter presets">
            {GUI_PRESET_META.map((p) => (
              <Button
                key={p.id}
                variant="outline"
                size="xs"
                data-gui-preset={p.id}
                onClick={() => handleLoadPreset(p.id)}
                title={p.summary}
              >
                {p.label}
              </Button>
            ))}
          </div>

          <Button
            variant="ghost"
            size="xs"
            data-gui-new
            onClick={() => {
              setSlugError("");
              setShowNewSlugRow((v) => !v);
            }}
            aria-label="New ScreenGui"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>New</span>
          </Button>
        </div>

        <div className="flex max-w-full flex-wrap items-center gap-1.5">
          <SegmentedControl aria-label="Device viewport" size="sm" className="max-w-full flex-wrap">
            {(["desktop", "tablet", "mobile"] as GuiDeviceId[]).map((d) => {
              const Icon = d === "desktop" ? Laptop : d === "tablet" ? Tablet : Smartphone;
              return (
                <SegmentedControlItem
                  key={d}
                  data-gui-device={d}
                  data-checked={device === d ? "" : undefined}
                  aria-pressed={device === d}
                  onClick={() => setDevice(d)}
                  className="gap-1"
                >
                  <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
                  <span>{GUI_DEVICES[d].shortLabel}</span>
                </SegmentedControlItem>
              );
            })}
          </SegmentedControl>

          <Button
            variant={dirty ? "secondary" : "outline"}
            size="xs"
            data-gui-save
            loading={saving}
            onClick={() => void handleSave()}
          >
            <Save className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>{dirty ? "Save*" : "Save"}</span>
          </Button>

          <Button
            variant="default"
            size="xs"
            data-gui-compile
            loading={compiling}
            onClick={() => void handleCompileToLuau()}
            className="border-ink-button bg-ink-button text-ink-button-fg"
          >
            <Code2 className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Compile to Luau</span>
          </Button>
        </div>
      </div>

      {showNewSlugRow && (
        <div className="flex flex-col gap-1.5 rounded-card border border-hairline bg-surface px-2.5 py-2">
          <div className="flex flex-wrap items-center gap-2">
            <Input
              aria-label="New screen slug"
              data-gui-slug-input
              aria-invalid={Boolean(slugError)}
              value={newSlugDraft}
              onChange={(e) => {
                setNewSlugDraft(e.target.value);
                if (slugError) setSlugError("");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreateSlugScreen();
              }}
              placeholder="shop-modal"
              className="h-8 max-w-[200px] font-mono text-xs"
            />
            <Button variant="outline" size="xs" data-gui-create-slug onClick={handleCreateSlugScreen}>
              Create screen
            </Button>
            {slugError && newSlugDraft.trim() && (
              <Button
                variant="ghost"
                size="xs"
                onClick={() => {
                  const clean = sanitizeGuiSlug(newSlugDraft);
                  setNewSlugDraft(clean);
                  setSlugError("");
                }}
              >
                Use {sanitizeGuiSlug(newSlugDraft)}
              </Button>
            )}
            <Button
              variant="ghost"
              size="xs"
              onClick={() => {
                setShowNewSlugRow(false);
                setSlugError("");
              }}
            >
              Cancel
            </Button>
          </div>
          {slugError && (
            <p role="alert" data-gui-slug-error className="text-xs text-pill-pro-fg">
              {slugError}
            </p>
          )}
        </div>
      )}

      {guiError && (
        <div
          role="alert"
          data-gui-error
          className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-pill-pro-bg/60 bg-pill-pro-bg/40 px-2.5 py-2 text-xs text-pill-pro-fg"
        >
          <span>{guiError}</span>
          <div className="flex items-center gap-1.5">
            <Button variant="outline" size="xs" onClick={() => void refreshScreensAndArt()} className="bg-surface">
              Retry
            </Button>
            <Button variant="ghost" size="xs" onClick={() => handleLoadPreset("rpg")}>
              Reset preset
            </Button>
          </div>
        </div>
      )}

      {/* UI Kit Generator Strip: Mixable Layout Topology + Visual Skin + Asset Sheet */}
      <div
        data-ui-kit-strip
        className="flex flex-col gap-1.5 rounded-card border border-hairline bg-surface px-2.5 py-1.5"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <span className="flex items-center gap-1 text-xs font-medium text-ink">
              <Sparkles className="size-3.5 text-blue" strokeWidth={1.5} aria-hidden />
              <span>UI Kit</span>
            </span>
            <select
              aria-label="UI Kit layout topology"
              data-ui-kit-layout
              value={uiKitLayout}
              onChange={(e) => {
                const next = e.target.value as UiKitLayoutId;
                setUiKitLayout(next);
                handleApplyUiKitLayout(next, uiKitSkin);
              }}
              className="h-8 max-w-full rounded-lg border border-hairline bg-board px-2 text-xs text-ink max-md:min-h-11"
            >
              {UI_KIT_LAYOUT_META.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.label}
                </option>
              ))}
            </select>
            <select
              aria-label="UI Kit visual skin"
              data-ui-kit-skin
              value={uiKitSkin}
              onChange={(e) => {
                const next = e.target.value as UiKitSkinId;
                setUiKitSkin(next);
                handleApplyUiKitLayout(uiKitLayout, next);
              }}
              className="h-8 max-w-full rounded-lg border border-hairline bg-board px-2 text-xs text-ink max-md:min-h-11"
            >
              {UI_KIT_SKIN_META.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.label}
                </option>
              ))}
            </select>
            <select
              aria-label="UI Kit asset sheet"
              data-ui-kit-sheet
              value={uiKitSheet}
              onChange={(e) => setUiKitSheet(e.target.value as UiKitSheetId)}
              className="h-8 max-w-full rounded-lg border border-hairline bg-board px-2 text-xs text-ink max-md:min-h-11"
            >
              {UI_KIT_SHEET_META.map((sh) => (
                <option key={sh.id} value={sh.id}>
                  {sh.label}
                </option>
              ))}
            </select>
          </div>
          <div className="flex flex-wrap items-center gap-1.5">
            {uiKitSliceCount !== null && (
              <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-active-fg">
                {uiKitSliceCount} slices
              </Pill>
            )}
            <Button
              variant="outline"
              size="xs"
              data-ui-kit-apply
              onClick={() => handleApplyUiKitLayout()}
              className="max-md:min-h-11"
            >
              <span>Apply Layout</span>
            </Button>
            <Button
              variant="secondary"
              size="xs"
              data-ui-kit-generate
              loading={uiKitBusy}
              onClick={() => void handleGenerateUiKitSheet()}
              className="max-md:min-h-11"
            >
              <ImageIcon className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Generate UI Kit Sheet</span>
            </Button>
          </div>
        </div>

        {/* Reference Variant Strip */}
        <div
          data-ui-kit-reference-strip
          className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline pt-1.5"
        >
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            {(selectedTeardownRefs.length > 0 || teardownRefs.length > 0) && (
              <Badge
                data-ui-kit-reference-badge
                className="border-transparent bg-tag-design-bg px-2 py-0.5 text-xs text-tag-design-fg"
              >
                Reference Variant
              </Badge>
            )}
            <label
              htmlFor={useTeardownCheckboxId}
              className="inline-flex min-h-8 cursor-pointer items-center gap-1.5 text-xs text-ink max-md:min-h-11"
            >
              <input
                id={useTeardownCheckboxId}
                type="checkbox"
                data-ui-kit-use-teardown
                checked={useTeardownReference}
                onChange={(e) => setUseTeardownReference(e.target.checked)}
                className="size-3.5 accent-blue"
              />
              <span>Use teardown screenshots (default)</span>
            </label>
            {useTeardownReference && teardownRefs.length > 0 && (
              <div className="flex flex-wrap items-center gap-1" role="group" aria-label="Teardown reference screenshots">
                {teardownRefs.slice(0, 4).map((refPath) => {
                  const active = selectedTeardownRefs.includes(refPath);
                  const shortName = refPath.split("/").pop() || refPath;
                  return (
                    <button
                      key={refPath}
                      type="button"
                      aria-pressed={active}
                      onClick={() =>
                        setSelectedTeardownRefs((prev) =>
                          prev.includes(refPath) ? prev.filter((p) => p !== refPath) : [...prev, refPath],
                        )
                      }
                      className={`inline-flex h-7 items-center gap-1 rounded-sm border px-1.5 font-mono text-xs transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) max-md:min-h-11 ${
                        active
                          ? "border-blue/60 bg-pill-active-bg text-pill-active-fg"
                          : "border-hairline bg-board text-ink-2 hover:text-ink"
                      }`}
                      title={refPath}
                    >
                      {projectId && (
                        <img
                          src={teardownScreenshotPreviewUrl(projectId, refPath)}
                          alt=""
                          className="size-4 rounded-xs object-cover"
                        />
                      )}
                      <span className="max-w-[96px] truncate">{shortName}</span>
                    </button>
                  );
                })}
              </div>
            )}
            <Input
              aria-label="Variant twist theme"
              data-ui-kit-variant-theme
              value={variantTheme}
              onChange={(e) => setVariantTheme(e.target.value)}
              placeholder="Variant twist (e.g. Meme Arcade Pop)"
              className="h-8 min-w-[200px] max-w-[260px] bg-board text-xs max-md:min-h-11"
            />
          </div>
          {uiKitVariantFeedback && (
            <Pill
              data-ui-kit-variant-feedback
              className="bg-tag-sky-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-sky-fg"
            >
              {uiKitVariantFeedback.variantMode ||
                (uiKitVariantFeedback.referenceGuided ? "reference-variant" : "skin-default")}
              {" · "}
              {uiKitVariantFeedback.referenceCount} ref{uiKitVariantFeedback.referenceCount === 1 ? "" : "s"}
            </Pill>
          )}
        </div>
      </div>

      {/* Safe-Area Overlay Strip */}
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-card border border-hairline bg-surface px-2.5 py-1.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs font-medium text-ink-2">Safe-area overlays:</span>
          <Button
            variant={showTopBarOverlay ? "secondary" : "outline"}
            size="xs"
            data-gui-overlay="topbar"
            aria-pressed={showTopBarOverlay}
            onClick={() => setShowTopBarOverlay((v) => !v)}
          >
            {showTopBarOverlay ? <Eye className="size-3.5" strokeWidth={1.5} /> : <EyeOff className="size-3.5" strokeWidth={1.5} />}
            <span>58px TopBar</span>
          </Button>
          <Button
            variant={showThumbstickOverlay ? "secondary" : "outline"}
            size="xs"
            data-gui-overlay="thumbstick"
            aria-pressed={showThumbstickOverlay}
            onClick={() => setShowThumbstickOverlay((v) => !v)}
          >
            <Gamepad2 className="size-3.5" strokeWidth={1.5} aria-hidden />
            <span>Thumbstick / Jump</span>
          </Button>
          <Button
            variant={doc.ignore_gui_inset ? "secondary" : "outline"}
            size="xs"
            data-gui-ignore-inset
            aria-pressed={doc.ignore_gui_inset}
            onClick={() => {
              setDoc((prev) => ({ ...prev, ignore_gui_inset: !prev.ignore_gui_inset }));
              setDirty(true);
            }}
          >
            <span>IgnoreGuiInset: {doc.ignore_gui_inset ? "On" : "Off"}</span>
          </Button>
        </div>

        <div className="flex items-center gap-2 font-mono text-xs tabular-nums text-ink-2">
          <span>
            {deviceSpec.width}x{deviceSpec.height}
          </span>
          <span>({Math.round(scale * 100)}%)</span>
          {activeWarnings.length === 0 ? (
            <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
              <CheckCircle2 className="size-3" strokeWidth={1.5} aria-hidden />
              <span>0 warnings</span>
            </Pill>
          ) : (
            <Pill className="bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg">
              <AlertTriangle className="size-3" strokeWidth={1.5} aria-hidden />
              <span>{activeWarnings.length} warnings</span>
            </Pill>
          )}
        </div>
      </div>

      {/* Main Workspace: Full-width Interactive Canvas above 2-column Tree + Inspector */}
      <div className="flex flex-col gap-2.5">
        {/* Interactive ScreenGui Canvas */}
        <div
          ref={stageWrapRef}
          className="flex w-full shrink-0 flex-col items-center justify-center gap-2 overflow-hidden rounded-card border border-hairline bg-board p-2.5"
        >
          <div
            data-gui-canvas
            onPointerMove={onPointerMove}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
            style={{
              width: `${canvasCssWidth}px`,
              height: `${canvasCssHeight}px`,
              backgroundColor: "rgb(15, 19, 26)",
            }}
            className="relative select-none overflow-hidden rounded-lg border border-hairline shadow-[var(--elev-card)]"
          >
            {/* Subtle Roblox Viewport Grid */}
            <div
              className="pointer-events-none absolute inset-0 opacity-20"
              style={{
                backgroundImage:
                  "linear-gradient(to right, rgba(255,255,255,0.08) 1px, transparent 1px), linear-gradient(to bottom, rgba(255,255,255,0.08) 1px, transparent 1px)",
                backgroundSize: `${Math.max(12, Math.round(48 * scale))}px ${Math.max(12, Math.round(48 * scale))}px`,
              }}
            />

            {/* Rendered GuiNodes */}
            {rects.map((item) => {
              const { node } = item;
              const isRoot = node.id === doc.root.id;
              const isSelected = node.id === selectedNode.id;
              const corner = node.children.find((c) => c.class_name === "UICorner");
              const stroke = node.children.find((c) => c.class_name === "UIStroke");
              const radiusPx = corner?.props.CornerRadius
                ? (Math.min(item.w, item.h) * corner.props.CornerRadius[0] + corner.props.CornerRadius[1]) * scale
                : 0;
              const strokeWidthPx = stroke?.props.Thickness ? Math.max(1, stroke.props.Thickness * scale) : 0;
              const strokeColor = stroke?.props.Color || "#334155";
              const bg = colorWithAlpha(node.props.BackgroundColor3, node.props.BackgroundTransparency ?? (isRoot ? 1 : 0));
              const isButton = node.class_name === "TextButton" || node.class_name === "ImageButton";
              const isImage = node.class_name === "ImageLabel" || node.class_name === "ImageButton";
              const rawImgUrl = isImage
                ? resolveAssetPreviewUrl(projectId, node.props.Image || node.props.AssetPath || "", projectArtFiles, assets)
                : null;
              const imgUrl = rawImgUrl && !brokenImgUrls.has(rawImgUrl) ? rawImgUrl : null;
              const hasSlice = isImage && node.props.ScaleType === "Slice" && node.props.SliceCenter;
              const nodeWarn = deviceWarnings.some((w) => w.node_id === node.id);

              return (
                <div
                  key={node.id}
                  role="button"
                  tabIndex={isRoot ? -1 : 0}
                  aria-label={`${node.name} (${node.class_name})`}
                  aria-pressed={isSelected}
                  data-gui-canvas-node={node.id}
                  onPointerDown={(e) => startDrag(e, "move", item)}
                  onKeyDown={(e) => {
                    if (isRoot) return;
                    const step = e.shiftKey ? 10 : 2;
                    if (e.key === "ArrowLeft") {
                      e.preventDefault();
                      nudgeSelectedBy(-step, 0);
                    } else if (e.key === "ArrowRight") {
                      e.preventDefault();
                      nudgeSelectedBy(step, 0);
                    } else if (e.key === "ArrowUp") {
                      e.preventDefault();
                      nudgeSelectedBy(0, -step);
                    } else if (e.key === "ArrowDown") {
                      e.preventDefault();
                      nudgeSelectedBy(0, step);
                    } else if (e.key === "Delete" || e.key === "Backspace") {
                      e.preventDefault();
                      handleDeleteNode();
                    }
                  }}
                  style={{
                    left: `${item.x * scale}px`,
                    top: `${item.y * scale}px`,
                    width: `${Math.max(6, item.w * scale)}px`,
                    height: `${Math.max(6, item.h * scale)}px`,
                    zIndex: (node.props.ZIndex ?? 1) + (isSelected ? 40 : 0),
                    backgroundColor: bg,
                    borderRadius: `${radiusPx}px`,
                    border: strokeWidthPx > 0 ? `${strokeWidthPx}px solid ${strokeColor}` : undefined,
                    cursor: isRoot ? "default" : "move",
                  }}
                  className={`absolute flex items-center justify-center overflow-visible ${
                    isSelected
                      ? "outline-2 outline-focus"
                      : nodeWarn
                        ? "outline-[1.5px] outline-dashed outline-dot-amber"
                        : isRoot
                          ? ""
                          : "outline outline-white/10"
                  }`}
                >
                  {imgUrl ? (
                    <img
                      src={imgUrl}
                      alt={node.name}
                      draggable={false}
                      onError={() => setBrokenImgUrls((prev) => new Set(prev).add(imgUrl))}
                      className="pointer-events-none h-full w-full object-cover"
                      style={{ borderRadius: `${radiusPx}px` }}
                    />
                  ) : isImage && node.props.ScaleType === "Slice" && node.props.ZIndex === 1 ? (
                    <div
                      className="pointer-events-none h-full w-full border border-white/15"
                      style={{
                        borderRadius: `${radiusPx}px`,
                        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.12)",
                      }}
                    />
                  ) : isImage ? (
                    <div className="pointer-events-none flex flex-col items-center justify-center gap-0.5 px-1 text-center text-xs text-white/75">
                      <ImageIcon className="size-3.5" strokeWidth={1.5} aria-hidden />
                      <span className="max-w-full truncate font-mono text-[12px]">
                        {(node.props.Image || node.props.AssetPath || "Image").split("/").pop()}
                      </span>
                    </div>
                  ) : null}

                  {(node.class_name === "TextLabel" || node.class_name === "TextButton") && (
                    <span
                      className={`pointer-events-none truncate px-1 text-center font-medium ${node.props.TextColor3 ? "" : "text-white"}`}
                      style={{
                        ...(node.props.TextColor3 ? { color: node.props.TextColor3 } : {}),
                        fontSize: `${Math.max(10, Math.round((node.props.TextSize ?? 14) * scale))}px`,
                      }}
                    >
                      {node.props.Text ?? node.name}
                    </span>
                  )}

                  {node.class_name === "ScrollingFrame" && (
                    <span className="pointer-events-none absolute right-1 top-1 text-[12px] text-white/50">
                      Scroll
                    </span>
                  )}

                  {/* 9-slice SliceCenter guide */}
                  {isSelected && hasSlice && (
                    <div
                      className="pointer-events-none absolute inset-1.5 border border-dashed border-tag-research-fg/80"
                      title={`SliceCenter: ${node.props.SliceCenter?.join(", ")}`}
                    />
                  )}

                  {/* Interactive 8 resize handles */}
                  {isSelected && !isRoot && (
                    <>
                      {(Object.keys(HANDLE_POSITIONS) as ResizeHandle[]).map((h) => (
                        <div
                          key={h}
                          data-gui-handle={h}
                          onPointerDown={(e) => startDrag(e, h, item)}
                          style={{ cursor: HANDLE_CURSORS[h] }}
                          className={`absolute z-50 size-2.5 rounded-xs border border-ink bg-surface ${HANDLE_POSITIONS[h]}`}
                        />
                      ))}
                      {isButton && node.props.ProductId && (
                        <span className="pointer-events-none absolute -top-5 left-0 rounded-chip bg-blue px-1.5 py-0 text-[12px] tabular-nums text-blue-fg">
                          R$ {String(node.props.ProductId)}
                        </span>
                      )}
                    </>
                  )}
                </div>
              );
            })}

            {/* 58px TopBar CoreGui Inset Overlay */}
            {showTopBarOverlay && (
              <div
                data-gui-topbar-overlay
                style={{ height: `${Math.max(16, Math.round(58 * scale))}px` }}
                className="pointer-events-none absolute inset-x-0 top-0 z-50 flex items-center justify-between border-b border-dashed border-dot-amber bg-pill-warn-bg/20 px-2"
              >
                <span className="rounded-chip bg-pill-warn-bg px-1.5 py-0.5 text-[12px] tabular-nums text-pill-warn-fg">
                  CoreGui TopBar 58px
                </span>
                <span className="rounded-chip bg-surface/90 px-1.5 py-0.5 font-mono text-[12px] text-ink">
                  {doc.ignore_gui_inset ? "IgnoreGuiInset=true" : "Inset Active"}
                </span>
              </div>
            )}

            {/* Mobile Thumbstick & Jump Safe-Area Overlays */}
            {showThumbstickOverlay && (
              <>
                <div
                  data-gui-thumbstick-overlay
                  style={{
                    width: `${Math.round(200 * scale)}px`,
                    height: `${Math.round(170 * scale)}px`,
                  }}
                  className="pointer-events-none absolute bottom-0 left-0 z-50 flex items-center justify-center rounded-tr-xl border-r border-t border-dashed border-blue bg-blue-soft/25"
                >
                  <span className="rounded-chip bg-tag-sky-bg px-1.5 py-0.5 text-[12px] text-tag-sky-fg">
                    Thumbstick
                  </span>
                </div>
                <div
                  data-gui-jump-overlay
                  style={{
                    width: `${Math.round(180 * scale)}px`,
                    height: `${Math.round(170 * scale)}px`,
                  }}
                  className="pointer-events-none absolute bottom-0 right-0 z-50 flex items-center justify-center rounded-tl-xl border-l border-t border-dashed border-blue bg-blue-soft/25"
                >
                  <span className="rounded-chip bg-tag-sky-bg px-1.5 py-0.5 text-[12px] text-tag-sky-fg">
                    Jump
                  </span>
                </div>
              </>
            )}
          </div>

          {selectedRect && (
            <div className="flex flex-wrap items-center gap-3 text-xs tabular-nums text-ink-2">
              <span>Node: <code className="font-mono">{selectedNode.name}</code></span>
              <span>X {Math.round(selectedRect.x)}</span>
              <span>Y {Math.round(selectedRect.y)}</span>
              <span>W {Math.round(selectedRect.w)}</span>
              <span>H {Math.round(selectedRect.h)}</span>
            </div>
          )}
        </div>

        {/* Lower 2-Column Split: Hierarchy + Safe-Area Warnings | Inspector / Bind / Luau */}
        <div className="grid grid-cols-1 gap-2.5 md:grid-cols-[220px_minmax(0,1fr)]">
          {/* Left: Hierarchy Tree + Safe-Area Warnings */}
          <div className="flex min-h-0 flex-col gap-2.5">
            <div className="flex min-h-[170px] flex-col gap-2 rounded-card border border-hairline bg-surface p-2.5" data-gui-tree>
              <div className="flex items-center justify-between gap-1">
                <span className="text-xs font-medium text-ink">Hierarchy</span>
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    onClick={handleDuplicateNode}
                    disabled={selectedNode.id === doc.root.id}
                    aria-label="Duplicate selected node"
                    title="Duplicate"
                    className="size-6"
                  >
                    <CopyPlus className="size-3.5" strokeWidth={1.5} />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    onClick={handleDeleteNode}
                    disabled={selectedNode.id === doc.root.id}
                    aria-label="Delete selected node"
                    title="Delete"
                    className="size-6 text-destructive-foreground"
                  >
                    <Trash2 className="size-3.5" strokeWidth={1.5} />
                  </Button>
                </div>
              </div>

              <div className="flex items-center gap-1">
                <select
                  aria-label="New node class"
                  value={addClass}
                  onChange={(e) => setAddClass(e.target.value as GuiClassName)}
                  className="h-7 min-w-0 flex-1 rounded-md border border-hairline bg-board px-1.5 font-mono text-xs text-ink"
                >
                  {GUI_CLASS_ALLOWLIST.map((cls) => (
                    <option key={cls} value={cls}>
                      {cls}
                    </option>
                  ))}
                </select>
                <Button variant="outline" size="xs" data-gui-add-node onClick={handleAddNode} className="h-7 px-2">
                  <Plus className="size-3" strokeWidth={1.5} />
                  <span>Add</span>
                </Button>
              </div>

              <div className="min-h-0 flex-1 overflow-y-auto pr-0.5">{renderTree(doc.root)}</div>
            </div>

            <div
              data-gui-warnings
              className="flex max-h-[220px] flex-col gap-1.5 overflow-y-auto rounded-card border border-hairline bg-surface p-2.5"
            >
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-1 text-xs font-medium text-ink">
                  <ShieldAlert className="size-3.5 text-ink-2" strokeWidth={1.5} aria-hidden />
                  <span>Safe-area check</span>
                </span>
                <Pill
                  className={
                    activeWarnings.length > 0
                      ? "bg-pill-warn-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-warn-fg"
                      : "bg-pill-active-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-active-fg"
                  }
                >
                  {activeWarnings.length}
                </Pill>
              </div>

              {activeWarnings.length === 0 ? (
                <p className="text-xs text-ink-2">
                  All controls clear the 58px TopBar, mobile thumbstick zones, and tap target minimums across Desktop, Tablet, and Mobile.
                </p>
              ) : (
                <div className="flex flex-col gap-1">
                  {activeWarnings.map((w, idx) => (
                    <button
                      key={`${w.device}-${w.node_id}-${w.kind}-${idx}`}
                      type="button"
                      data-gui-warning-item={w.kind}
                      onClick={() => {
                        setDevice(w.device);
                        setSelectedNodeId(w.node_id);
                      }}
                      className="flex flex-col gap-0.5 rounded-md border border-hairline bg-board/60 p-1.5 text-left transition duration-(--dur-press) ease-(--ease-house) hover:bg-board"
                    >
                      <div className="flex items-center justify-between gap-1">
                        <span className="truncate font-mono text-xs font-medium text-ink">{w.node_name}</span>
                        <span className="rounded-chip bg-pill-warn-bg px-1.5 py-0 font-mono text-xs text-pill-warn-fg">
                          {w.kind}
                        </span>
                      </div>
                      <span className="text-xs text-ink-2">{w.message}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Right: Property Inspector, Asset/Product Picker, and Compiled Luau Output */}
          <div
            data-gui-inspector
            className="flex min-h-[260px] min-w-0 flex-col gap-2 rounded-card border border-hairline bg-surface p-2.5"
          >
          <SegmentedControl aria-label="Inspector tab" size="sm">
            <SegmentedControlItem
              data-gui-inspector-tab="props"
              data-checked={inspectorTab === "props" ? "" : undefined}
              aria-pressed={inspectorTab === "props"}
              onClick={() => setInspectorTab("props")}
            >
              Properties
            </SegmentedControlItem>
            <SegmentedControlItem
              data-gui-inspector-tab="bind"
              data-checked={inspectorTab === "bind" ? "" : undefined}
              aria-pressed={inspectorTab === "bind"}
              onClick={() => setInspectorTab("bind")}
            >
              Bind Art/ID
            </SegmentedControlItem>
            <SegmentedControlItem
              data-gui-inspector-tab="luau"
              data-checked={inspectorTab === "luau" ? "" : undefined}
              aria-pressed={inspectorTab === "luau"}
              onClick={() => setInspectorTab("luau")}
            >
              Luau
            </SegmentedControlItem>
          </SegmentedControl>

          {inspectorTab === "props" && (
            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto pr-0.5">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium text-ink">{selectedNode.name}</span>
                <Badge className="font-mono text-xs">
                  {selectedNode.class_name}
                </Badge>
              </div>

              <label className="flex flex-col gap-1">
                <span className="text-xs text-ink-2">Name</span>
                <Input
                  data-gui-prop="name"
                  value={selectedNode.name}
                  onChange={(e) => updateSelectedNodeName(e.target.value)}
                  className="h-8 text-xs"
                />
              </label>

              {isVisual && (
                <>
                  {renderUDim2Editor("Position (UDim2)", "Position", posVal)}
                  {renderUDim2Editor("Size (UDim2)", "Size", sizeVal)}

                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">Anchor X</span>
                      <Input
                        type="number"
                        step={0.1}
                        min={0}
                        max={1}
                        data-gui-prop="AnchorPoint.x"
                        value={anchorVal[0]}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) updateSelectedNodeProps({ AnchorPoint: [clampGuiNum(n, 0, 1), anchorVal[1]] });
                        }}
                        className="h-7 font-mono text-xs tabular-nums"
                      />
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">Anchor Y</span>
                      <Input
                        type="number"
                        step={0.1}
                        min={0}
                        max={1}
                        data-gui-prop="AnchorPoint.y"
                        value={anchorVal[1]}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) updateSelectedNodeProps({ AnchorPoint: [anchorVal[0], clampGuiNum(n, 0, 1)] });
                        }}
                        className="h-7 font-mono text-xs tabular-nums"
                      />
                    </label>
                  </div>

                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">BackgroundColor3</span>
                      <Input
                        data-gui-prop="BackgroundColor3"
                        value={selectedNode.props.BackgroundColor3 ?? "#141820"}
                        onChange={(e) => updateSelectedNodeProps({ BackgroundColor3: e.target.value })}
                        className="h-7 font-mono text-xs"
                      />
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">BgTransparency</span>
                      <Input
                        type="number"
                        step={0.05}
                        min={0}
                        max={1}
                        data-gui-prop="BackgroundTransparency"
                        value={selectedNode.props.BackgroundTransparency ?? 0}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) updateSelectedNodeProps({ BackgroundTransparency: clampGuiNum(n, 0, 1) });
                        }}
                        className="h-7 font-mono text-xs tabular-nums"
                      />
                    </label>
                  </div>
                </>
              )}

              {(selectedNode.class_name === "TextLabel" || selectedNode.class_name === "TextButton") && (
                <>
                  <label className="flex flex-col gap-1">
                    <span className="text-xs text-ink-2">Text</span>
                    <Input
                      data-gui-prop="Text"
                      value={selectedNode.props.Text ?? ""}
                      onChange={(e) => updateSelectedNodeProps({ Text: e.target.value })}
                      className="h-8 text-xs"
                    />
                  </label>
                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">TextSize</span>
                      <Input
                        type="number"
                        min={8}
                        max={96}
                        data-gui-prop="TextSize"
                        value={selectedNode.props.TextSize ?? 14}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) updateSelectedNodeProps({ TextSize: clampGuiNum(n, 8, 96) });
                        }}
                        className="h-7 font-mono text-xs tabular-nums"
                      />
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">TextColor3</span>
                      <Input
                        data-gui-prop="TextColor3"
                        value={selectedNode.props.TextColor3 ?? "#F3F4F6"}
                        onChange={(e) => updateSelectedNodeProps({ TextColor3: e.target.value })}
                        className="h-7 font-mono text-xs"
                      />
                    </label>
                  </div>
                </>
              )}

              {(selectedNode.class_name === "ImageLabel" || selectedNode.class_name === "ImageButton") && (
                <>
                  <label className="flex flex-col gap-1">
                    <span className="text-xs text-ink-2">Image Asset</span>
                    <Input
                      data-gui-prop="Image"
                      value={selectedNode.props.Image ?? ""}
                      onChange={(e) => updateSelectedNodeProps({ Image: e.target.value })}
                      className="h-8 font-mono text-xs"
                    />
                  </label>
                  <div className="grid grid-cols-2 gap-1.5">
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">ScaleType</span>
                      <select
                        data-gui-prop="ScaleType"
                        value={selectedNode.props.ScaleType ?? "Stretch"}
                        onChange={(e) =>
                          updateSelectedNodeProps({
                            ScaleType: e.target.value as GuiNodeProps["ScaleType"],
                            ...(e.target.value === "Slice" && !selectedNode.props.SliceCenter
                              ? { SliceCenter: [24, 24, 232, 232] }
                              : {}),
                          })
                        }
                        className="h-7 rounded-md border border-hairline bg-surface px-1.5 text-xs text-ink"
                      >
                        <option value="Stretch">Stretch</option>
                        <option value="Slice">Slice (9-slice)</option>
                        <option value="Fit">Fit</option>
                        <option value="Crop">Crop</option>
                        <option value="Tile">Tile</option>
                      </select>
                    </label>
                    <label className="flex flex-col gap-0.5">
                      <span className="text-xs text-ink-2">SliceCenter</span>
                      <Input
                        data-gui-prop="SliceCenter"
                        value={(selectedNode.props.SliceCenter ?? [24, 24, 232, 232]).join(",")}
                        onChange={(e) => {
                          const nums = e.target.value
                            .split(",")
                            .map((s) => Number(s.trim()))
                            .filter((n) => Number.isFinite(n));
                          if (nums.length === 4) {
                            updateSelectedNodeProps({
                              SliceCenter: [nums[0], nums[1], nums[2], nums[3]],
                            });
                          }
                        }}
                        className="h-7 font-mono text-xs tabular-nums"
                      />
                    </label>
                  </div>
                </>
              )}

              {(selectedNode.class_name === "TextButton" || selectedNode.class_name === "ImageButton") && (
                <div className="grid grid-cols-2 gap-1.5">
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">CallbackName</span>
                    <Input
                      data-gui-prop="CallbackName"
                      value={selectedNode.props.CallbackName ?? ""}
                      onChange={(e) => updateSelectedNodeProps({ CallbackName: e.target.value })}
                      className="h-7 font-mono text-xs"
                    />
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">ProductId</span>
                    <Input
                      data-gui-prop="ProductId"
                      value={selectedNode.props.ProductId ? String(selectedNode.props.ProductId) : ""}
                      onChange={(e) =>
                        updateSelectedNodeProps({
                          ProductId: e.target.value.trim() ? Number(e.target.value.trim()) || e.target.value.trim() : null,
                        })
                      }
                      placeholder="Optional"
                      className="h-7 font-mono text-xs tabular-nums"
                    />
                  </label>
                </div>
              )}

              {selectedNode.class_name === "UICorner" && (
                <label className="flex flex-col gap-1">
                  <span className="text-xs text-ink-2">CornerRadius Offset (px)</span>
                  <Input
                    type="number"
                    min={0}
                    max={120}
                    data-gui-prop="CornerRadius"
                    value={selectedNode.props.CornerRadius?.[1] ?? 10}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) updateSelectedNodeProps({ CornerRadius: [0, n] });
                    }}
                    className="h-7 font-mono text-xs tabular-nums"
                  />
                </label>
              )}

              {selectedNode.class_name === "UIStroke" && (
                <div className="grid grid-cols-3 gap-1.5">
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">Thickness</span>
                    <Input
                      type="number"
                      step={0.5}
                      data-gui-prop="Thickness"
                      value={selectedNode.props.Thickness ?? 1.5}
                      onChange={(e) => {
                        const n = Number(e.target.value);
                        if (Number.isFinite(n)) updateSelectedNodeProps({ Thickness: n });
                      }}
                      className="h-7 font-mono text-xs tabular-nums"
                    />
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">Color</span>
                    <Input
                      data-gui-prop="Color"
                      value={selectedNode.props.Color ?? "#334155"}
                      onChange={(e) => updateSelectedNodeProps({ Color: e.target.value })}
                      className="h-7 font-mono text-xs"
                    />
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">Transparency</span>
                    <Input
                      type="number"
                      step={0.05}
                      min={0}
                      max={1}
                      data-gui-prop="Transparency"
                      value={selectedNode.props.Transparency ?? 0.2}
                      onChange={(e) => {
                        const n = Number(e.target.value);
                        if (Number.isFinite(n)) updateSelectedNodeProps({ Transparency: n });
                      }}
                      className="h-7 font-mono text-xs tabular-nums"
                    />
                  </label>
                </div>
              )}

              {selectedNode.class_name === "UIListLayout" && (
                <div className="grid grid-cols-2 gap-1.5">
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">FillDirection</span>
                    <select
                      data-gui-prop="FillDirection"
                      value={selectedNode.props.FillDirection ?? "Vertical"}
                      onChange={(e) =>
                        updateSelectedNodeProps({ FillDirection: e.target.value as "Horizontal" | "Vertical" })
                      }
                      className="h-7 rounded-md border border-hairline bg-surface px-1.5 text-xs text-ink"
                    >
                      <option value="Horizontal">Horizontal</option>
                      <option value="Vertical">Vertical</option>
                    </select>
                  </label>
                  <label className="flex flex-col gap-0.5">
                    <span className="text-xs text-ink-2">Padding (px)</span>
                    <Input
                      type="number"
                      data-gui-prop="Padding"
                      value={selectedNode.props.Padding?.[1] ?? 8}
                      onChange={(e) => {
                        const n = Number(e.target.value);
                        if (Number.isFinite(n)) updateSelectedNodeProps({ Padding: [0, n] });
                      }}
                      className="h-7 font-mono text-xs tabular-nums"
                    />
                  </label>
                </div>
              )}
            </div>
          )}

          {inspectorTab === "bind" && (
            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto pr-0.5" data-gui-picker>
              <Input
                value={assetQuery}
                onChange={(e) => setAssetQuery(e.target.value)}
                placeholder="Filter art/** or products..."
                className="h-8 text-xs"
              />

              <div className="flex flex-col gap-1">
                <span className="text-xs font-medium text-ink">Project art/** assets</span>
                {availableAssets.map((a) => (
                  <button
                    key={a.path}
                    type="button"
                    data-gui-bind-asset={a.path}
                    onClick={() => {
                      const imgVal = a.assetId ? `rbxassetid://${a.assetId}` : a.path;
                      updateSelectedNodeProps({ Image: imgVal, AssetPath: a.path });
                      toast.success(`Bound ${a.label} to ${selectedNode.name}`);
                    }}
                    className="flex items-center justify-between gap-2 rounded-md border border-hairline bg-board/60 px-2 py-1.5 text-left text-xs transition duration-(--dur-press) ease-(--ease-house) hover:bg-board"
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <ImageIcon className="size-3.5 shrink-0 text-icon" strokeWidth={1.5} />
                      <span className="truncate font-medium text-ink">{a.label}</span>
                    </span>
                    <span className="shrink-0 font-mono text-xs text-ink-3">
                      {a.assetId ? `#${a.assetId}` : "Bind"}
                    </span>
                  </button>
                ))}
              </div>

              <div className="flex flex-col gap-1">
                <span className="text-xs font-medium text-ink">ProductIds & GamePasses</span>
                {availableProducts.map((pr) => (
                  <button
                    key={pr.key}
                    type="button"
                    data-gui-bind-product={String(pr.id ?? pr.key)}
                    onClick={() => {
                      const prodId = pr.id ?? 3736007337;
                      updateSelectedNodeProps({
                        ProductId: prodId,
                        CallbackName: selectedNode.props.CallbackName || `OnBuy_${pr.key}`,
                      });
                      toast.success(`Bound ProductId ${prodId} to ${selectedNode.name}`);
                    }}
                    className="flex items-center justify-between gap-2 rounded-md border border-hairline bg-board/60 px-2 py-1.5 text-left text-xs transition duration-(--dur-press) ease-(--ease-house) hover:bg-board"
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <Package className="size-3.5 shrink-0 text-blue" strokeWidth={1.5} />
                      <span className="truncate font-medium text-ink">{pr.name}</span>
                    </span>
                    <span className="shrink-0 text-xs tabular-nums text-ink-2">
                      {pr.priceRobux} R$
                    </span>
                  </button>
                ))}
              </div>

              {place && Object.keys(place.scripts || {}).length > 0 && (
                <p className="text-xs text-ink-3">
                  Linked place scripts: <span className="font-mono tabular-nums">{Object.keys(place.scripts).length}</span>
                </p>
              )}
            </div>
          )}

          {inspectorTab === "luau" && (
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden" data-gui-luau-output>
              <div className="flex flex-wrap items-center justify-between gap-1.5">
                <div className="flex items-center gap-1">
                  <Button
                    variant={compileOutputMode === "luau" ? "secondary" : "ghost"}
                    size="xs"
                    onClick={() => setCompileOutputMode("luau")}
                  >
                    <FileCode2 className="size-3.5" strokeWidth={1.5} />
                    <span>Luau</span>
                  </Button>
                  <Button
                    variant={compileOutputMode === "model" ? "secondary" : "ghost"}
                    size="xs"
                    onClick={() => setCompileOutputMode("model")}
                    disabled={!compiledModelJson}
                  >
                    <span>.model.json</span>
                  </Button>
                </div>

                <Button
                  variant="outline"
                  size="xs"
                  disabled={!compiledLuau}
                  onClick={() => {
                    const txt = compileOutputMode === "model" ? compiledModelJson : compiledLuau;
                    void navigator.clipboard?.writeText(txt);
                    toast.success("Copied compiled output");
                  }}
                >
                  <Copy className="size-3.5" strokeWidth={1.5} />
                  <span>Copy</span>
                </Button>
              </div>

              {compiledLuau ? (
                <>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                      <Sparkles className="size-3" strokeWidth={1.5} aria-hidden />
                      <code className="font-mono">--!strict</code>
                    </Pill>
                    <span className="text-xs tabular-nums text-ink-2">
                      {compileDiagnostics.filter((d) => d.severity === "error").length} errors,{" "}
                      {compileDiagnostics.filter((d) => d.severity === "warning").length} warnings
                    </span>
                  </div>

                  {compileDiagnostics.length > 0 && (
                    <div className="flex max-h-24 flex-col gap-1 overflow-y-auto rounded-md border border-hairline bg-board/60 p-1.5">
                      {compileDiagnostics.map((d, i) => (
                        <div key={i} className="text-xs text-ink-2">
                          <code className="font-mono">[{d.code}] L{d.line}:</code> {d.message}
                        </div>
                      ))}
                    </div>
                  )}

                  <pre className="min-h-0 flex-1 overflow-auto rounded-lg border border-hairline bg-board p-2 font-mono text-xs leading-relaxed text-ink">
                    <code>{compileOutputMode === "model" ? compiledModelJson : compiledLuau}</code>
                  </pre>
                </>
              ) : (
                <div className="flex flex-1 flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-hairline bg-board/40 p-4 text-center">
                  <Code2 className="size-5 text-icon" strokeWidth={1.5} aria-hidden />
                  <p className="text-xs text-ink-2">
                    Click Compile to Luau to generate a strict ModuleScript with create(parent, callbacks) and Rojo model output.
                  </p>
                  <Button variant="outline" size="xs" onClick={() => void handleCompileToLuau()}>
                    Compile now
                  </Button>
                </div>
              )}
            </div>
          )}
          </div>
        </div>
      </div>
    </div>
  );
});
