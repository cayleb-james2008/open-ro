import * as DialogPrimitive from '@radix-ui/react-dialog';
import type { ReactNode } from 'react';

/**
 * Modal on the Radix Dialog primitive (fleet primitives standard): focus
 * trap, Escape to close, focus restoration and WAI-ARIA wiring come from
 * Radix; the surface styling is the Square UI grammar in styles.css.
 * The props API is unchanged from the previous hand-rolled dialog.
 */
export function Dialog({ labelledBy, onClose, children, className }: { labelledBy: string; onClose: () => void; children: ReactNode; className?: string }) {
  return (
    <DialogPrimitive.Root open onOpenChange={(open) => { if (!open) { onClose(); } }}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="modal-backdrop" />
        <DialogPrimitive.Content className={className ?? 'modal-card'} aria-labelledby={labelledBy} aria-describedby={undefined}>
          {children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
