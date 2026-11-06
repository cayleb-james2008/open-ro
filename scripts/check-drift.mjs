import { readdir, readFile } from 'node:fs/promises';
import * as path from 'node:path';

async function safeReadFile(filePath) {
  try {
    return await readFile(filePath, 'utf8');
  } catch {
    return null;
  }
}

async function findTestFiles(dir) {
  try {
    const entries = await readdir(dir, { withFileTypes: true });
    const results = [];
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        results.push(...(await findTestFiles(full)));
      } else if (/\.test\.ts$/.test(entry.name)) {
        results.push(full);
      }
    }
    return results;
  } catch {
    return [];
  }
}

const root = process.cwd();
const pkgContent = await safeReadFile(path.join(root, 'package.json'));
if (!pkgContent) {
  console.error('Error: package.json not found in project root.');
  process.exit(1);
}

const pkg = JSON.parse(pkgContent);
const version = pkg.version;
const failures = [];
let totalChecks = 0;

function check(assertion, description, details) {
  totalChecks += 1;
  if (!assertion) {
    failures.push(`[DRIFT] ${description}${details ? ` (${details})` : ''}`);
  }
}

// 1. Version drift
const changelog = (await safeReadFile(path.join(root, 'CHANGELOG.md'))) ?? '';
check(
  changelog.includes(`[${version}]`) || changelog.includes(`## ${version}`),
  'CHANGELOG.md references current package version',
  `expected: [${version}]`
);

const buildStatus = (await safeReadFile(path.join(root, 'docs', 'BUILD-STATUS.md'))) ?? '';
const statusVersionMatch = buildStatus.match(/\*\*Source version:\*\*\s*`([^`]+)`/);
check(
  statusVersionMatch && statusVersionMatch[1] === version,
  'docs/BUILD-STATUS.md matches package.json version',
  `expected ${version}, found ${statusVersionMatch ? statusVersionMatch[1] : 'none'}`
);

const releaseGates = (await safeReadFile(path.join(root, 'docs', 'RELEASE-GATES.md'))) ?? '';
check(
  releaseGates.includes(version),
  'docs/RELEASE-GATES.md references current package version',
  `expected ${version}`
);

// 2. Engine and dependencies alignment
const archDoc = (await safeReadFile(path.join(root, 'docs', 'ARCHITECTURE.md'))) ?? '';
const electronDep = pkg.devDependencies?.electron ?? '';
const electronMajor = electronDep.match(/\^?(\d+)/)?.[1];
if (electronMajor) {
  check(
    archDoc.includes(`Electron ${electronMajor}`) || buildStatus.includes(`Electron ${electronMajor}`),
    `Electron major version ${electronMajor} documented in architectural specs`
  );
}

const reactDep = pkg.dependencies?.react ?? '';
const reactMajor = reactDep.match(/\^?(\d+)/)?.[1];
if (reactMajor) {
  check(
    archDoc.includes(`React ${reactMajor}`) || buildStatus.includes(`React ${reactMajor}`),
    `React major version ${reactMajor} documented in architectural specs`
  );
}

// 3. Provider catalog alignment & data flow
const catalogSource = (await safeReadFile(path.join(root, 'electron', 'providerCatalog.ts'))) ?? '';
const agentSource = (await safeReadFile(path.join(root, 'electron', 'agent.ts'))) ?? '';

check(
  agentSource.includes('getDataFlowDisclosure') &&
  agentSource.includes('LOCAL_ONLY') &&
  agentSource.includes('SUBSCRIPTION_CLI') &&
  agentSource.includes('REMOTE_INFERENCE'),
  'Agent implements data egress disclosure categories (LOCAL_ONLY, SUBSCRIPTION_CLI, REMOTE_INFERENCE)'
);

const kindMatches = Array.from(catalogSource.matchAll(/kind:\s*'([a-z0-9-]+)'/g), (m) => m[1]);
const uniqueKinds = Array.from(new Set(kindMatches));

for (const kind of uniqueKinds) {
  const cleanName = kind
    .replace('llama-cpp', 'llama.cpp')
    .replace(/-cli|-api/, '')
    .replace(/-/g, ' ');

  check(
    buildStatus.toLowerCase().includes(cleanName.toLowerCase()) ||
    archDoc.toLowerCase().includes(cleanName.toLowerCase()),
    `Provider '${kind}' documented in platform build status or architecture`
  );
}

// 4. Studio tools alignment
const studioSource = (await safeReadFile(path.join(root, 'electron', 'studioTools.ts'))) ?? '';
const stageMatches = Array.from(studioSource.matchAll(/name === '(studio_[a-z0-9_]+)'/g), (m) => m[1]);
const uniqueStudioTools = Array.from(new Set(stageMatches));

for (const tool of uniqueStudioTools) {
  check(
    agentSource.includes(`name: '${tool}'`) || tool === 'studio_navigate',
    `Studio tool '${tool}' aligned between studioTools.ts and agent.ts`
  );
}

// 5. Blender 3D pipeline tasks
const typesSource = (await safeReadFile(path.join(root, 'electron', 'blenderTypes.ts'))) ?? '';
const runnerSource = (await safeReadFile(path.join(root, 'electron', 'blenderRunner.ts'))) ?? '';
const coreBlenderTasks = ['inspect_mesh', 'convert_format', 'decimate_mesh', 'generate_collision', 'export_roblox_package'];

for (const task of coreBlenderTasks) {
  check(
    typesSource.includes(`'${task}'`) && runnerSource.includes(`'${task}'`),
    `Blender pipeline task '${task}' defined in types and runner`
  );
}

// 6. Test suite non-regression
const testFiles = await findTestFiles(path.join(root, 'tests'));
const testFilesMatch = buildStatus.match(/(\d+)\s+test files/);
const documentedBaseline = testFilesMatch ? parseInt(testFilesMatch[1], 10) : 0;
check(
  testFiles.length >= documentedBaseline,
  `Test suite count (${testFiles.length}) meets documented baseline (>= ${documentedBaseline})`
);

if (failures.length > 0) {
  console.error(`\nIntegration version drift detected (${failures.length} issues across ${totalChecks} checks):`);
  for (const failure of failures) {
    console.error(`  - ${failure}`);
  }
  process.exit(1);
}

console.log(`Drift check passed: verified ${totalChecks} version & integration touchpoints with 0 drift.`);
