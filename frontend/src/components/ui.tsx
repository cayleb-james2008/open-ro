import React, { createContext, useContext, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, LayoutGroup, m, useReducedMotion as useMotionReduced } from "motion/react";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { Loader2 } from "lucide-react";
import { glide, instant, open as openAnim } from "@/motion";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type ButtonVariant = "default" | "outline" | "secondary" | "ghost" | "destructive" | "destructive-outline" | "link";
export type ButtonSize = "xs" | "sm" | "default" | "lg" | "icon" | "icon-sm" | "icon-xs";

const VARIANT_CLS: Record<ButtonVariant, string> = {
  default: "border-primary bg-primary text-primary-foreground shadow-[var(--elev-primary)] hover:opacity-92",
  outline: "border-hairline bg-surface text-ink hover:bg-board",
  secondary: "border-hairline bg-board text-ink hover:bg-track",
  ghost: "border-transparent bg-transparent text-ink-2 hover:bg-board hover:text-ink",
  destructive: "border-destructive bg-destructive text-white hover:opacity-90",
  "destructive-outline": "border-hairline bg-surface text-destructive-foreground hover:bg-pill-pro-bg",
  link: "border-transparent bg-transparent text-blue hover:underline",
};

const SIZE_CLS: Record<ButtonSize, string> = {
  xs: "h-8 gap-1 rounded-md px-2 text-xs",
  sm: "h-8 gap-1.5 rounded-lg px-2.5 text-sm",
  default: "h-9 gap-2 rounded-lg px-3 text-sm",
  lg: "h-10 gap-2 rounded-lg px-4 text-sm",
  icon: "size-9 rounded-lg max-md:min-w-11 max-md:min-h-11",
  "icon-sm": "size-8 rounded-lg max-md:min-w-11 max-md:min-h-11",
  "icon-xs": "size-8 rounded-md max-md:min-w-11 max-md:min-h-11",
};

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  ref?: React.Ref<HTMLButtonElement>;
}

export function Button({
  className,
  variant = "default",
  size = "default",
  loading = false,
  disabled,
  children,
  type = "button",
  ref,
  ...props
}: ButtonProps) {
  const isDisabled = Boolean(disabled || loading);
  return (
    <button
      ref={ref}
      type={type}
      disabled={isDisabled}
      aria-disabled={loading || undefined}
      className={cn(
        "relative inline-flex shrink-0 cursor-pointer items-center justify-center whitespace-nowrap border font-medium transition-[color,background-color,border-color,opacity,transform] duration-(--dur-press) ease-(--ease-house) active:scale-[0.97] motion-reduce:transition-none motion-reduce:active:scale-100 focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-2 disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-hairline disabled:bg-track disabled:text-ink-3 disabled:shadow-none [&_svg]:pointer-events-none [&_svg]:shrink-0",
        VARIANT_CLS[variant],
        SIZE_CLS[size],
        className,
      )}
      {...props}
    >
      {loading ? <Loader2 className="size-3.5 animate-spin" aria-hidden /> : null}
      {children}
    </button>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn("size-4 animate-spin", className)} aria-hidden />;
}

