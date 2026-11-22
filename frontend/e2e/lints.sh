#!/bin/bash
# lints.sh <url> <outdir>: taste lint (source), impeccable detect, taste inspect, Hallmark sloplint app scope (render).
set -u
U=$1; G=$2; mkdir -p "$G"
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TASTE_DIR=${TASTE_DIR:-$HOME/refskills/taste/skills/taste-skill}
IMPECCABLE=${IMPECCABLE:-$HOME/refskills/impeccable/.claude/skills/impeccable/scripts/impeccable}
HALLMARK=${HALLMARK:-$HOME/.agents/skills/hallmark/scripts/sloplint.mjs}
export CHROME_PATH=${CHROME_PATH:-${E2E_CHROME:-$HOME/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome}} HALLMARK_PUPPETEER=${HALLMARK_PUPPETEER:-$HOME/.npm/_npx/17406ab935e1f629}
have() { [ -e "$1" ] || { echo "SKIP $2: $1 not found"; return 1; }; }
cd "$G"
cp "$ROOT/assets/index.html" "$G/index.html"
have "$TASTE_DIR/scripts/lint.mjs" lint && timeout 120 node "$TASTE_DIR/scripts/lint.mjs" "$ROOT/frontend/src/app/Blueprints.tsx" "$ROOT/frontend/src/app/Thread.tsx" "$ROOT/frontend/src/App.tsx" --json > "$G/lint.json" 2>&1
have "$IMPECCABLE" detect && timeout 120 "$IMPECCABLE" detect --json "$U" > "$G/detect.json" 2>/dev/null
have "$TASTE_DIR/scripts/inspect.mjs" inspect && timeout 300 node "$TASTE_DIR/scripts/inspect.mjs" "$U" --out "$G/inspect" --json > "$G/inspect.json" 2>/dev/null
have "$HALLMARK" sloplint && timeout 300 node "$HALLMARK" "$G/index.html" --url "$U" --scope app --json > "$G/slop.json" 2>/dev/null
python3 - "$G" <<'PY'
import json,collections,sys,os
b=sys.argv[1]+'/'
def load(n):
    if not os.path.exists(b+n): return None
    return json.load(open(b+n))
try:
    L=load('lint.json')
    if L is not None:
        f=L['findings'] if isinstance(L,dict) else L
        print('LINT', [(x['level'],x['id'],x['file'].split('/')[-1],x.get('line'),x.get('match','')[:40]) for x in f])
except Exception as e: print('LINT ERR', open(b+'lint.json').read()[-500:])
try:
    D=load('detect.json')
    if D is not None: print('DETECT', [(x.get('antipattern'),(x.get('snippet') or '')[:70],x.get('selector','')[:40]) for x in D])
except Exception as e: print('DETECT ERR', e)
try:
    I=load('inspect.json'); seen=set()
    for x in (I or {}).get('findings',[]):
        k=(x.get('level'),x.get('id'))
        if x.get('level') in ('FAIL','WARN') and k not in seen: seen.add(k); print('INSPECT',x['level'],x['id'],'|',str(x.get('message'))[:200])
except Exception as e: print('INSPECT ERR', e)
try:
    S=load('slop.json')
    if S is not None:
        print('SLOP', S['summary'])
        for x in S['findings']:
            if x['grade'] in ('FAIL','WARN','WAIVER'): print('  ',x['grade'],x['gate'],x['evidence'][:160])
except Exception as e: print('SLOP ERR', e)
PY
