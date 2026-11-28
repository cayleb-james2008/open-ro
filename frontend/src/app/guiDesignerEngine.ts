// Shared ScreenDoc v1 schema, presets, multi-device safe-area analyzer, and deterministic --!strict Luau compiler.

export const GUI_CLASS_ALLOWLIST = [
  "Frame",
  "TextLabel",
  "TextButton",
  "ImageLabel",
  "ImageButton",
  "ScrollingFrame",
  "UICorner",
  "UIStroke",
  "UIListLayout",
  "UIGridLayout",
  "UIAspectRatioConstraint",
  "UIPadding",
] as const;

export type GuiClassName = (typeof GUI_CLASS_ALLOWLIST)[number];

export const GUI_VISUAL_CLASSES = new Set<GuiClassName>([
  "Frame",
  "TextLabel",
  "TextButton",
  "ImageLabel",
  "ImageButton",
  "ScrollingFrame",
]);

export const GUI_MODIFIER_CLASSES = new Set<GuiClassName>([
  "UICorner",
  "UIStroke",
  "UIListLayout",
  "UIGridLayout",
  "UIAspectRatioConstraint",
  "UIPadding",
]);

export type UDim2Value = {
  x_scale: number;
  x_offset: number;
  y_scale: number;
  y_offset: number;
};

export type GuiNodeProps = {
  Position?: UDim2Value;
  Size?: UDim2Value;
  AnchorPoint?: [number, number];
  BackgroundColor3?: string;
  BackgroundTransparency?: number;
  BorderSizePixel?: number;
  Visible?: boolean;
  ZIndex?: number;
  ClipsDescendants?: boolean;
  LayoutOrder?: number;
  // Text
  Text?: string;
  TextColor3?: string;
  TextSize?: number;
  TextScaled?: boolean;
  Font?: string;
  TextXAlignment?: "Left" | "Center" | "Right";
  TextYAlignment?: "Top" | "Center" | "Bottom";
  // Image + 9-slice
  Image?: string;
  ImageColor3?: string;
  ImageTransparency?: number;
  ScaleType?: "Stretch" | "Slice" | "Fit" | "Crop" | "Tile";
  SliceCenter?: [number, number, number, number];
  SliceScale?: number;
  // ScrollingFrame
  CanvasSize?: UDim2Value;
  ScrollBarThickness?: number;
  // UICorner
  CornerRadius?: [number, number];
  // UIStroke
  Color?: string;
  Thickness?: number;
  Transparency?: number;
  ApplyStrokeMode?: "Contextual" | "Border";
  // UIListLayout
  FillDirection?: "Horizontal" | "Vertical";
  Padding?: [number, number];
  HorizontalAlignment?: "Left" | "Center" | "Right";
  VerticalAlignment?: "Top" | "Center" | "Bottom";
  SortOrder?: "LayoutOrder" | "Name";
  // UIGridLayout
  CellSize?: UDim2Value;
  CellPadding?: UDim2Value;
  // UIAspectRatioConstraint
  AspectRatio?: number;
  // UIPadding
  PaddingTop?: [number, number];
  PaddingBottom?: [number, number];
  PaddingLeft?: [number, number];
  PaddingRight?: [number, number];
  // OpenRO asset & product bindings
  AssetPath?: string;
  ProductId?: number | string | null;
  CallbackName?: string;
};

export type GuiNode = {
  id: string;
  name: string;
  class_name: GuiClassName;
  props: GuiNodeProps;
  children: GuiNode[];
};

export type ScreenDocTheme = {
  accent: string;
  surface: string;
  text: string;
};

export type ScreenDoc = {
  version: 1;
  slug: string;
  name: string;
  ignore_gui_inset: boolean;
  reset_on_spawn: boolean;
  modal_blur?: boolean;
  theme: ScreenDocTheme;
  root: GuiNode;
};

export type GuiDeviceId = "desktop" | "tablet" | "mobile";

export type GuiDeviceSpec = {
  id: GuiDeviceId;
  label: string;
  shortLabel: string;
  width: number;
  height: number;
};

export const GUI_DEVICES: Record<GuiDeviceId, GuiDeviceSpec> = {
  desktop: { id: "desktop", label: "Desktop 16:9", shortLabel: "Desktop", width: 1920, height: 1080 },
  tablet: { id: "tablet", label: "Tablet 4:3", shortLabel: "Tablet", width: 1024, height: 768 },
  mobile: { id: "mobile", label: "Mobile Landscape", shortLabel: "Mobile", width: 844, height: 390 },
};

export type SafeAreaWarningKind =
  | "topbar_overlap"
  | "thumbstick_overlap"
  | "offscreen_bounds"
  | "tiny_tap_target"
  | "low_contrast";

export type SafeAreaWarning = {
  device: GuiDeviceId;
  node_id: string;
  node_name: string;
  kind: SafeAreaWarningKind;
  severity: "error" | "warning";
  message: string;
  rect: { x: number; y: number; w: number; h: number };
};

export type GuiPresetId = "rpg" | "obby" | "tycoon" | "gacha";
export type UiKitLayoutId = "skybridge-crest" | "perimeter-command" | "workshop-ribbon" | "compact-mobile-split";
export type UiKitSkinId = "obsidian-prism" | "skybridge-gilded" | "sakura-lacquer" | "cyber-forge" | "clean-monochrome" | "meme-pop-arcade";
export type UiKitSheetId = "chrome_sheet" | "icons_sheet" | "rarity_frames_sheet";

export type ComputedNodeRect = {
  node: GuiNode;
  parentId: string | null;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
};

export function isValidGuiSlug(slug: string): boolean {
  return /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/.test(slug);
}

export function sanitizeGuiSlug(raw: string): string {
  const cleaned = raw
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^[-_]+|[-_]+$/g, "");
  return cleaned.slice(0, 64) || "main-hud";
}

export function udim2(xScale: number, xOffset: number, yScale: number, yOffset: number): UDim2Value {
  return { x_scale: xScale, x_offset: xOffset, y_scale: yScale, y_offset: yOffset };
}

function asNumber(val: unknown, fallback: number): number {
  return typeof val === "number" && Number.isFinite(val) ? val : fallback;
}

function clampFinite(val: unknown, min: number, max: number, fallback: number): number {
  if (typeof val !== "number" || !Number.isFinite(val)) return fallback;
  return Math.max(min, Math.min(max, val));
}

function safeHexColor(val: unknown, fallback: string): string {
  if (typeof val !== "string") return fallback;
  const trimmed = val.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) return trimmed;
  if (/^[0-9a-fA-F]{6}$/.test(trimmed)) return `#${trimmed}`;
  return fallback;
}

const UDIM_SCALE_MIN = -10;
const UDIM_SCALE_MAX = 10;
const UDIM_OFFSET_MIN = -16384;
const UDIM_OFFSET_MAX = 16384;

function clampUDim2Value(u: UDim2Value): UDim2Value {
  return {
    x_scale: clampFinite(u.x_scale, UDIM_SCALE_MIN, UDIM_SCALE_MAX, 0),
    x_offset: clampFinite(u.x_offset, UDIM_OFFSET_MIN, UDIM_OFFSET_MAX, 0),
    y_scale: clampFinite(u.y_scale, UDIM_SCALE_MIN, UDIM_SCALE_MAX, 0),
    y_offset: clampFinite(u.y_offset, UDIM_OFFSET_MIN, UDIM_OFFSET_MAX, 0),
  };
}

export function normalizeUDim2(val: unknown, fallback: UDim2Value): UDim2Value {
  if (!val) return clampUDim2Value(fallback);
  if (typeof val === "object" && !Array.isArray(val) && "UDim2" in (val as Record<string, unknown>)) {
    return normalizeUDim2((val as Record<string, unknown>).UDim2, fallback);
  }
  if (Array.isArray(val) && val.length === 2 && Array.isArray(val[0]) && Array.isArray(val[1])) {
    return clampUDim2Value({
      x_scale: asNumber(val[0][0], fallback.x_scale),
      x_offset: asNumber(val[0][1], fallback.x_offset),
      y_scale: asNumber(val[1][0], fallback.y_scale),
      y_offset: asNumber(val[1][1], fallback.y_offset),
    });
  }
  if (Array.isArray(val) && val.length >= 4) {
    return clampUDim2Value({
      x_scale: asNumber(val[0], fallback.x_scale),
      x_offset: asNumber(val[1], fallback.x_offset),
      y_scale: asNumber(val[2], fallback.y_scale),
      y_offset: asNumber(val[3], fallback.y_offset),
    });
  }
  if (typeof val === "object") {
    const obj = val as Record<string, unknown>;
    if ("x_scale" in obj || "x_offset" in obj || "y_scale" in obj || "y_offset" in obj) {
      return clampUDim2Value({
        x_scale: asNumber(obj.x_scale, fallback.x_scale),
        x_offset: asNumber(obj.x_offset, fallback.x_offset),
        y_scale: asNumber(obj.y_scale, fallback.y_scale),
        y_offset: asNumber(obj.y_offset, fallback.y_offset),
      });
    }
    const xObj = (obj.X ?? obj.x) as Record<string, unknown> | undefined;
    const yObj = (obj.Y ?? obj.y) as Record<string, unknown> | undefined;
    if (xObj || yObj) {
      return clampUDim2Value({
        x_scale: asNumber(xObj?.Scale ?? xObj?.scale, fallback.x_scale),
        x_offset: asNumber(xObj?.Offset ?? xObj?.offset, fallback.x_offset),
        y_scale: asNumber(yObj?.Scale ?? yObj?.scale, fallback.y_scale),
        y_offset: asNumber(yObj?.Offset ?? yObj?.offset, fallback.y_offset),
      });
    }
  }
  return clampUDim2Value(fallback);
}

function normalizeUDimPair(val: unknown): [number, number] | undefined {
  if (Array.isArray(val) && val.length >= 2) {
    return [
      clampFinite(val[0], UDIM_SCALE_MIN, UDIM_SCALE_MAX, 0),
      clampFinite(val[1], UDIM_OFFSET_MIN, UDIM_OFFSET_MAX, 0),
    ];
  }
  if (typeof val === "number" && Number.isFinite(val)) {
    return [0, clampFinite(val, UDIM_OFFSET_MIN, UDIM_OFFSET_MAX, 0)];
  }
  if (val && typeof val === "object") {
    const o = val as Record<string, unknown>;
    return [
      clampFinite(o.scale ?? o.Scale ?? o.s, UDIM_SCALE_MIN, UDIM_SCALE_MAX, 0),
      clampFinite(o.offset ?? o.Offset ?? o.o, UDIM_OFFSET_MIN, UDIM_OFFSET_MAX, 0),
    ];
  }
  return undefined;
}

const MAX_GUI_NODE_DEPTH = 24;

