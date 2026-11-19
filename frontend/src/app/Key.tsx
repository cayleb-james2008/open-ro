import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

// The one primary key of a screen: a flat pear card-stock key with a 3px lower edge and a lit top line.
// Press seats it down (2px) and shrinks the edge to 1px. No gradient on the screen (house gate).
const EDGE = "shadow-[0_3px_0_var(--key-edge),inset_0_1px_0_var(--key-lit)]";
export function Key({ className, ...props }: ComponentProps<"button">) {
  return (
    <button
      type="button"
      {...props}
      className={cn(
        "inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-card bg-blue px-4 text-ui font-medium text-blue-fg outline-none",
        EDGE,
        "transition-[transform,background-color] duration-(--dur-press) ease-(--ease-house)",
        "hover:bg-blue-hover active:translate-y-[2px] active:shadow-[0_1px_0_var(--key-edge),inset_0_1px_0_var(--key-lit)]",
        "focus-visible:ring-2 focus-visible:ring-focus focus-visible:ring-offset-2 focus-visible:ring-offset-canvas",
        "disabled:cursor-default disabled:bg-track disabled:text-ink-2 disabled:shadow-none disabled:translate-y-0",
        className,
      )}
    />
  );
}
