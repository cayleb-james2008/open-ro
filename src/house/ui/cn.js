// The class joiner (the shadcn primitive): class names in any order, falsy values dropped, arrays and objects
// flattened. This pack composes its classes deterministically - a variant never contradicts another variant's
// utility - so tailwind-merge is not part of the stack; the joiner is clsx, the same one shadcn's `cn` wraps.
import { clsx } from 'clsx';

export const cn = clsx;
export default cn;