function normalizeGuiNode(raw: unknown, idxPath: string, depth = 0): GuiNode {
  const obj = (raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {}) as Record<string, unknown>;
  const rawClass = String(obj.class_name ?? obj.ClassName ?? "Frame") as GuiClassName;
  const className: GuiClassName = (GUI_CLASS_ALLOWLIST as readonly string[]).includes(rawClass)
    ? rawClass
    : "Frame";
  const rawProps = (
    obj.props && typeof obj.props === "object" && !Array.isArray(obj.props)
      ? obj.props
      : obj.properties && typeof obj.properties === "object" && !Array.isArray(obj.properties)
        ? obj.properties
        : {}
  ) as Record<string, unknown>;
  const props: GuiNodeProps = { ...rawProps } as GuiNodeProps;
  if (rawProps.Position !== undefined) props.Position = normalizeUDim2(rawProps.Position, udim2(0, 0, 0, 0));
  if (rawProps.Size !== undefined) props.Size = normalizeUDim2(rawProps.Size, udim2(0, 120, 0, 48));
  if (rawProps.CanvasSize !== undefined) props.CanvasSize = normalizeUDim2(rawProps.CanvasSize, udim2(0, 0, 1.5, 0));
  if (rawProps.CellSize !== undefined) props.CellSize = normalizeUDim2(rawProps.CellSize, udim2(0, 88, 0, 88));
  if (rawProps.CellPadding !== undefined) props.CellPadding = normalizeUDim2(rawProps.CellPadding, udim2(0, 8, 0, 8));
  if (rawProps.AnchorPoint !== undefined) {
    if (Array.isArray(rawProps.AnchorPoint) && rawProps.AnchorPoint.length >= 2) {
      props.AnchorPoint = [
        clampFinite(rawProps.AnchorPoint[0], 0, 1, 0),
        clampFinite(rawProps.AnchorPoint[1], 0, 1, 0),
      ];
    } else {
      delete props.AnchorPoint;
    }
  }
  if (rawProps.BackgroundColor3 !== undefined) {
    props.BackgroundColor3 = safeHexColor(rawProps.BackgroundColor3, "#141820");
  }
  if (rawProps.BackgroundTransparency !== undefined) {
    props.BackgroundTransparency = clampFinite(rawProps.BackgroundTransparency, 0, 1, 0);
  }
  if (rawProps.TextColor3 !== undefined) {
    props.TextColor3 = safeHexColor(rawProps.TextColor3, "#F3F4F6");
  }
  if (rawProps.TextSize !== undefined) {
    props.TextSize = clampFinite(rawProps.TextSize, 8, 96, 14);
  }
  if (rawProps.Text !== undefined) {
    props.Text = typeof rawProps.Text === "string" ? rawProps.Text : String(rawProps.Text ?? "");
  }
  if (rawProps.Font !== undefined) {
    props.Font = typeof rawProps.Font === "string" ? rawProps.Font : "GothamMedium";
  }
  if (rawProps.Image !== undefined) {
    props.Image = typeof rawProps.Image === "string" ? rawProps.Image : "";
  }
  if (rawProps.AssetPath !== undefined) {
    props.AssetPath = typeof rawProps.AssetPath === "string" ? rawProps.AssetPath : "";
  }
  if (rawProps.ImageTransparency !== undefined) {
    props.ImageTransparency = clampFinite(rawProps.ImageTransparency, 0, 1, 0);
  }
  if (rawProps.SliceCenter !== undefined) {
    if (Array.isArray(rawProps.SliceCenter) && rawProps.SliceCenter.length >= 4) {
      props.SliceCenter = [
        asNumber(rawProps.SliceCenter[0], 24),
        asNumber(rawProps.SliceCenter[1], 24),
        asNumber(rawProps.SliceCenter[2], 232),
        asNumber(rawProps.SliceCenter[3], 232),
      ];
    } else {
      delete props.SliceCenter;
    }
  }
  if (rawProps.Color !== undefined) {
    props.Color = safeHexColor(rawProps.Color, "#334155");
  }
  if (rawProps.Thickness !== undefined) {
    props.Thickness = clampFinite(rawProps.Thickness, 0, 32, 1.5);
  }
  if (rawProps.Transparency !== undefined) {
    props.Transparency = clampFinite(rawProps.Transparency, 0, 1, 0.2);
  }
  if (rawProps.ZIndex !== undefined) {
    props.ZIndex = clampFinite(rawProps.ZIndex, -100, 1000, 1);
  }
  if (rawProps.CornerRadius !== undefined) {
    const pair = normalizeUDimPair(rawProps.CornerRadius);
    if (pair) props.CornerRadius = [clampFinite(pair[0], 0, 1, 0), clampFinite(pair[1], 0, 240, 10)];
    else delete props.CornerRadius;
  }
  if (rawProps.Padding !== undefined) {
    const pair = normalizeUDimPair(rawProps.Padding);
    if (pair) props.Padding = pair;
    else delete props.Padding;
  }
  if (props.CallbackName === undefined && typeof rawProps.Callback === "string") {
    props.CallbackName = rawProps.Callback;
  }
  if (props.ProductId === undefined && rawProps.ProductKey !== undefined) {
    props.ProductId = String(rawProps.ProductKey);
  }

  const rawChildren = depth < MAX_GUI_NODE_DEPTH && Array.isArray(obj.children) ? obj.children : [];
  return {
    id: typeof obj.id === "string" && obj.id.trim() ? obj.id.trim() : `node-${idxPath}`,
    name: typeof obj.name === "string" && obj.name.trim() ? obj.name.trim() : className,
    class_name: className,
    props,
    children: rawChildren.map((c, i) => normalizeGuiNode(c, `${idxPath}-${i}`, depth + 1)),
  };
}

export function normalizeScreenDoc(raw: unknown, fallbackSlug = "main-hud"): ScreenDoc {
  let parsed = raw;
  if (typeof raw === "string") {
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = null;
    }
  }
  const obj = (parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {}) as Record<string, unknown>;
  const docObj = (obj.doc && typeof obj.doc === "object" && !Array.isArray(obj.doc) ? obj.doc : obj) as Record<string, unknown>;
  const slugRaw = typeof docObj.slug === "string" && isValidGuiSlug(docObj.slug) ? docObj.slug : sanitizeGuiSlug(fallbackSlug);
  const themeRaw = (docObj.theme && typeof docObj.theme === "object" ? docObj.theme : {}) as Record<string, unknown>;
  const hasRoot = Boolean(docObj.root && typeof docObj.root === "object" && !Array.isArray(docObj.root));
  return {
    version: 1,
    slug: slugRaw,
    name: typeof docObj.name === "string" && docObj.name.trim() ? docObj.name.trim() : "MainHud",
    ignore_gui_inset: docObj.ignore_gui_inset !== false,
    reset_on_spawn: docObj.reset_on_spawn === true,
    modal_blur: docObj.modal_blur === true,
    theme: {
      accent: safeHexColor(themeRaw.accent, "#1562FC"),
      surface: safeHexColor(themeRaw.surface, "#141820"),
      text: safeHexColor(themeRaw.text, "#F3F4F6"),
    },
    root: hasRoot
      ? normalizeGuiNode(docObj.root, "root")
      : buildGuiPreset("rpg").root,
  };
}

function makeModifier(id: string, name: string, class_name: GuiClassName, props: GuiNodeProps): GuiNode {
  return { id, name, class_name, props, children: [] };
}

