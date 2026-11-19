import type { ComponentProps } from "react";
import { TextureButton } from "@/components/ui/texture-button";
import { cn } from "@/lib/utils";

// The one primary key of a screen: Cult UI TextureButton's slab, re-coloured to the Hum pear token (never its black default).
// The outer ring and the inner slab (a child div with its own gradient) both take the pear.
const KEY =
  "w-auto min-h-11 !border-blue-hover !from-blue-hover !to-blue !text-blue-fg hover:!from-blue hover:!to-blue-hover [&>div]:!from-blue-hover [&>div]:!to-blue [&>div]:!text-blue-fg dark:!border-blue-hover dark:!from-blue-hover dark:!to-blue dark:!text-blue-fg dark:[&>div]:!from-blue-hover dark:[&>div]:!to-blue";

export function Key(props: ComponentProps<typeof TextureButton>) {
  return <TextureButton variant="primary" {...props} className={cn(KEY, props.className)} />;
}
