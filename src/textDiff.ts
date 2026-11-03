export interface DiffRow {
  kind: 'add' | 'del' | 'context';
  text: string;
}

/** Line diff used by the approval tray. Empty input means "file did not exist", not a blank line. */
export function diffLines(before: string, after: string): DiffRow[] {
  const a = before === '' ? [] : before.replace(/\r\n/g, '\n').split('\n');
  const b = after === '' ? [] : after.replace(/\r\n/g, '\n').split('\n');
  if (a.length * b.length > 250_000) {
    return [
      { kind: 'context', text: `Large change (${a.length} lines to ${b.length} lines). Showing the first 80 lines of each side.` },
      ...a.slice(0, 80).map((text) => ({ kind: 'del' as const, text })),
      ...b.slice(0, 80).map((text) => ({ kind: 'add' as const, text })),
    ];
  }
  const dp: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i -= 1) {
    const row = dp[i];
    const next = dp[i + 1];
    for (let j = b.length - 1; j >= 0; j -= 1) {
      row[j] = a[i] === b[j] ? next[j + 1] + 1 : Math.max(next[j], row[j + 1]);
    }
  }
  const rows: DiffRow[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      rows.push({ kind: 'context', text: a[i] });
      i += 1;
      j += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      rows.push({ kind: 'del', text: a[i] });
      i += 1;
    } else {
      rows.push({ kind: 'add', text: b[j] });
      j += 1;
    }
  }
  while (i < a.length) {
    rows.push({ kind: 'del', text: a[i] });
    i += 1;
  }
  while (j < b.length) {
    rows.push({ kind: 'add', text: b[j] });
    j += 1;
  }
  return rows;
}