export function buildGuiPreset(preset: GuiPresetId): ScreenDoc {
  if (preset === "obby") {
    return {
      version: 1,
      slug: "obby-hud",
      name: "ObbyHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#1562FC", surface: "#131720", text: "#F4F6FA" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: {
          Position: udim2(0, 0, 0, 0),
          Size: udim2(1, 0, 1, 0),
          BackgroundTransparency: 1,
          BorderSizePixel: 0,
          Visible: true,
          ZIndex: 1,
        },
        children: [
          {
            id: "obby-stage-pill",
            name: "StageBar",
            class_name: "Frame",
            props: {
              Position: udim2(0.5, 0, 0, 66),
              Size: udim2(0, 260, 0, 48),
              AnchorPoint: [0.5, 0],
              BackgroundColor3: "#131720",
              BackgroundTransparency: 0.12,
              BorderSizePixel: 0,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("obby-stage-corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("obby-stage-stroke", "UIStroke", "UIStroke", { Color: "#2C3548", Thickness: 1.5, Transparency: 0.2 }),
              {
                id: "obby-stage-label",
                name: "StageText",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 12, 0, 4),
                  Size: udim2(1, -24, 1, -8),
                  BackgroundTransparency: 1,
                  Text: "Stage 4 / 20",
                  TextColor3: "#F4F6FA",
                  TextSize: 18,
                  Font: "GothamBold",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
            ],
          },
          {
            id: "obby-timer",
            name: "TimerCard",
            class_name: "TextLabel",
            props: {
              Position: udim2(1, -24, 0, 66),
              Size: udim2(0, 136, 0, 44),
              AnchorPoint: [1, 0],
              BackgroundColor3: "#131720",
              BackgroundTransparency: 0.15,
              Text: "01:24.8",
              TextColor3: "#4DC296",
              TextSize: 18,
              Font: "Code",
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("obby-timer-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
            ],
          },
          {
            id: "obby-actions",
            name: "BottomActions",
            class_name: "Frame",
            props: {
              Position: udim2(0.5, 0, 1, -24),
              Size: udim2(0, 320, 0, 52),
              AnchorPoint: [0.5, 1],
              BackgroundTransparency: 1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("obby-actions-list", "UIListLayout", "UIListLayout", {
                FillDirection: "Horizontal",
                Padding: [0, 10],
                HorizontalAlignment: "Center",
                VerticalAlignment: "Center",
              }),
              {
                id: "obby-skip-btn",
                name: "SkipStageButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(0, 0, 0, 0),
                  Size: udim2(0, 164, 0, 48),
                  BackgroundColor3: "#1562FC",
                  BackgroundTransparency: 0,
                  Text: "Skip Stage (25 R$)",
                  TextColor3: "#FFFFFF",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "OnSkipStage",
                  ProductId: 3736007337,
                  Visible: true,
                  ZIndex: 3,
                },
                children: [
                  makeModifier("obby-skip-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
                ],
              },
              {
                id: "obby-respawn-btn",
                name: "RespawnButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(0, 174, 0, 0),
                  Size: udim2(0, 128, 0, 48),
                  BackgroundColor3: "#232938",
                  BackgroundTransparency: 0,
                  Text: "Respawn",
                  TextColor3: "#F4F6FA",
                  TextSize: 15,
                  Font: "GothamMedium",
                  CallbackName: "OnRespawn",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [
                  makeModifier("obby-respawn-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
                ],
              },
            ],
          },
        ],
      },
    };
  }

  if (preset === "tycoon") {
    return {
      version: 1,
      slug: "tycoon-hud",
      name: "TycoonHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#24946E", surface: "#12181B", text: "#F2F7F4" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: {
          Position: udim2(0, 0, 0, 0),
          Size: udim2(1, 0, 1, 0),
          BackgroundTransparency: 1,
          Visible: true,
          ZIndex: 1,
        },
        children: [
          {
            id: "tycoon-cash",
            name: "CashCounter",
            class_name: "Frame",
            props: {
              Position: udim2(0, 24, 0, 66),
              Size: udim2(0, 220, 0, 56),
              BackgroundColor3: "#12181B",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("tycoon-cash-corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("tycoon-cash-stroke", "UIStroke", "UIStroke", { Color: "#24946E", Thickness: 1.5, Transparency: 0.2 }),
              {
                id: "tycoon-cash-text",
                name: "CashLabel",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 12, 0, 6),
                  Size: udim2(1, -24, 1, -12),
                  BackgroundTransparency: 1,
                  Text: "$14,850 (+240/s)",
                  TextColor3: "#4DC296",
                  TextSize: 16,
                  Font: "GothamBold",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
            ],
          },
          {
            id: "tycoon-shop-panel",
            name: "UpgradeDrawer",
            class_name: "ScrollingFrame",
            props: {
              Position: udim2(1, -24, 0.5, -20),
              Size: udim2(0, 232, 0, 176),
              AnchorPoint: [1, 0.5],
              BackgroundColor3: "#12181B",
              BackgroundTransparency: 0.12,
              CanvasSize: udim2(0, 0, 1.4, 0),
              ScrollBarThickness: 4,
              ClipsDescendants: true,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("tycoon-shop-corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("tycoon-shop-pad", "UIPadding", "UIPadding", {
                PaddingTop: [0, 10],
                PaddingBottom: [0, 10],
                PaddingLeft: [0, 10],
                PaddingRight: [0, 10],
              }),
              makeModifier("tycoon-shop-list", "UIListLayout", "UIListLayout", {
                FillDirection: "Vertical",
                Padding: [0, 8],
                HorizontalAlignment: "Center",
              }),
              {
                id: "tycoon-boost-btn",
                name: "DoubleCashButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(0, 0, 0, 0),
                  Size: udim2(1, 0, 0, 48),
                  BackgroundColor3: "#24946E",
                  BackgroundTransparency: 0,
                  Text: "2x Cash Boost (99 R$)",
                  TextColor3: "#12181B",
                  TextSize: 14,
                  Font: "GothamBold",
                  CallbackName: "OnBuyDoubleCash",
                  ProductId: 3736007339,
                  Visible: true,
                  ZIndex: 3,
                },
                children: [
                  makeModifier("tycoon-boost-corner", "UICorner", "UICorner", { CornerRadius: [0, 8] }),
                ],
              },
              {
                id: "tycoon-collect-btn",
                name: "AutoCollectButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(0, 0, 0, 56),
                  Size: udim2(1, 0, 0, 48),
                  BackgroundColor3: "#1E272B",
                  BackgroundTransparency: 0,
                  Text: "Auto Collect",
                  TextColor3: "#F2F7F4",
                  TextSize: 14,
                  Font: "GothamMedium",
                  CallbackName: "OnToggleAutoCollect",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [
                  makeModifier("tycoon-collect-corner", "UICorner", "UICorner", { CornerRadius: [0, 8] }),
                ],
              },
            ],
          },
          {
            id: "tycoon-rebirth-btn",
            name: "RebirthButton",
            class_name: "TextButton",
            props: {
              Position: udim2(0.5, 0, 1, -24),
              Size: udim2(0, 180, 0, 48),
              AnchorPoint: [0.5, 1],
              BackgroundColor3: "#D49B26",
              BackgroundTransparency: 0,
              Text: "Rebirth (+15% Boost)",
              TextColor3: "#14161B",
              TextSize: 14,
              Font: "GothamBold",
              CallbackName: "OnRebirth",
              Visible: true,
              ZIndex: 3,
            },
            children: [
              makeModifier("tycoon-rebirth-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
            ],
          },
        ],
      },
    };
  }

  if (preset === "gacha") {
    return {
      version: 1,
      slug: "gacha-hud",
      name: "GachaBannerHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#6E1ED8", surface: "#171422", text: "#F6F3FF" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: {
          Position: udim2(0, 0, 0, 0),
          Size: udim2(1, 0, 1, 0),
          BackgroundTransparency: 1,
          Visible: true,
          ZIndex: 1,
        },
        children: [
          {
            id: "gacha-gems",
            name: "GemCounter",
            class_name: "TextLabel",
            props: {
              Position: udim2(1, -24, 0, 66),
              Size: udim2(0, 188, 0, 44),
              AnchorPoint: [1, 0],
              BackgroundColor3: "#171422",
              BackgroundTransparency: 0.1,
              Text: "Gems: 2,400",
              TextColor3: "#F6F3FF",
              TextSize: 15,
              Font: "GothamBold",
              Visible: true,
              ZIndex: 3,
            },
            children: [
              makeModifier("gacha-gems-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
              makeModifier("gacha-gems-stroke", "UIStroke", "UIStroke", { Color: "#6E1ED8", Thickness: 1.5, Transparency: 0.25 }),
            ],
          },
          {
            id: "gacha-banner-card",
            name: "BannerFrame",
            class_name: "ImageLabel",
            props: {
              Position: udim2(0.5, 0, 0.5, -10),
              Size: udim2(0, 380, 0, 196),
              AnchorPoint: [0.5, 0.5],
              BackgroundColor3: "#1D182C",
              BackgroundTransparency: 0.06,
              Image: "art/store/banner_frame.png",
              AssetPath: "art/store/banner_frame.png",
              ScaleType: "Slice",
              SliceCenter: [24, 24, 232, 232],
              SliceScale: 1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("gacha-banner-corner", "UICorner", "UICorner", { CornerRadius: [0, 16] }),
              makeModifier("gacha-banner-stroke", "UIStroke", "UIStroke", { Color: "#8B5CF6", Thickness: 2, Transparency: 0.2 }),
              {
                id: "gacha-banner-title",
                name: "BannerTitle",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 16, 0, 16),
                  Size: udim2(1, -32, 0, 36),
                  BackgroundTransparency: 1,
                  Text: "Mythic Banner | Pity 42/80",
                  TextColor3: "#F6F3FF",
                  TextSize: 18,
                  Font: "GothamBold",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
              {
                id: "gacha-roll1-btn",
                name: "RollOneButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(0, 20, 1, -20),
                  Size: udim2(0, 158, 0, 48),
                  AnchorPoint: [0, 1],
                  BackgroundColor3: "#2A2340",
                  BackgroundTransparency: 0,
                  Text: "Roll x1 (160)",
                  TextColor3: "#F6F3FF",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "OnRollSingle",
                  Visible: true,
                  ZIndex: 4,
                },
                children: [
                  makeModifier("gacha-r1-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
                ],
              },
              {
                id: "gacha-roll10-btn",
                name: "RollTenButton",
                class_name: "TextButton",
                props: {
                  Position: udim2(1, -20, 1, -20),
                  Size: udim2(0, 168, 0, 48),
                  AnchorPoint: [1, 1],
                  BackgroundColor3: "#6E1ED8",
                  BackgroundTransparency: 0,
                  Text: "Roll x10 (1600)",
                  TextColor3: "#FFFFFF",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "OnRollTen",
                  ProductId: 3736007342,
                  Visible: true,
                  ZIndex: 4,
                },
                children: [
                  makeModifier("gacha-r10-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
                ],
              },
            ],
          },
        ],
      },
    };
  }

  // Default: "rpg"
  return {
    version: 1,
    slug: "rpg-hud",
    name: "RpgActionHud",
    ignore_gui_inset: true,
    reset_on_spawn: false,
    theme: { accent: "#1562FC", surface: "#141820", text: "#F3F4F6" },
    root: {
      id: "root",
      name: "HudRoot",
      class_name: "Frame",
      props: {
        Position: udim2(0, 0, 0, 0),
        Size: udim2(1, 0, 1, 0),
        BackgroundTransparency: 1,
        BorderSizePixel: 0,
        Visible: true,
        ZIndex: 1,
      },
      children: [
        {
          id: "rpg-vitals",
          name: "VitalsBar",
          class_name: "Frame",
          props: {
            Position: udim2(0, 24, 0, 66),
            Size: udim2(0, 244, 0, 68),
            BackgroundColor3: "#141820",
            BackgroundTransparency: 0.1,
            Visible: true,
            ZIndex: 2,
          },
          children: [
            makeModifier("rpg-vitals-corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
            makeModifier("rpg-vitals-stroke", "UIStroke", "UIStroke", { Color: "#2B3242", Thickness: 1.5, Transparency: 0.15 }),
            {
              id: "rpg-hp-fill",
              name: "HealthFill",
              class_name: "Frame",
              props: {
                Position: udim2(0, 12, 0, 12),
                Size: udim2(0, 192, 0, 16),
                BackgroundColor3: "#31B88A",
                BackgroundTransparency: 0,
                Visible: true,
                ZIndex: 3,
              },
              children: [
                makeModifier("rpg-hp-corner", "UICorner", "UICorner", { CornerRadius: [0, 6] }),
              ],
            },
            {
              id: "rpg-hp-text",
              name: "VitalsLabel",
              class_name: "TextLabel",
              props: {
                Position: udim2(0, 12, 0, 34),
                Size: udim2(1, -24, 0, 24),
                BackgroundTransparency: 1,
                Text: "HP 420/420 | LV 12",
                TextColor3: "#F3F4F6",
                TextSize: 13,
                Font: "GothamMedium",
                Visible: true,
                ZIndex: 3,
              },
              children: [],
            },
          ],
        },
        {
          id: "rpg-quest",
          name: "QuestTracker",
          class_name: "Frame",
          props: {
            Position: udim2(1, -24, 0, 66),
            Size: udim2(0, 212, 0, 74),
            AnchorPoint: [1, 0],
            BackgroundColor3: "#141820",
            BackgroundTransparency: 0.14,
            Visible: true,
            ZIndex: 2,
          },
          children: [
            makeModifier("rpg-quest-corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
            {
              id: "rpg-quest-text",
              name: "QuestLabel",
              class_name: "TextLabel",
              props: {
                Position: udim2(0, 12, 0, 10),
                Size: udim2(1, -24, 1, -20),
                BackgroundTransparency: 1,
                Text: "Quest: Defeat 5 Golems (3/5)",
                TextColor3: "#E5E7EB",
                TextSize: 13,
                Font: "GothamMedium",
                Visible: true,
                ZIndex: 3,
              },
              children: [],
            },
          ],
        },
        {
          id: "rpg-hotbar",
          name: "SkillHotbar",
          class_name: "Frame",
          props: {
            Position: udim2(0.5, 0, 1, -22),
            Size: udim2(0, 296, 0, 58),
            AnchorPoint: [0.5, 1],
            BackgroundColor3: "#141820",
            BackgroundTransparency: 0.12,
            Visible: true,
            ZIndex: 2,
          },
          children: [
            makeModifier("rpg-hotbar-corner", "UICorner", "UICorner", { CornerRadius: [0, 14] }),
            makeModifier("rpg-hotbar-stroke", "UIStroke", "UIStroke", { Color: "#2E3648", Thickness: 1.5, Transparency: 0.2 }),
            makeModifier("rpg-hotbar-pad", "UIPadding", "UIPadding", {
              PaddingTop: [0, 5],
              PaddingBottom: [0, 5],
              PaddingLeft: [0, 8],
              PaddingRight: [0, 8],
            }),
            makeModifier("rpg-hotbar-list", "UIListLayout", "UIListLayout", {
              FillDirection: "Horizontal",
              Padding: [0, 8],
              HorizontalAlignment: "Center",
              VerticalAlignment: "Center",
            }),
            {
              id: "rpg-skill-1",
              name: "SlashButton",
              class_name: "TextButton",
              props: {
                Position: udim2(0, 0, 0, 0),
                Size: udim2(0, 84, 0, 48),
                BackgroundColor3: "#1562FC",
                BackgroundTransparency: 0,
                Text: "[1] Slash",
                TextColor3: "#FFFFFF",
                TextSize: 13,
                Font: "GothamBold",
                CallbackName: "OnSlash",
                Visible: true,
                ZIndex: 3,
              },
              children: [
                makeModifier("rpg-s1-corner", "UICorner", "UICorner", { CornerRadius: [0, 8] }),
              ],
            },
            {
              id: "rpg-skill-2",
              name: "DashButton",
              class_name: "TextButton",
              props: {
                Position: udim2(0, 92, 0, 0),
                Size: udim2(0, 84, 0, 48),
                BackgroundColor3: "#252B3B",
                BackgroundTransparency: 0,
                Text: "[2] Dash",
                TextColor3: "#F3F4F6",
                TextSize: 13,
                Font: "GothamMedium",
                CallbackName: "OnDash",
                Visible: true,
                ZIndex: 3,
              },
              children: [
                makeModifier("rpg-s2-corner", "UICorner", "UICorner", { CornerRadius: [0, 8] }),
              ],
            },
            {
              id: "rpg-potion-btn",
              name: "PotionButton",
              class_name: "ImageButton",
              props: {
                Position: udim2(0, 184, 0, 0),
                Size: udim2(0, 84, 0, 48),
                BackgroundColor3: "#252B3B",
                BackgroundTransparency: 0,
                Image: "art/store/potion_icon.png",
                AssetPath: "art/store/potion_icon.png",
                ScaleType: "Fit",
                CallbackName: "OnUsePotion",
                ProductId: 3736007338,
                Visible: true,
                ZIndex: 3,
              },
              children: [
                makeModifier("rpg-pot-corner", "UICorner", "UICorner", { CornerRadius: [0, 8] }),
              ],
            },
          ],
        },
      ],
    },
  };
}

export function templateScreenDoc(templateId: string): ScreenDoc | null {
  const key = templateId.trim().toLowerCase();
  if (key === "anime-card-battles-apex" || key === "anime-rng-cards" || key === "anime-rng-cards-hud") {
    return {
      version: 1,
      slug: "anime-rng-cards-hud",
      name: "AnimeRngCardsHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#FCD34D", surface: "#0B0B0C", text: "#F5F5F5" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1, Visible: true, ZIndex: 1 },
        children: [
          {
            id: "pity_currency_banner",
            name: "PityCurrencyBanner",
            class_name: "Frame",
            props: {
              AnchorPoint: [1, 0],
              Position: udim2(1, -16, 0, 66),
              Size: udim2(0, 260, 0, 54),
              BackgroundColor3: "#171719",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("pity_banner_corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
              makeModifier("pity_banner_stroke", "UIStroke", "UIStroke", { Color: "#FCD34D", Thickness: 1.5, Transparency: 0.2 }),
              {
                id: "coins_pity_label",
                name: "CoinsPityLabel",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 12, 0, 7),
                  Size: udim2(1, -24, 0, 40),
                  BackgroundTransparency: 1,
                  Text: "250 Coins  •  Pity 0/50",
                  TextColor3: "#F5F5F5",
                  TextSize: 15,
                  Font: "GothamBold",
                  TextXAlignment: "Left",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
            ],
          },
          {
            id: "card_roll_dock",
            name: "CardRollDock",
            class_name: "Frame",
            props: {
              AnchorPoint: [0.5, 1],
              Position: udim2(0.5, 0, 1, -16),
              Size: udim2(0, 380, 0, 62),
              BackgroundColor3: "#171719",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("card_dock_corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("card_dock_list", "UIListLayout", "UIListLayout", {
                FillDirection: "Horizontal",
                HorizontalAlignment: "Center",
                VerticalAlignment: "Center",
                Padding: [0, 8],
              }),
              {
                id: "reveal_free_btn",
                name: "RevealFreeButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 132, 0, 48),
                  LayoutOrder: 1,
                  BackgroundColor3: "#F5F5F5",
                  Text: "Reveal Free",
                  TextColor3: "#111113",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onRevealCard",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("reveal_btn_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
              {
                id: "auto_roll_btn",
                name: "AutoRollButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 108, 0, 48),
                  LayoutOrder: 2,
                  BackgroundColor3: "#1E293B",
                  Text: "Auto Roll",
                  TextColor3: "#F8FAFC",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onToggleAuto",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("auto_btn_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
              {
                id: "deck_open_btn",
                name: "DeckOpenButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 104, 0, 48),
                  LayoutOrder: 3,
                  BackgroundColor3: "#6D28D9",
                  Text: "Deck 4/4",
                  TextColor3: "#FFFFFF",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onOpenDeck",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("deck_btn_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
            ],
          },
        ],
      },
    };
  }

  if (key === "viral-sim-tycoon-template" || key === "viral-sim-tycoon" || key === "viral-sim-tycoon-hud") {
    return {
      version: 1,
      slug: "viral-sim-tycoon-hud",
      name: "ViralSimTycoonHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#10B981", surface: "#111827", text: "#F9FAFB" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1, Visible: true, ZIndex: 1 },
        children: [
          {
            id: "balance_quest_card",
            name: "BalanceQuestCard",
            class_name: "Frame",
            props: {
              AnchorPoint: [1, 0],
              Position: udim2(1, -16, 0, 66),
              Size: udim2(0, 256, 0, 56),
              BackgroundColor3: "#111827",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("tycoon_card_corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
              makeModifier("tycoon_card_stroke", "UIStroke", "UIStroke", { Color: "#10B981", Thickness: 1.5, Transparency: 0.2 }),
              {
                id: "balance_quest_label",
                name: "BalanceQuestLabel",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 10, 0, 8),
                  Size: udim2(1, -20, 0, 40),
                  BackgroundTransparency: 1,
                  Text: "100 Coins  •  Foundry Lv1",
                  TextColor3: "#F9FAFB",
                  TextSize: 15,
                  Font: "GothamBold",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
            ],
          },
          {
            id: "workshop_dock",
            name: "WorkshopDock",
            class_name: "Frame",
            props: {
              AnchorPoint: [0.5, 1],
              Position: udim2(0.5, 0, 1, -16),
              Size: udim2(0, 356, 0, 62),
              BackgroundColor3: "#111827",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("workshop_dock_corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("workshop_dock_list", "UIListLayout", "UIListLayout", {
                FillDirection: "Horizontal",
                HorizontalAlignment: "Center",
                VerticalAlignment: "Center",
                Padding: [0, 10],
              }),
              {
                id: "upgrade_dropper_btn",
                name: "UpgradeDropperButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 160, 0, 48),
                  LayoutOrder: 1,
                  BackgroundColor3: "#10B981",
                  Text: "Upgrade x2",
                  TextColor3: "#0F172A",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onUpgradeDropper",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("upgrade_btn_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
              {
                id: "magnet_shop_btn",
                name: "MagnetShopButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 160, 0, 48),
                  LayoutOrder: 2,
                  BackgroundColor3: "#1E293B",
                  Text: "Magnet Shop",
                  TextColor3: "#F8FAFC",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onOpenShop",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("shop_btn_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
            ],
          },
        ],
      },
    };
  }

  if (key === "anime-action-rpg-td-template" || key === "anime-action-rpg-td" || key === "anime-action-rpg-td-hud") {
    return {
      version: 1,
      slug: "anime-action-rpg-td-hud",
      name: "AnimeActionRpgTdHud",
      ignore_gui_inset: true,
      reset_on_spawn: false,
      theme: { accent: "#F59E0B", surface: "#0F172A", text: "#F8FAFC" },
      root: {
        id: "root",
        name: "HudRoot",
        class_name: "Frame",
        props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1, Visible: true, ZIndex: 1 },
        children: [
          {
            id: "citadel_status_card",
            name: "CitadelStatusCard",
            class_name: "Frame",
            props: {
              AnchorPoint: [1, 0],
              Position: udim2(1, -16, 0, 66),
              Size: udim2(0, 260, 0, 56),
              BackgroundColor3: "#0F172A",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("citadel_card_corner", "UICorner", "UICorner", { CornerRadius: [0, 10] }),
              makeModifier("citadel_card_stroke", "UIStroke", "UIStroke", { Color: "#F59E0B", Thickness: 1.5, Transparency: 0.2 }),
              {
                id: "citadel_status_label",
                name: "CitadelStatusLabel",
                class_name: "TextLabel",
                props: {
                  Position: udim2(0, 10, 0, 8),
                  Size: udim2(1, -20, 0, 40),
                  BackgroundTransparency: 1,
                  Text: "150 Coins  •  Tower F1",
                  TextColor3: "#F8FAFC",
                  TextSize: 15,
                  Font: "GothamBold",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [],
              },
            ],
          },
          {
            id: "citadel_dock",
            name: "CitadelDock",
            class_name: "Frame",
            props: {
              AnchorPoint: [0.5, 1],
              Position: udim2(0.5, 0, 1, -16),
              Size: udim2(0, 360, 0, 62),
              BackgroundColor3: "#0F172A",
              BackgroundTransparency: 0.1,
              Visible: true,
              ZIndex: 2,
            },
            children: [
              makeModifier("citadel_dock_corner", "UICorner", "UICorner", { CornerRadius: [0, 12] }),
              makeModifier("citadel_dock_list", "UIListLayout", "UIListLayout", {
                FillDirection: "Horizontal",
                HorizontalAlignment: "Center",
                VerticalAlignment: "Center",
                Padding: [0, 10],
              }),
              {
                id: "citadel_strike_btn",
                name: "StrikeButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 108, 0, 48),
                  LayoutOrder: 1,
                  BackgroundColor3: "#F59E0B",
                  Text: "Strike",
                  TextColor3: "#0F172A",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onStrike",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("citadel_strike_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
              {
                id: "citadel_warp_btn",
                name: "TowerWarpButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 108, 0, 48),
                  LayoutOrder: 2,
                  BackgroundColor3: "#1D4ED8",
                  Text: "Tower Warp",
                  TextColor3: "#FFFFFF",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onWarpWorld",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("citadel_warp_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
              {
                id: "citadel_armory_btn",
                name: "ArmoryButton",
                class_name: "TextButton",
                props: {
                  Size: udim2(0, 108, 0, 48),
                  LayoutOrder: 3,
                  BackgroundColor3: "#1E293B",
                  Text: "Armory",
                  TextColor3: "#F8FAFC",
                  TextSize: 15,
                  Font: "GothamBold",
                  CallbackName: "onOpenShop",
                  Visible: true,
                  ZIndex: 3,
                },
                children: [makeModifier("citadel_armory_corner", "UICorner", "UICorner", { CornerRadius: [0, 8] })],
              },
            ],
          },
        ],
      },
    };
  }

  return null;
}

export const GUI_PRESET_META: Array<{ id: GuiPresetId; label: string; summary: string }> = [
  { id: "rpg", label: "RPG", summary: "Vitals bar, quest tracker, and skill hotbar" },
  { id: "obby", label: "Obby", summary: "Stage pill, speedrun timer, and skip button" },
  { id: "tycoon", label: "Tycoon", summary: "Cash counter, upgrade drawer, and rebirth" },
  { id: "gacha", label: "Gacha", summary: "9-slice banner frame, gem counter, and roll buttons" },
];

export const UI_KIT_LAYOUT_META: Array<{ id: UiKitLayoutId; label: string }> = [
  { id: "skybridge-crest", label: "Skybridge Crest" },
  { id: "perimeter-command", label: "Perimeter Command" },
  { id: "workshop-ribbon", label: "Workshop Ribbon" },
  { id: "compact-mobile-split", label: "Compact Mobile Split" },
];

export const UI_KIT_SKIN_META: Array<{
  id: UiKitSkinId;
  label: string;
  surface: string;
  elevated: string;
  text: string;
  muted: string;
  accent: string;
  accentFg: string;
  rim: string;
  stroke: string;
  crestFill: string;
  crestTrim: string;
}> = [
  { id: "obsidian-prism", label: "Obsidian Prism", surface: "#0A0D14", elevated: "#131926", text: "#F4F7FC", muted: "#B6C2D9", accent: "#38BDF8", accentFg: "#08101C", rim: "#38BDF8", stroke: "#05070B", crestFill: "#172033", crestTrim: "#7DD3FC" },
  { id: "skybridge-gilded", label: "Skybridge Gilded", surface: "#0B1220", elevated: "#141F33", text: "#F8FAFC", muted: "#CBD5E1", accent: "#F59E0B", accentFg: "#0B1220", rim: "#FBBF24", stroke: "#070B14", crestFill: "#1B2840", crestTrim: "#FCD34D" },
  { id: "sakura-lacquer", label: "Sakura Lacquer", surface: "#120B14", elevated: "#1F1424", text: "#FDF2F8", muted: "#E2C6D8", accent: "#F472B6", accentFg: "#120B14", rim: "#F472B6", stroke: "#09050B", crestFill: "#2A1A30", crestTrim: "#F9A8D4" },
  { id: "cyber-forge", label: "Cyber Forge", surface: "#090F12", elevated: "#111D22", text: "#ECFDF5", muted: "#A7F3D0", accent: "#2DD4BF", accentFg: "#090F12", rim: "#14B8A6", stroke: "#04080A", crestFill: "#152830", crestTrim: "#5EEAD4" },
  { id: "clean-monochrome", label: "Clean Monochrome", surface: "#0A0D14", elevated: "#141923", text: "#F7F8FA", muted: "#B8C0CC", accent: "#E2E8F0", accentFg: "#0A0D14", rim: "#94A3B8", stroke: "#05070B", crestFill: "#1C2330", crestTrim: "#CBD5E1" },
  { id: "meme-pop-arcade", label: "Meme Pop Arcade", surface: "#121520", elevated: "#1B2030", text: "#F8FAFC", muted: "#CBD5E1", accent: "#22C55E", accentFg: "#052E16", rim: "#FACC15", stroke: "#000000", crestFill: "#1E293B", crestTrim: "#38BDF8" },
];

export const UI_KIT_SHEET_META: Array<{ id: UiKitSheetId; label: string }> = [
  { id: "chrome_sheet", label: "9-Slice Chrome" },
  { id: "icons_sheet", label: "12 Game Icons" },
  { id: "rarity_frames_sheet", label: "Rarity Frames" },
];

export function buildUiKitScreenDoc(layout: UiKitLayoutId, skinId: UiKitSkinId): ScreenDoc {
  const s = UI_KIT_SKIN_META.find((x) => x.id === skinId) ?? UI_KIT_SKIN_META[0];
  const slug = `${layout}-${s.id}`;
  const name = slug.split("-").map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join("");
  const corner = (id: string, px = 12): GuiNode => ({ id, name: "UICorner", class_name: "UICorner", props: { CornerRadius: [0, px] }, children: [] });
  const rim = (id: string, col = s.rim): GuiNode => ({ id, name: "UIStroke", class_name: "UIStroke", props: { Color: col, Thickness: 1.5, Transparency: 0.25, ApplyStrokeMode: "Border" }, children: [] });
  const sliceImg = (id: string, nm: string, path: string, slice: [number, number, number, number], tint: string): GuiNode => ({
    id,
    name: nm,
    class_name: "ImageLabel",
    props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1, Image: path, AssetPath: path, ImageColor3: tint, ImageTransparency: 0.18, ScaleType: "Slice", SliceCenter: slice, ZIndex: 1 },
    children: [],
  });
  const lbl = (id: string, nm: string, pos: UDim2Value, size: UDim2Value, txt: string, col = s.text, sz = 14): GuiNode => ({
    id,
    name: nm,
    class_name: "TextLabel",
    props: { Position: pos, Size: size, BackgroundTransparency: 1, Text: txt, TextColor3: col, TextSize: sz, Font: "GothamBold", TextXAlignment: "Center", ZIndex: 2 },
    children: [{ id: `${id}-s`, name: "UIStroke", class_name: "UIStroke", props: { Color: s.stroke, Thickness: 1.8, Transparency: 0.15, ApplyStrokeMode: "Contextual" }, children: [] }],
  });
  const btn = (id: string, nm: string, order: number, w: number, bg: string, fg: string, txt: string, cb: string, r = s.rim): GuiNode => ({
    id,
    name: nm,
    class_name: "TextButton",
    props: { Size: udim2(0, w, 0, 48), LayoutOrder: order, BackgroundColor3: bg, BackgroundTransparency: 0.04, Text: txt, TextColor3: fg, TextSize: 15, Font: "GothamBold", CallbackName: cb },
    children: [corner(`${id}-c`, 10), rim(`${id}-r`, r)],
  });
  const panelPath = `art/ui/${s.id}/modal_panel.png`;
  const crestPath = `art/ui/${s.id}/header_crest.png`;
  const hRow = (id: string, gap: number, btns: GuiNode[]): GuiNode => ({
    id,
    name: "ButtonRow",
    class_name: "Frame",
    props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1, ZIndex: 2 },
    children: [{ id: `${id}-l`, name: "UIListLayout", class_name: "UIListLayout", props: { FillDirection: "Horizontal", HorizontalAlignment: "Center", VerticalAlignment: "Center", Padding: [0, gap] }, children: [] }, ...btns],
  });

  let children: GuiNode[];
  if (layout === "perimeter-command") {
    children = [
      { id: "uk-cmd", name: "CommanderBadge", class_name: "Frame", props: { Position: udim2(0, 16, 0, 64), Size: udim2(0, 224, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-cc"), rim("uk-cr"), sliceImg("uk-ci", "CrestChrome", crestPath, [80, 28, 360, 100], s.crestFill), lbl("uk-cl", "CommanderLabel", udim2(0, 10, 0, 6), udim2(1, -20, 1, -12), "LVL 24  •  RANK S", s.text, 15)] },
      { id: "uk-tr", name: "CurrencyOrbCluster", class_name: "Frame", props: { AnchorPoint: [1, 0], Position: udim2(1, -16, 0, 64), Size: udim2(0, 232, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-tc"), rim("uk-trm"), lbl("uk-tl", "ShardsLabel", udim2(0, 10, 0, 6), udim2(1, -20, 1, -12), "8,120 CR  •  4/5 WINS")] },
      { id: "uk-ms", name: "MilestoneStrip", class_name: "Frame", props: { AnchorPoint: [1, 0], Position: udim2(1, -16, 0, 124), Size: udim2(0, 212, 0, 84), BackgroundColor3: s.surface, BackgroundTransparency: 0.14 }, children: [corner("uk-mc"), rim("uk-mr", s.crestTrim), sliceImg("uk-mi", "MilestoneChrome", panelPath, [64, 48, 448, 272], s.elevated), lbl("uk-ml", "MilestoneLabel", udim2(0, 10, 0, 8), udim2(1, -20, 0, 32), "MILESTONE 3 / 5"), lbl("uk-msub", "MilestoneSub", udim2(0, 10, 0, 42), udim2(1, -20, 0, 28), "NEXT: MYTHIC KEY", s.muted, 12)] },
      { id: "uk-dock", name: "CommandDock", class_name: "Frame", props: { AnchorPoint: [0.5, 1], Position: udim2(0.5, 0, 1, -16), Size: udim2(0, 352, 0, 64), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-dc", 14), rim("uk-dr"), hRow("uk-dl", 8, [btn("uk-b1", "DeployButton", 1, 108, s.accent, s.accentFg, "Deploy", "onDeploy", s.crestTrim), btn("uk-b2", "RosterButton", 2, 104, s.elevated, s.text, "Roster", "onRoster"), btn("uk-b3", "ShopButton", 3, 104, s.elevated, s.text, "Shop", "onShop")])] },
    ];
  } else if (layout === "workshop-ribbon") {
    children = [
      { id: "uk-fq", name: "ForgeQuickPill", class_name: "Frame", props: { Position: udim2(0, 16, 0, 64), Size: udim2(0, 188, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-fc"), rim("uk-fr"), lbl("uk-fl", "ForgeStatus", udim2(0, 8, 0, 6), udim2(1, -16, 1, -12), "ESSENCE 640", s.text, 15)] },
      { id: "uk-sr", name: "MilestoneStepperRibbon", class_name: "Frame", props: { AnchorPoint: [0.5, 0], Position: udim2(0.5, 0, 0, 64), Size: udim2(0, 320, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-sc"), rim("uk-srm", s.crestTrim), sliceImg("uk-si", "StepperChrome", crestPath, [80, 28, 360, 100], s.crestFill), lbl("uk-sl", "StepperText", udim2(0, 10, 0, 6), udim2(1, -20, 1, -12), "TIER I  >  TIER II  >  TIER III")] },
      { id: "uk-wd", name: "WorkshopRibbonDock", class_name: "Frame", props: { AnchorPoint: [0.5, 1], Position: udim2(0.5, 0, 1, -16), Size: udim2(0, 344, 0, 64), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-wc", 14), rim("uk-wr"), sliceImg("uk-wi", "RibbonChrome", panelPath, [64, 48, 448, 272], s.elevated), hRow("uk-wl", 8, [btn("uk-b1", "ForgeButton", 1, 106, s.accent, s.accentFg, "Forge", "onForge", s.crestTrim), btn("uk-b2", "InspectButton", 2, 104, s.elevated, s.text, "Inspect", "onInspect"), btn("uk-b3", "AutoButton", 3, 98, s.elevated, s.text, "Auto", "onToggleAuto")])] },
    ];
  } else if (layout === "compact-mobile-split") {
    children = [
      { id: "uk-ss", name: "SplitStatusCard", class_name: "Frame", props: { Position: udim2(0, 16, 0, 64), Size: udim2(0, 206, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-sc"), rim("uk-sr"), sliceImg("uk-si", "StatusChrome", crestPath, [80, 28, 360, 100], s.crestFill), lbl("uk-sl", "StatusTitle", udim2(0, 10, 0, 6), udim2(1, -20, 1, -12), "HP 100%  •  W3", s.text, 15)] },
      { id: "uk-sp", name: "SplitCurrencyBag", class_name: "Frame", props: { AnchorPoint: [1, 0], Position: udim2(1, -16, 0, 64), Size: udim2(0, 240, 0, 56), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-spc"), rim("uk-spr"), lbl("uk-spl", "SplitCoins", udim2(0, 10, 0, 6), udim2(0, 146, 1, -12), "1,250 G", s.text, 15), { id: "uk-bag", name: "BagButton", class_name: "TextButton", props: { AnchorPoint: [1, 0.5], Position: udim2(1, -6, 0.5, 0), Size: udim2(0, 68, 0, 44), BackgroundColor3: s.elevated, BackgroundTransparency: 0.04, Text: "Bag", TextColor3: s.text, TextSize: 14, Font: "GothamBold", CallbackName: "onOpenBag" }, children: [corner("uk-bc", 8), rim("uk-br")] }] },
      { id: "uk-sd", name: "SplitActionDock", class_name: "Frame", props: { AnchorPoint: [0.5, 1], Position: udim2(0.5, 0, 1, -16), Size: udim2(0, 312, 0, 64), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-sdc", 14), rim("uk-sdr"), sliceImg("uk-sdi", "SplitDockChrome", panelPath, [64, 48, 448, 272], s.elevated), hRow("uk-sdl", 12, [btn("uk-b1", "RollButton", 1, 140, s.accent, s.accentFg, "Roll Card", "onRoll", s.crestTrim), btn("uk-b2", "FightButton", 2, 136, s.elevated, s.text, "Auto Battle", "onAutoBattle")])] },
    ];
  } else {
    children = [
      { id: "uk-crest", name: "SkybridgeCrestBanner", class_name: "Frame", props: { AnchorPoint: [0.5, 0], Position: udim2(0.5, 0, 0, 64), Size: udim2(0, 260, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-cc"), rim("uk-cr", s.crestTrim), sliceImg("uk-ci", "CrestChrome", crestPath, [80, 28, 360, 100], s.crestFill), lbl("uk-cl", "CrestTitle", udim2(0, 12, 0, 6), udim2(1, -24, 1, -12), "SKYBRIDGE  •  STAGE 12", s.text, 15)] },
      { id: "uk-curr", name: "CurrencyPill", class_name: "Frame", props: { AnchorPoint: [1, 0], Position: udim2(1, -16, 0, 64), Size: udim2(0, 224, 0, 52), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-curc"), rim("uk-curr-r"), sliceImg("uk-curi", "PanelChrome", panelPath, [64, 48, 448, 272], s.elevated), lbl("uk-curl", "CurrencyValue", udim2(0, 10, 0, 6), udim2(1, -20, 1, -12), "2,450 SHARDS  •  PITY 18/50")] },
      { id: "uk-rail", name: "QuickNavRail", class_name: "Frame", props: { Position: udim2(0, 16, 0, 124), Size: udim2(0, 152, 0, 88), BackgroundColor3: s.surface, BackgroundTransparency: 0.14 }, children: [corner("uk-rc"), rim("uk-rr"), hRow("uk-rl", 8, [btn("uk-rb1", "CodexButton", 1, 64, s.elevated, s.text, "Codex", "onOpenCodex"), btn("uk-rb2", "QuestButton", 2, 64, s.elevated, s.text, "Quests", "onOpenQuests")])] },
      { id: "uk-dock", name: "PrimaryActionDock", class_name: "Frame", props: { AnchorPoint: [0.5, 1], Position: udim2(0.5, 0, 1, -16), Size: udim2(0, 360, 0, 64), BackgroundColor3: s.surface, BackgroundTransparency: 0.12 }, children: [corner("uk-dc", 14), rim("uk-dr"), sliceImg("uk-di", "DockChrome", panelPath, [64, 48, 448, 272], s.elevated), hRow("uk-dl", 10, [btn("uk-b1", "SummonButton", 1, 110, s.accent, s.accentFg, "Summon x1", "onSummonOne", s.crestTrim), btn("uk-b2", "DeckButton", 2, 108, s.elevated, s.text, "Deck", "onOpenDeck"), btn("uk-b3", "BattleButton", 3, 108, s.elevated, s.text, "Battle", "onStartBattle")])] },
    ];
  }
  return {
    version: 1,
    slug,
    name,
    ignore_gui_inset: true,
    reset_on_spawn: false,
    modal_blur: false,
    theme: { accent: s.accent, surface: s.surface, text: s.text },
    root: { id: "root", name: "HudRoot", class_name: "Frame", props: { Position: udim2(0, 0, 0, 0), Size: udim2(1, 0, 1, 0), BackgroundTransparency: 1 }, children },
  };
}

export function computeNodeRects(doc: ScreenDoc, viewportW: number, viewportH: number): ComputedNodeRect[] {
  const out: ComputedNodeRect[] = [];

  function walk(
    node: GuiNode,
    parentId: string | null,
    parentBox: { x: number; y: number; w: number; h: number },
    depth: number,
    overridePos?: { x: number; y: number },
  ) {
    if (!GUI_VISUAL_CLASSES.has(node.class_name)) return;
    if (node.props.Visible === false) return;

    const pos = node.props.Position ?? udim2(0, 0, 0, 0);
    const size = node.props.Size ?? udim2(0, 120, 0, 48);
    const anchor = node.props.AnchorPoint ?? [0, 0];

    let w = parentBox.w * size.x_scale + size.x_offset;
    let h = parentBox.h * size.y_scale + size.y_offset;

    const aspectNode = node.children.find((c) => c.class_name === "UIAspectRatioConstraint");
    if (aspectNode && typeof aspectNode.props.AspectRatio === "number" && aspectNode.props.AspectRatio > 0) {
      h = w / aspectNode.props.AspectRatio;
    }

    const x = overridePos
      ? overridePos.x
      : parentBox.x + parentBox.w * pos.x_scale + pos.x_offset - w * anchor[0];
    const y = overridePos
      ? overridePos.y
      : parentBox.y + parentBox.h * pos.y_scale + pos.y_offset - h * anchor[1];

    out.push({ node, parentId, x, y, w, h, depth });

    // Compute inner content box honoring UIPadding
    const padNode = node.children.find((c) => c.class_name === "UIPadding");
    const padTop = padNode?.props.PaddingTop ? h * padNode.props.PaddingTop[0] + padNode.props.PaddingTop[1] : 0;
    const padBottom = padNode?.props.PaddingBottom ? h * padNode.props.PaddingBottom[0] + padNode.props.PaddingBottom[1] : 0;
    const padLeft = padNode?.props.PaddingLeft ? w * padNode.props.PaddingLeft[0] + padNode.props.PaddingLeft[1] : 0;
    const padRight = padNode?.props.PaddingRight ? w * padNode.props.PaddingRight[0] + padNode.props.PaddingRight[1] : 0;

    const innerBox = {
      x: x + padLeft,
      y: y + padTop,
      w: Math.max(0, w - padLeft - padRight),
      h: Math.max(0, h - padTop - padBottom),
    };

    const listLayout = node.children.find((c) => c.class_name === "UIListLayout");
    const visualChildren = node.children.filter((c) => GUI_VISUAL_CLASSES.has(c.class_name) && c.props.Visible !== false);

    if (listLayout && visualChildren.length > 0) {
      const horiz = (listLayout.props.FillDirection ?? "Vertical") === "Horizontal";
      const padArr = listLayout.props.Padding ?? [0, 4];
      const gap = (horiz ? innerBox.w : innerBox.h) * padArr[0] + padArr[1];
      const childSizes = visualChildren.map((ch) => {
        const s = ch.props.Size ?? udim2(0, 100, 0, 44);
        return {
          w: innerBox.w * s.x_scale + s.x_offset,
          h: innerBox.h * s.y_scale + s.y_offset,
        };
      });
      const totalSpan =
        childSizes.reduce((acc, s) => acc + (horiz ? s.w : s.h), 0) +
        gap * Math.max(0, childSizes.length - 1);

      let cursorX = innerBox.x;
      let cursorY = innerBox.y;
      const hAlign = listLayout.props.HorizontalAlignment ?? "Left";
      const vAlign = listLayout.props.VerticalAlignment ?? "Top";
      if (horiz && hAlign === "Center") cursorX = innerBox.x + (innerBox.w - totalSpan) / 2;
      else if (horiz && hAlign === "Right") cursorX = innerBox.x + innerBox.w - totalSpan;
      if (!horiz && vAlign === "Center") cursorY = innerBox.y + (innerBox.h - totalSpan) / 2;
      else if (!horiz && vAlign === "Bottom") cursorY = innerBox.y + innerBox.h - totalSpan;

      visualChildren.forEach((ch, idx) => {
        const cs = childSizes[idx];
        let cx = cursorX;
        let cy = cursorY;
        if (horiz) {
          if (vAlign === "Center") cy = innerBox.y + (innerBox.h - cs.h) / 2;
          else if (vAlign === "Bottom") cy = innerBox.y + innerBox.h - cs.h;
          cursorX += cs.w + gap;
        } else {
          if (hAlign === "Center") cx = innerBox.x + (innerBox.w - cs.w) / 2;
          else if (hAlign === "Right") cx = innerBox.x + innerBox.w - cs.w;
          cursorY += cs.h + gap;
        }
        walk(ch, node.id, innerBox, depth + 1, { x: cx, y: cy });
      });
    } else {
      for (const ch of node.children) {
        walk(ch, node.id, innerBox, depth + 1);
      }
    }
  }

  walk(doc.root, null, { x: 0, y: 0, w: viewportW, h: viewportH }, 0);
  return out;
}

function rectsOverlap(
  a: { x: number; y: number; w: number; h: number },
  b: { x: number; y: number; w: number; h: number },
): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function parseHexRgb(hex: string | undefined): [number, number, number] | null {
  if (!hex || typeof hex !== "string") return null;
  const clean = hex.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{6}$/.test(clean)) {
    return [
      parseInt(clean.slice(0, 2), 16),
      parseInt(clean.slice(2, 4), 16),
      parseInt(clean.slice(4, 6), 16),
    ];
  }
  if (/^[0-9a-fA-F]{3}$/.test(clean)) {
    return [
      parseInt(clean[0] + clean[0], 16),
      parseInt(clean[1] + clean[1], 16),
      parseInt(clean[2] + clean[2], 16),
    ];
  }
  return null;
}

function srgbLum(c: number): number {
  const cs = c / 255;
  return cs <= 0.04045 ? cs / 12.92 : Math.pow((cs + 0.055) / 1.055, 2.4);
}

function wcagContrastRatio(fg: [number, number, number], bg: [number, number, number]): number {
  const l1 = 0.2126 * srgbLum(fg[0]) + 0.7152 * srgbLum(fg[1]) + 0.0722 * srgbLum(fg[2]);
  const l2 = 0.2126 * srgbLum(bg[0]) + 0.7152 * srgbLum(bg[1]) + 0.0722 * srgbLum(bg[2]);
  const hi = Math.max(l1, l2);
  const lo = Math.min(l1, l2);
  return (hi + 0.05) / (lo + 0.05);
}

export function analyzeSafeArea(doc: ScreenDoc, devices: GuiDeviceId[] = ["desktop", "tablet", "mobile"]): SafeAreaWarning[] {
  const warnings: SafeAreaWarning[] = [];
  const canvasRgb: [number, number, number] = parseHexRgb(doc.theme?.surface) ?? [0x17, 0x17, 0x19];
  const defaultTextRgb: [number, number, number] = parseHexRgb(doc.theme?.text) ?? [248, 250, 252];

  for (const devId of devices) {
    const spec = GUI_DEVICES[devId];
    const rects = computeNodeRects(doc, spec.width, spec.height);
    const byId = new Map<string, ComputedNodeRect>();
    for (const item of rects) byId.set(item.node.id, item);

    const effectiveBgFor = (item: ComputedNodeRect): [number, number, number] => {
      let cur: ComputedNodeRect | undefined = item;
      while (cur) {
        const bgT = cur.node.props.BackgroundTransparency ?? 0;
        if (bgT < 0.9) {
          const parsed = parseHexRgb(cur.node.props.BackgroundColor3);
          if (parsed) return parsed;
        }
        cur = cur.parentId ? byId.get(cur.parentId) : undefined;
      }
      return canvasRgb;
    };

    for (const item of rects) {
      if (item.node.id === doc.root.id) continue;
      const isFullBackdrop = item.w >= spec.width * 0.88 && item.h >= spec.height * 0.88;
      const isButton = item.node.class_name === "TextButton" || item.node.class_name === "ImageButton";
      const r = {
        x: Math.round(item.x),
        y: Math.round(item.y),
        w: Math.round(item.w),
        h: Math.round(item.h),
      };

      if (doc.ignore_gui_inset && !isFullBackdrop && r.y < 58) {
        warnings.push({
          device: devId,
          node_id: item.node.id,
          node_name: item.node.name,
          kind: "topbar_overlap",
          severity: "warning",
          message: `${item.node.name} sits at y=${r.y}px inside the 58px TopBar CoreGui inset on ${spec.shortLabel}.`,
          rect: r,
        });
      }

      if (devId === "mobile" && !isFullBackdrop) {
        const leftThumbZone = { x: 0, y: spec.height - 170, w: 200, h: 170 };
        const rightJumpZone = { x: spec.width - 180, y: spec.height - 170, w: 180, h: 170 };
        if (rectsOverlap(r, leftThumbZone) || rectsOverlap(r, rightJumpZone)) {
          warnings.push({
            device: devId,
            node_id: item.node.id,
            node_name: item.node.name,
            kind: "thumbstick_overlap",
            severity: "warning",
            message: `${item.node.name} overlaps the Mobile Thumbstick or Jump safe zone (${spec.width}x${spec.height}).`,
            rect: r,
          });
        }
      }

      if (r.x < -1 || r.y < -1 || r.x + r.w > spec.width + 1 || r.y + r.h > spec.height + 1) {
        warnings.push({
          device: devId,
          node_id: item.node.id,
          node_name: item.node.name,
          kind: "offscreen_bounds",
          severity: "error",
          message: `${item.node.name} extends outside ${spec.shortLabel} bounds (${spec.width}x${spec.height}).`,
          rect: r,
        });
      }

      if (isButton && (r.w < 44 || r.h < 44)) {
        warnings.push({
          device: devId,
          node_id: item.node.id,
          node_name: item.node.name,
          kind: "tiny_tap_target",
          severity: "warning",
          message: `${item.node.name} is ${r.w}x${r.h}px (under 44x44px touch target minimum) on ${spec.shortLabel}.`,
          rect: r,
        });
      }

      if (
        (item.node.class_name === "TextLabel" || item.node.class_name === "TextButton") &&
        typeof item.node.props.Text === "string" &&
        item.node.props.Text.trim().length > 0
      ) {
        const fg = parseHexRgb(item.node.props.TextColor3) ?? defaultTextRgb;
        const bg = effectiveBgFor(item);
        const ratio = wcagContrastRatio(fg, bg);
        if (ratio < 4.5) {
          warnings.push({
            device: devId,
            node_id: item.node.id,
            node_name: item.node.name,
            kind: "low_contrast",
            severity: "warning",
            message: `${item.node.name} text contrast ratio is ${ratio.toFixed(2)}:1 (<4.5:1 WCAG AA minimum) on ${spec.shortLabel}.`,
            rect: r,
          });
        }
      }
    }
  }

  return warnings;
}

function luaQuote(s: string): string {
  return `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n")}"`;
}

function luaColor3(hex: string | undefined, fallback = "#FFFFFF"): string {
  const clean = (hex || fallback).replace(/[^#0-9a-fA-F]/g, "");
  return `Color3.fromHex(${luaQuote(clean.startsWith("#") ? clean : `#${clean}`)})`;
}

function luaUDim2(u: UDim2Value): string {
  return `UDim2.new(${Number(u.x_scale.toFixed(4))}, ${Math.round(u.x_offset)}, ${Number(u.y_scale.toFixed(4))}, ${Math.round(u.y_offset)})`;
}

function luaIdent(raw: string, used: Set<string>): string {
  const base = (raw.replace(/[^A-Za-z0-9_]/g, "") || "node").replace(/^[0-9]/, "n_$&");
  const lower = base.charAt(0).toLowerCase() + base.slice(1);
  let cand = lower;
  let i = 2;
  while (used.has(cand)) {
    cand = `${lower}${i++}`;
  }
  used.add(cand);
  return cand;
}

export function compileScreenDocToLuau(doc: ScreenDoc): {
  luau_source: string;
  model_json: string;
  diagnostics: Array<{ severity: "error" | "warning" | "info"; line: number; message: string; code: string }>;
} {
  const usedVars = new Set<string>(["screenGui", "callbacks", "parent", "MarketplaceService", "Players", "Module"]);
  const lines: string[] = [
    "--!strict",
    `-- Generated by OpenRO UI Designer (ScreenDoc v1)`,
    `-- Screen: ${doc.name} (${doc.slug})`,
    "",
    "export type GuiCallbacks = {",
    "\t[string]: (...any) -> (),",
    "}",
    "",
    'local MarketplaceService = game:GetService("MarketplaceService")',
    ...(doc.modal_blur ? ['local Lighting = game:GetService("Lighting")'] : []),
    'local Players = game:GetService("Players")',
    "",
    "local Module = {}",
    ...(doc.modal_blur
      ? [
          "",
          "function Module.setModalBlur(enabled: boolean)",
          '\tlocal existing = Lighting:FindFirstChild("OpenROModalBlur")',
          "\tif enabled then",
          '\t\tlocal blur = if existing and existing:IsA("BlurEffect") then existing else Instance.new("BlurEffect")',
          '\t\tblur.Name = "OpenROModalBlur"',
          "\t\tblur.Size = 16",
          "\t\tblur.Enabled = true",
          "\t\tblur.Parent = Lighting",
          '\telseif existing and existing:IsA("BlurEffect") then',
          "\t\texisting:Destroy()",
          "\tend",
          "end",
        ]
      : []),
    "",
    "function Module.create(parent: Instance, callbacks: GuiCallbacks?): ScreenGui",
    "\tlocal cbMap: GuiCallbacks = callbacks or {}",
    '\tlocal screenGui = Instance.new("ScreenGui")',
    `\tscreenGui.Name = ${luaQuote(doc.name)}`,
    `\tscreenGui.IgnoreGuiInset = ${doc.ignore_gui_inset ? "true" : "false"}`,
    `\tscreenGui.ResetOnSpawn = ${doc.reset_on_spawn ? "true" : "false"}`,
    "\tscreenGui.ZIndexBehavior = Enum.ZIndexBehavior.Sibling",
    "",
  ];

  let hasProducts = false;

  function emitNode(node: GuiNode, parentVar: string) {
    const varName = luaIdent(node.name, usedVars);
    const cls = (GUI_CLASS_ALLOWLIST as readonly string[]).includes(node.class_name) ? node.class_name : "Frame";
    lines.push(`\tlocal ${varName} = Instance.new(${luaQuote(cls)})`);
    lines.push(`\t${varName}.Name = ${luaQuote(node.name)}`);

    const p = node.props;
    if (GUI_VISUAL_CLASSES.has(cls as GuiClassName)) {
      if (p.Position) lines.push(`\t${varName}.Position = ${luaUDim2(p.Position)}`);
      if (p.Size) lines.push(`\t${varName}.Size = ${luaUDim2(p.Size)}`);
      if (p.AnchorPoint) lines.push(`\t${varName}.AnchorPoint = Vector2.new(${p.AnchorPoint[0]}, ${p.AnchorPoint[1]})`);
      if (p.BackgroundColor3) lines.push(`\t${varName}.BackgroundColor3 = ${luaColor3(p.BackgroundColor3)}`);
      if (p.BackgroundTransparency !== undefined) lines.push(`\t${varName}.BackgroundTransparency = ${p.BackgroundTransparency}`);
      if (p.BorderSizePixel !== undefined) lines.push(`\t${varName}.BorderSizePixel = ${Math.round(p.BorderSizePixel)}`);
      else lines.push(`\t${varName}.BorderSizePixel = 0`);
      if (p.Visible !== undefined) lines.push(`\t${varName}.Visible = ${p.Visible ? "true" : "false"}`);
      if (p.ZIndex !== undefined) lines.push(`\t${varName}.ZIndex = ${Math.round(p.ZIndex)}`);
      if (p.ClipsDescendants) lines.push(`\t${varName}.ClipsDescendants = true`);
      if (p.LayoutOrder !== undefined) lines.push(`\t${varName}.LayoutOrder = ${Math.round(p.LayoutOrder)}`);
    }

    if (cls === "TextLabel" || cls === "TextButton") {
      if (p.Text !== undefined) lines.push(`\t${varName}.Text = ${luaQuote(p.Text)}`);
      if (p.TextColor3) lines.push(`\t${varName}.TextColor3 = ${luaColor3(p.TextColor3)}`);
      if (p.TextSize !== undefined) lines.push(`\t${varName}.TextSize = ${Math.round(p.TextSize)}`);
      if (p.TextScaled) lines.push(`\t${varName}.TextScaled = true`);
      if (p.Font) lines.push(`\t${varName}.Font = Enum.Font.${p.Font.replace(/[^A-Za-z0-9]/g, "") || "GothamMedium"}`);
      if (p.TextXAlignment) lines.push(`\t${varName}.TextXAlignment = Enum.TextXAlignment.${p.TextXAlignment}`);
      if (p.TextYAlignment) lines.push(`\t${varName}.TextYAlignment = Enum.TextYAlignment.${p.TextYAlignment}`);
    }

    if (cls === "ImageLabel" || cls === "ImageButton") {
      const imgVal = p.Image || p.AssetPath || "";
      if (imgVal) lines.push(`\t${varName}.Image = ${luaQuote(imgVal)}`);
      if (p.ImageColor3) lines.push(`\t${varName}.ImageColor3 = ${luaColor3(p.ImageColor3)}`);
      if (p.ImageTransparency !== undefined) lines.push(`\t${varName}.ImageTransparency = ${p.ImageTransparency}`);
      if (p.ScaleType) lines.push(`\t${varName}.ScaleType = Enum.ScaleType.${p.ScaleType}`);
      if (p.ScaleType === "Slice" && p.SliceCenter) {
        lines.push(`\t${varName}.SliceCenter = Rect.new(${p.SliceCenter.map((n) => Math.round(n)).join(", ")})`);
        if (p.SliceScale !== undefined) lines.push(`\t${varName}.SliceScale = ${p.SliceScale}`);
      }
    }

    if (cls === "ScrollingFrame") {
      if (p.CanvasSize) lines.push(`\t${varName}.CanvasSize = ${luaUDim2(p.CanvasSize)}`);
      if (p.ScrollBarThickness !== undefined) lines.push(`\t${varName}.ScrollBarThickness = ${Math.round(p.ScrollBarThickness)}`);
    }

    if (cls === "UICorner" && p.CornerRadius) {
      lines.push(`\t${varName}.CornerRadius = UDim.new(${p.CornerRadius[0]}, ${Math.round(p.CornerRadius[1])})`);
    }
    if (cls === "UIStroke") {
      if (p.Color) lines.push(`\t${varName}.Color = ${luaColor3(p.Color)}`);
      if (p.Thickness !== undefined) lines.push(`\t${varName}.Thickness = ${p.Thickness}`);
      if (p.Transparency !== undefined) lines.push(`\t${varName}.Transparency = ${p.Transparency}`);
      if (p.ApplyStrokeMode) lines.push(`\t${varName}.ApplyStrokeMode = Enum.ApplyStrokeMode.${p.ApplyStrokeMode}`);
    }
    if (cls === "UIListLayout") {
      if (p.FillDirection) lines.push(`\t${varName}.FillDirection = Enum.FillDirection.${p.FillDirection}`);
      if (p.Padding) lines.push(`\t${varName}.Padding = UDim.new(${p.Padding[0]}, ${Math.round(p.Padding[1])})`);
      if (p.HorizontalAlignment) lines.push(`\t${varName}.HorizontalAlignment = Enum.HorizontalAlignment.${p.HorizontalAlignment}`);
      if (p.VerticalAlignment) lines.push(`\t${varName}.VerticalAlignment = Enum.VerticalAlignment.${p.VerticalAlignment}`);
      if (p.SortOrder) lines.push(`\t${varName}.SortOrder = Enum.SortOrder.${p.SortOrder}`);
    }
    if (cls === "UIGridLayout") {
      if (p.CellSize) lines.push(`\t${varName}.CellSize = ${luaUDim2(p.CellSize)}`);
      if (p.CellPadding) lines.push(`\t${varName}.CellPadding = ${luaUDim2(p.CellPadding)}`);
    }
    if (cls === "UIAspectRatioConstraint" && p.AspectRatio !== undefined) {
      lines.push(`\t${varName}.AspectRatio = ${p.AspectRatio}`);
    }
    if (cls === "UIPadding") {
      if (p.PaddingTop) lines.push(`\t${varName}.PaddingTop = UDim.new(${p.PaddingTop[0]}, ${Math.round(p.PaddingTop[1])})`);
      if (p.PaddingBottom) lines.push(`\t${varName}.PaddingBottom = UDim.new(${p.PaddingBottom[0]}, ${Math.round(p.PaddingBottom[1])})`);
      if (p.PaddingLeft) lines.push(`\t${varName}.PaddingLeft = UDim.new(${p.PaddingLeft[0]}, ${Math.round(p.PaddingLeft[1])})`);
      if (p.PaddingRight) lines.push(`\t${varName}.PaddingRight = UDim.new(${p.PaddingRight[0]}, ${Math.round(p.PaddingRight[1])})`);
    }

    if ((cls === "TextButton" || cls === "ImageButton") && (p.CallbackName || p.ProductId)) {
      const prodNum = p.ProductId ? Number(p.ProductId) : 0;
      if (prodNum > 0) hasProducts = true;
      lines.push(`\t${varName}.Activated:Connect(function()`);
      if (p.CallbackName) {
        lines.push(`\t\tlocal fn = cbMap[${luaQuote(p.CallbackName)}]`);
        lines.push(`\t\tif fn then fn() end`);
      }
      if (prodNum > 0) {
        lines.push(`\t\tlocal player = Players.LocalPlayer`);
        lines.push(`\t\tif player then MarketplaceService:PromptProductPurchase(player, ${Math.round(prodNum)}) end`);
      }
      lines.push("\tend)");
    }

    lines.push(`\t${varName}.Parent = ${parentVar}`);
    lines.push("");

    for (const ch of node.children) {
      emitNode(ch, varName);
    }
  }

  emitNode(doc.root, "screenGui");

  if (doc.modal_blur) {
    lines.push("\tModule.setModalBlur(true)");
    lines.push("\tscreenGui.Destroying:Connect(function()");
    lines.push("\t\tModule.setModalBlur(false)");
    lines.push("\tend)");
  }
  lines.push("\tscreenGui.Parent = parent");
  lines.push("\treturn screenGui");
  lines.push("end");
  lines.push("");
  lines.push("return Module");

  const luau_source = lines.join("\n");
  const modelJsonObj = toRojoModelJson(doc);
  const diagnostics: Array<{ severity: "error" | "warning" | "info"; line: number; message: string; code: string }> = [];

  if (hasProducts) {
    diagnostics.push({
      severity: "info",
      line: 1,
      message: "ProductId bindings invoke MarketplaceService:PromptProductPurchase on LocalPlayer.",
      code: "GUI_PRODUCT_BOUND",
    });
  }

  return {
    luau_source,
    model_json: JSON.stringify(modelJsonObj, null, 2),
    diagnostics,
  };
}

function toRojoModelNode(node: GuiNode): Record<string, unknown> {
  const properties: Record<string, unknown> = {};
  const p = node.props;
  if (p.Position) properties.Position = [p.Position.x_scale, p.Position.x_offset, p.Position.y_scale, p.Position.y_offset];
  if (p.Size) properties.Size = [p.Size.x_scale, p.Size.x_offset, p.Size.y_scale, p.Size.y_offset];
  if (p.AnchorPoint) properties.AnchorPoint = p.AnchorPoint;
  if (p.BackgroundTransparency !== undefined) properties.BackgroundTransparency = p.BackgroundTransparency;
  if (p.Text !== undefined) properties.Text = p.Text;
  if (p.Image !== undefined) properties.Image = p.Image;
  if (p.ScaleType !== undefined) properties.ScaleType = p.ScaleType;
  if (p.SliceCenter !== undefined) properties.SliceCenter = p.SliceCenter;
  return {
    Name: node.name,
    ClassName: node.class_name,
    Properties: properties,
    Children: node.children.map(toRojoModelNode),
  };
}

export function toRojoModelJson(doc: ScreenDoc): Record<string, unknown> {
  return {
    ClassName: "ScreenGui",
    Name: doc.name,
    Properties: {
      IgnoreGuiInset: doc.ignore_gui_inset,
      ResetOnSpawn: doc.reset_on_spawn,
    },
    Children: [toRojoModelNode(doc.root)],
  };
}

export function findGuiNode(root: GuiNode, id: string): GuiNode | null {
  if (root.id === id) return root;
  for (const ch of root.children) {
    const found = findGuiNode(ch, id);
    if (found) return found;
  }
  return null;
}

export function updateGuiNode(root: GuiNode, id: string, updater: (node: GuiNode) => GuiNode): GuiNode {
  if (root.id === id) return updater(root);
  return {
    ...root,
    children: root.children.map((ch) => updateGuiNode(ch, id, updater)),
  };
}

export function insertGuiChild(root: GuiNode, parentId: string, child: GuiNode): GuiNode {
  if (root.id === parentId) {
    return { ...root, children: [...root.children, child] };
  }
  return {
    ...root,
    children: root.children.map((ch) => insertGuiChild(ch, parentId, child)),
  };
}

export function deleteGuiNode(root: GuiNode, id: string): GuiNode {
  if (root.id === id) return root;
  return {
    ...root,
    children: root.children
      .filter((ch) => ch.id !== id)
      .map((ch) => deleteGuiNode(ch, id)),
  };
}

export function cloneGuiNodeWithNewIds(node: GuiNode, suffix: string): GuiNode {
  return {
    ...node,
    id: `${node.id}-${suffix}`,
    name: `${node.name}Copy`,
    props: JSON.parse(JSON.stringify(node.props)) as GuiNodeProps,
    children: node.children.map((c, idx) => cloneGuiNodeWithNewIds(c, `${suffix}-${idx}`)),
  };
}
