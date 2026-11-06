// Kitchen — the Ticket, carried into React from the fleet's own dashboard build.
//
// Source on this machine: /home/cayleb/.dsh-fleet-state/missions/m-0035/work/build-dashboard.mjs
// (sha256 82223b141867c2ce6df0c40561a2b2c81b0ddb1f253a9b978d8d5073af96547a), the accepted
// ticket-001 dashboard. The recipe is the one its report names: "a body, a perforated stub, a
// punched notch, a paper sheen". `drawTicketNode()` draws the ticket body over its wall (the
// `.n-depth` and `.n-face` classes), the dashed tear line (`.n-perf`), two punched circles at the
// ends of that line (`.n-notch`) and the id line in the stub; this component keeps the same parts
// as `.ticket-body`, `.ticket-perf`, two `.ticket-notch` disks and `.ticket-stub`.
//
// House rules: a 10 px radius, a 1 px hairline, no shadow. The paper sheen is a single 1 px top
// lip inside the face — never a gradient. Colour comes from public/house/house.css only.
import type { ReactNode } from 'react';
import { cn } from '../ui/cn.js';

export interface TicketProps {
  title?: ReactNode;
  detail?: ReactNode;
  /** The mission or approval id, set in Geist Mono on the stub. */
  meta?: ReactNode;
  className?: string;
  children?: ReactNode;
}

export function Ticket({ title, detail, meta, className, children }: TicketProps) {
  return (
    <article className={cn('ticket', className)}>
      <div className="ticket-body">
        {title != null && <h3 className="ticket-title">{title}</h3>}
        {detail != null && <p className="ticket-detail">{detail}</p>}
        {children}
      </div>
      <div className="ticket-perf" aria-hidden="true">
        <span className="ticket-notch ticket-notch-start" aria-hidden="true" />
        <span className="ticket-notch ticket-notch-end" aria-hidden="true" />
      </div>
      <div className="ticket-stub">{meta}</div>
    </article>
  );
}
