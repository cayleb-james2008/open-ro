// Type declarations for the house primitives carried verbatim in this directory.
//
// The implementation files are the real FRONTEND-LIBRARY standard sources (see ../PROVENANCE.md) and are plain
// JavaScript; these declarations give the TypeScript renderer the same contract their JSDoc describes, without
// touching the carried source. This is a type shim, not a re-implementation: every function here is the one the
// .js file exports at runtime.
import type { ButtonHTMLAttributes, ReactElement, ReactNode } from 'react';

export const cn: (...inputs: unknown[]) => string;

export function Button(props: {
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md';
  radius?: 'base' | 'control';
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;
export const button: (props?: Record<string, unknown>) => string;

export function Card(props: {
  tone?: 'panel' | 'base' | 'raised';
  as?: keyof HTMLElementTagNameMap | string;
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;
export const CARD: string;

export function Pill(props: {
  tone?: 'live' | 'live-chain' | 'shadow' | 'manual' | 'synthetic' | 'crit' | 'warn' | 'info' | 'neutral';
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;
export const pill: (props?: Record<string, unknown>) => string;

export function Stat(props: {
  size?: 'stat' | 'stat-compact' | 'meta';
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;
export const stat: (props?: Record<string, unknown>) => string;

export function AnimatedNumber(props: {
  value: number | null;
  format: (value: number) => string;
  durationMs?: number;
  reducedMotion?: boolean;
  className?: string;
  [key: string]: unknown;
}): ReactElement;

export function TransitionPanel(props: {
  open: boolean;
  durationMs?: number;
  reducedMotion?: boolean;
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;

export function TextMorph(props: {
  text: string;
  durationMs?: number;
  reducedMotion?: boolean;
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement;

export function MorphingDialog(props: {
  open: boolean;
  onClose: () => void;
  title?: string;
  reducedMotion?: boolean;
  className?: string;
  children?: ReactNode;
  [key: string]: unknown;
}): ReactElement | null;

export const easeOut: (x: number) => number;
export const EASE_OUT: readonly number[];
export const EASE_IN_OUT: readonly number[];
export function useReducedMotion(): boolean;