export function Input({
  className,
  ref,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement> & { ref?: React.Ref<HTMLInputElement> }) {
  return (
    <input
      ref={ref}
      className={cn(
        "h-9 w-full min-w-0 rounded-lg border border-[var(--field-border)] bg-surface px-3 text-sm text-ink placeholder:text-ink-3 transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) focus-visible:border-focus focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-0 disabled:cursor-not-allowed disabled:bg-track disabled:text-ink-3",
        className,
      )}
      {...props}
    />
  );
}

export function Textarea({
  className,
  ref,
  ...props
}: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { ref?: React.Ref<HTMLTextAreaElement> }) {
  return (
    <textarea
      ref={ref}
      className={cn(
        "w-full min-w-0 rounded-lg border border-[var(--field-border)] bg-surface px-3 py-2 text-sm text-ink placeholder:text-ink-3 transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) focus-visible:border-focus focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-0 disabled:cursor-not-allowed disabled:bg-track disabled:text-ink-3",
        className,
      )}
      {...props}
    />
  );
}

export function InputGroup({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "flex w-full items-center rounded-lg border border-[var(--field-border)] bg-surface text-ink transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) focus-within:border-focus",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}

export function InputGroupAddon({
  className,
  align: _align,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { align?: "inline-start" | "inline-end" | "block-end" }) {
  return (
    <div className={cn("flex shrink-0 items-center", className)} {...props}>
      {children}
    </div>
  );
}

export function InputGroupInput({
  className,
  ref,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement> & { ref?: React.Ref<HTMLInputElement> }) {
  return (
    <input
      ref={ref}
      className={cn(
        "h-8 w-full min-w-0 flex-1 rounded-md bg-transparent px-2 text-sm text-ink placeholder:text-ink-3 focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-[-1px]",
        className,
      )}
      {...props}
    />
  );
}

export function InputGroupTextarea({
  className,
  ref,
  ...props
}: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { ref?: React.Ref<HTMLTextAreaElement> }) {
  return (
    <textarea
      ref={ref}
      className={cn(
        "w-full min-w-0 flex-1 resize-none rounded-md bg-transparent px-2.5 py-2 text-sm text-ink placeholder:text-ink-3 focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-[-1px]",
        className,
      )}
      {...props}
    />
  );
}

export function Kbd({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) {
  return (
    <kbd
      className={cn(
        "inline-flex h-5 items-center rounded-sm border border-hairline bg-board px-1.5 font-mono text-xs font-medium text-ink-2",
        className,
      )}
      {...props}
    >
      {children}
    </kbd>
  );
}

export function Badge({ className, children, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-chip border border-hairline bg-board px-2 py-0.5 text-xs font-medium text-ink-2",
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}

export function Pill({ className, children, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full bg-board px-2.5 py-0.5 text-xs font-medium text-ink-2",
        className,
      )}
      {...props}
    >
      {children}
    </span>
  );
}

export function PillStatus({ className, children, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span className={cn("inline-flex items-center gap-1.5", className)} {...props}>
      {children}
    </span>
  );
}

export function PillIndicator({
  variant = "info",
  pulse = false,
  className,
}: {
  variant?: "success" | "info" | "warning" | "error";
  pulse?: boolean;
  className?: string;
}) {
  const color =
    variant === "success"
      ? "bg-dot-green"
      : variant === "warning"
        ? "bg-dot-amber"
        : variant === "error"
          ? "bg-dot-red"
          : "bg-dot-blue";
  return (
    <span className={cn("relative inline-flex size-2 shrink-0 rounded-full", color, className)} aria-hidden>
      {pulse ? <span className={cn("absolute inset-0 rounded-full opacity-60 animate-ping", color)} /> : null}
    </span>
  );
}

const SegCtx = createContext<{ size: "sm" | "default" | "lg"; groupId: string }>({
  size: "default",
  groupId: "seg",
});

export function SegmentedControl({
  size = "default",
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { size?: "sm" | "default" | "lg" }) {
  const groupId = useId();
  return (
    <SegCtx.Provider value={{ size, groupId }}>
      <LayoutGroup id={`lg-${groupId}`}>
        <div
          role="group"
          className={cn("inline-flex items-center gap-0.5 rounded-lg bg-track p-0.5", className)}
          {...props}
        >
          {children}
        </div>
      </LayoutGroup>
    </SegCtx.Provider>
  );
}

export function SegmentedControlItem({
  className,
  size,
  children,
  type = "button",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  size?: "sm" | "default" | "lg";
  "data-checked"?: string;
}) {
  const { size: ctxSize, groupId } = useContext(SegCtx);
  const s = size ?? ctxSize;
  const checked = props["data-checked"] !== undefined || props["aria-pressed"] === true;
  const reduced = useMotionReduced();
  return (
    <button
      type={type}
      className={cn(
        "relative inline-flex shrink-0 cursor-pointer select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-md border border-transparent font-medium transition-[color] duration-(--dur-reveal) ease-(--ease-house) motion-reduce:transition-none focus-visible:outline-2 focus-visible:outline-focus focus-visible:outline-offset-1 disabled:pointer-events-none disabled:cursor-not-allowed disabled:text-ink-3 max-md:min-h-11",
        s === "sm" ? "h-8 px-2 text-xs" : s === "lg" ? "h-9 px-3 text-sm" : "h-8 px-2.5 text-sm",
        checked ? "text-ink" : "bg-transparent text-ink-2 hover:text-ink",
        className,
      )}
      {...props}
    >
      {checked ? (
        <m.span
          layoutId={`seg-${groupId}`}
          transition={reduced ? instant : glide}
          aria-hidden
          className="absolute inset-0 rounded-md bg-raised shadow-[var(--elev-raised)]"
        />
      ) : null}
      <span className="relative z-10 inline-flex items-center gap-1.5">{children}</span>
    </button>
  );
}

export function TextShimmer({
  children,
  active = true,
  className,
}: {
  children: React.ReactNode;
  active?: boolean;
  className?: string;
}) {
  const reduced = useMotionReduced();
  if (!active || reduced) {
    return <span className={cn("text-ink", className)}>{children}</span>;
  }
  return (
    <m.span
      initial={{ opacity: 0.72 }}
      animate={{ opacity: [0.72, 1, 0.72] }}
      transition={{ duration: 1.8, repeat: Infinity, ease: "linear" }}
      className={cn("text-ink", className)}
    >
      {children}
    </m.span>
  );
}

export function BorderTrail({ active = true, className }: { active?: boolean; className?: string }) {
  const reduced = useMotionReduced();
  if (!active) return null;
  if (reduced) {
    return <span aria-hidden className={cn("pointer-events-none absolute inset-0 rounded-[inherit] border border-blue/50", className)} />;
  }
  return (
    <m.span
      aria-hidden
      initial={{ opacity: 0.35 }}
      animate={{ opacity: [0.35, 0.85, 0.35] }}
      transition={{ duration: 2, repeat: Infinity, ease: "linear" }}
      className={cn("pointer-events-none absolute inset-0 rounded-[inherit] border border-blue/55", className)}
    />
  );
}

export function ProgressCircle({
  value = 0,
  radius = 15,
  strokeWidth = 3.5,
  className,
  children,
}: {
  value?: number;
  radius?: number;
  strokeWidth?: number;
  className?: string;
  children?: React.ReactNode;
}) {
  const clamped = Math.max(0, Math.min(100, value));
  const normRadius = radius - strokeWidth / 2;
  const circ = 2 * Math.PI * normRadius;
  const offset = circ - (clamped / 100) * circ;
  const size = radius * 2;
  return (
    <div className={cn("relative inline-flex items-center justify-center", className)}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="-rotate-90" aria-hidden>
        <circle
          cx={radius}
          cy={radius}
          r={normRadius}
          fill="none"
          stroke="var(--track)"
          strokeWidth={strokeWidth}
        />
        <circle
          cx={radius}
          cy={radius}
          r={normRadius}
          fill="none"
          stroke="var(--blue)"
          strokeWidth={strokeWidth}
          strokeDasharray={circ}
          strokeDashoffset={offset}
          strokeLinecap="round"
        />
      </svg>
      {children ? <div className="absolute inset-0 flex items-center justify-center">{children}</div> : null}
    </div>
  );
}

export function SlidingNumber({ value }: { value: number }) {
  return <span className="tabular-nums">{value}</span>;
}

type DialogCtxValue = {
  open: boolean;
  setOpen: (v: boolean) => void;
  titleId: string;
  descId: string;
  triggerRef: React.MutableRefObject<HTMLElement | null>;
};

const DialogCtx = createContext<DialogCtxValue | null>(null);

export function Dialog({
  open: controlledOpen,
  onOpenChange,
  children,
}: {
  open?: boolean;
  onOpenChange?: (v: boolean) => void;
  children: React.ReactNode;
  transition?: unknown;
}) {
  const [uncontrolled, setUncontrolled] = useState(false);
  const open = controlledOpen !== undefined ? controlledOpen : uncontrolled;
  const setOpen = (v: boolean) => {
    if (controlledOpen === undefined) setUncontrolled(v);
    onOpenChange?.(v);
  };
  const titleId = useId();
  const descId = useId();
  const triggerRef = useRef<HTMLElement | null>(null);
  return (
    <DialogCtx.Provider value={{ open, setOpen, titleId, descId, triggerRef }}>
      {children}
    </DialogCtx.Provider>
  );
}

export function DialogTrigger({
  className,
  children,
  blocked,
  thing: _thing,
  onClick,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { blocked?: boolean; thing?: string }) {
  const ctx = useContext(DialogCtx);
  return (
    <button
      type="button"
      disabled={Boolean(blocked || props.disabled)}
      onClick={(e) => {
        if (ctx) ctx.triggerRef.current = e.currentTarget;
        onClick?.(e);
        ctx?.setOpen(true);
      }}
      className={className}
      {...props}
    >
      {children}
    </button>
  );
}

export function DialogContainer({ children }: { children: React.ReactNode }) {
  const ctx = useContext(DialogCtx);
  const reduced = useMotionReduced();
  return createPortal(
    <AnimatePresence>
      {ctx?.open ? (
        <m.div
          key="dialog-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={reduced ? instant : openAnim}
          className="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 p-3"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) {
              ctx.setOpen(false);
              ctx.triggerRef.current?.focus();
            }
          }}
        >
          {children}
        </m.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}

export function DialogContent({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  const ctx = useContext(DialogCtx);
  const ref = useRef<HTMLDivElement>(null);
  const reduced = useMotionReduced();

  useEffect(() => {
    if (!ctx?.open) return;
    const prevActive = document.activeElement as HTMLElement | null;
    const el = ref.current;
    const focusables = () =>
      el
        ? ([
            ...el.querySelectorAll<HTMLElement>(
              'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])',
            ),
          ].filter((n) => n.offsetParent !== null || n.getClientRects().length > 0))
        : [];
    const list = focusables();
    const firstPreferred = el?.querySelector<HTMLElement>("[data-autofocus]") ?? list[0];
    firstPreferred?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        ctx.setOpen(false);
        (ctx.triggerRef.current ?? prevActive)?.focus();
        return;
      }
      if (e.key === "Tab") {
        const items = focusables();
        if (items.length === 0) return;
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [ctx]);

  return (
    <m.div
      ref={ref}
      role="dialog"
      aria-modal="true"
      aria-labelledby={ctx?.titleId}
      aria-describedby={ctx?.descId}
      initial={reduced ? { opacity: 1 } : { opacity: 0, y: 6, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={reduced ? { opacity: 0 } : { opacity: 0, y: 4, scale: 0.98 }}
      transition={reduced ? instant : openAnim}
      className={cn(
        "relative w-full rounded-dialog border border-hairline bg-surface text-ink shadow-lift outline-none",
        className,
      )}
      {...(props as Record<string, unknown>)}
    >
      {children}
    </m.div>
  );
}

export function DialogTitle({ className, children }: React.HTMLAttributes<HTMLHeadingElement>) {
  const ctx = useContext(DialogCtx);
  return (
    <h2 id={ctx?.titleId} className={cn("font-heading text-title font-medium text-ink", className)}>
      {children}
    </h2>
  );
}

export function DialogDescription({ className, children }: React.HTMLAttributes<HTMLParagraphElement>) {
  const ctx = useContext(DialogCtx);
  return (
    <p id={ctx?.descId} className={cn("text-sm text-ink-2", className)}>
      {children}
    </p>
  );
}

export function DialogClose({
  className,
  children,
  onClick,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const ctx = useContext(DialogCtx);
  return (
    <button
      type="button"
      onClick={(e) => {
        onClick?.(e);
        ctx?.setOpen(false);
        ctx?.triggerRef.current?.focus();
      }}
      className={cn(
        "inline-flex cursor-pointer items-center justify-center rounded-lg text-ink-2 transition-[color,background-color] duration-(--dur-press) hover:bg-board hover:text-ink focus-visible:outline-2 focus-visible:outline-focus",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}

type ExpandCtx = { expanded: boolean; toggle: () => void };
const ExpandableContext = createContext<ExpandCtx>({ expanded: false, toggle: () => {} });

export function Expandable({
  children,
  defaultExpanded = false,
  expanded: controlledExpanded,
  onToggle,
}: {
  children: React.ReactNode | ((ctx: { isExpanded: boolean }) => React.ReactNode);
  defaultExpanded?: boolean;
  expanded?: boolean;
  onToggle?: () => void;
  expandDirection?: string;
  expandBehavior?: string;
}) {
  const [uncontrolled, setUncontrolled] = useState(defaultExpanded);
  const expanded = controlledExpanded !== undefined ? controlledExpanded : uncontrolled;
  const toggle = () => {
    if (onToggle) onToggle();
    if (controlledExpanded === undefined) setUncontrolled((v) => !v);
  };
  return (
    <ExpandableContext.Provider value={{ expanded, toggle }}>
      <div>{typeof children === "function" ? children({ isExpanded: expanded }) : children}</div>
    </ExpandableContext.Provider>
  );
}

export function ExpandableTrigger({
  className,
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const { expanded, toggle } = useContext(ExpandableContext);
  return (
    <button
      type="button"
      aria-expanded={expanded}
      onClick={toggle}
      className={cn("cursor-pointer", className)}
      {...props}
    >
      {children}
    </button>
  );
}

export function ExpandableContent({
  className,
  children,
}: {
  className?: string;
  children: React.ReactNode;
  preset?: string;
}) {
  const { expanded } = useContext(ExpandableContext);
  if (!expanded) return null;
  return <div className={className}>{children}</div>;
}

export function AnimatedBackground({
  children,
}: {
  children: React.ReactNode;
  defaultValue?: string;
  onValueChange?: (id: string) => void;
  className?: string;
  transition?: unknown;
  enableHover?: boolean;
}) {
  return <>{children}</>;
}
