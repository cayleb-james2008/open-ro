import { clsx, type ClassValue } from 'clsx';

/** Class-name joiner used by the vendored Motion Primitives and Square UI components. */
export function cn(...inputs: ClassValue[]): string {
  return clsx(inputs);
}
