import { useEffect, useState } from "react";

export const ease = [0.23, 1, 0.32, 1] as const;

export const dur = {
  press: 0.15,
  reveal: 0.18,
  swap: 0.2,
  enter: 0.22,
  sheet: 0.22,
} as const;

export const press = { duration: dur.press, ease };
export const glide = { type: "spring" as const, stiffness: 450, damping: 35, mass: 0.7 };
export const open = { duration: dur.enter, ease };
export const layout = { duration: dur.swap, ease };
export const reveal = { duration: dur.reveal, ease };
export const instant = { duration: 0 };

export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() =>
    typeof window !== "undefined" && window.matchMedia
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false,
  );
  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(mq.matches);
    mq.addEventListener("change", update);
    return () => mq.removeEventListener("change", update);
  }, []);
  return reduced;
}
