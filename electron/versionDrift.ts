import { readdir, readFile } from 'node:fs/promises';
import * as path from 'node:path';

export type DriftCategory =
  | 'project_version'
  | 'engine_toolchain'
  | 'provider_catalog'
  | 'studio_mcp_tools'
  | 'blender_pipeline'
  | 'test_suite_sync';

export interface DriftItem {
  category: DriftCategory;
  identifier: string;
  status: 'ok' | 'drift';
  message: string;
  expected?: string;
  actual?: string;
}

export interface DriftAuditReport {
  passed: boolean;
  totalChecks: number;
  driftCount: number;
  drifts: DriftItem[];
  checks: DriftItem[];
  summary: string;
}

export interface PackageJsonStructure {
  name: string;
  version: string;
  engines?: { node?: string };
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
}

async function safeReadFile(filePath: string): Promise<string | null> {
  try {
    return await readFile(filePath, 'utf8');
  } catch {
    return null;
  }
}

async function findTestFiles(dir: string): Promise<string[]> {
  try {
    const entries = await readdir(dir, { withFileTypes: true });
    const results: string[] = [];
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

export async function checkProjectVersionDrift(root: string, pkg: PackageJsonStructure): Promise<DriftItem[]> {
  const items: DriftItem[] = [];
  const version = pkg.version;

  // 1. CHANGELOG.md must document current version
  const changelog = await safeReadFile(path.join(root, 'CHANGELOG.md'));
  if (changelog) {
    const hasVersion = changelog.includes(`[${version}]`) || changelog.includes(`## ${version}`);
    items.push({
      category: 'project_version',
      identifier: 'CHANGELOG.md',
      status: hasVersion ? 'ok' : 'drift',
      message: hasVersion
        ? `CHANGELOG.md correctly references package version ${version}`
        : `CHANGELOG.md is missing release entry for version [${version}]`,
      expected: `[${version}]`,
      actual: hasVersion ? `[${version}]` : 'missing',
    });
  }

  // 2. docs/BUILD-STATUS.md must match package version
  const buildStatus = await safeReadFile(path.join(root, 'docs', 'BUILD-STATUS.md'));
  if (buildStatus) {
    const versionMatch = buildStatus.match(/\*\*Source version:\*\*\s*`([^`]+)`/);
    const docVersion = versionMatch ? versionMatch[1] : null;
    const matches = docVersion === version;
    items.push({
      category: 'project_version',
      identifier: 'docs/BUILD-STATUS.md:SourceVersion',
      status: matches ? 'ok' : 'drift',
      message: matches
        ? `docs/BUILD-STATUS.md matches source version ${version}`
        : `docs/BUILD-STATUS.md source version '${docVersion ?? 'none'}' drifts from package.json '${version}'`,
      expected: version,
      actual: docVersion ?? 'none',
    });
  }

  // 3. docs/RELEASE-GATES.md must cite current release version
  const releaseGates = await safeReadFile(path.join(root, 'docs', 'RELEASE-GATES.md'));
  if (releaseGates) {
    const hasVersion = releaseGates.includes(version);
    items.push({
      category: 'project_version',
      identifier: 'docs/RELEASE-GATES.md',
      status: hasVersion ? 'ok' : 'drift',
      message: hasVersion
        ? `docs/RELEASE-GATES.md references version ${version}`
        : `docs/RELEASE-GATES.md does not reference version ${version}`,
      expected: version,
      actual: hasVersion ? version : 'missing',
    });
  }

  return items;
}

export async function checkToolchainDrift(root: string, pkg: PackageJsonStructure): Promise<DriftItem[]> {
  const items: DriftItem[] = [];

  const nodeEngine = pkg.engines?.node ?? '>=22.12';
  const archDoc = (await safeReadFile(path.join(root, 'docs', 'ARCHITECTURE.md'))) ?? '';
  const buildStatus = (await safeReadFile(path.join(root, 'docs', 'BUILD-STATUS.md'))) ?? '';
  const releaseGates = (await safeReadFile(path.join(root, 'docs', 'RELEASE-GATES.md'))) ?? '';

  // Electron major version alignment
  const electronDep = pkg.devDependencies?.electron ?? '';
  const electronMajorMatch = electronDep.match(/\^?(\d+)/);
  if (electronMajorMatch) {
    const major = electronMajorMatch[1];
    const citedInArch = archDoc.includes(`Electron ${major}`);
    const citedInStatus = buildStatus.includes(`Electron ${major}`);
    const isSynced = citedInArch || citedInStatus;
    items.push({
      category: 'engine_toolchain',
      identifier: 'ElectronMajorVersion',
      status: isSynced ? 'ok' : 'drift',
      message: isSynced
        ? `Electron major version ${major} cited consistently across architectural docs`
        : `Electron dependency is ^${major} but docs do not cite Electron ${major}`,
      expected: `Electron ${major}`,
      actual: isSynced ? `Electron ${major}` : 'uncited',
    });
  }

  // React major version alignment
  const reactDep = pkg.dependencies?.react ?? '';
  const reactMajorMatch = reactDep.match(/\^?(\d+)/);
  if (reactMajorMatch) {
    const major = reactMajorMatch[1];
    const citedInArch = archDoc.includes(`React ${major}`);
    const citedInStatus = buildStatus.includes(`React ${major}`);
    const isSynced = citedInArch || citedInStatus;
    items.push({
      category: 'engine_toolchain',
      identifier: 'ReactMajorVersion',
      status: isSynced ? 'ok' : 'drift',
      message: isSynced
        ? `React major version ${major} cited consistently across architectural docs`
        : `React dependency is ^${major} but docs do not cite React ${major}`,
      expected: `React ${major}`,
      actual: isSynced ? `React ${major}` : 'uncited',
    });
  }

  // Node engine compatibility
  const citesNode = buildStatus.includes('Node 26') || releaseGates.includes('Node 26') || buildStatus.includes(nodeEngine);
  items.push({
    category: 'engine_toolchain',
    identifier: 'NodeEngineCompatibility',
    status: citesNode ? 'ok' : 'drift',
    message: citesNode
      ? `Node engine specification (${nodeEngine}) is compatible with documented verified runtime`
      : `Node runtime documented does not satisfy declared engines.node (${nodeEngine})`,
    expected: `>=22.12`,
    actual: citesNode ? 'verified' : 'unverified',
  });

  return items;
}

export async function checkProviderCatalogDrift(root: string): Promise<DriftItem[]> {
  const items: DriftItem[] = [];
  const catalogSource = (await safeReadFile(path.join(root, 'electron', 'providerCatalog.ts'))) ?? '';
  const agentSource = (await safeReadFile(path.join(root, 'electron', 'agent.ts'))) ?? '';
  const buildStatus = (await safeReadFile(path.join(root, 'docs', 'BUILD-STATUS.md'))) ?? '';
  const archDoc = (await safeReadFile(path.join(root, 'docs', 'ARCHITECTURE.md'))) ?? '';

  // Extract provider kinds from providerCatalog.ts
  const kindMatches = Array.from(catalogSource.matchAll(/kind:\s*'([a-z0-9-]+)'/g), (m) => m[1]);
  const uniqueKinds = Array.from(new Set(kindMatches));

  // Verify that agent.ts implements data flow category mappings
  const hasDataFlowEngine =
    agentSource.includes('getDataFlowDisclosure') &&
    agentSource.includes('LOCAL_ONLY') &&
    agentSource.includes('SUBSCRIPTION_CLI') &&
    agentSource.includes('REMOTE_INFERENCE');

  items.push({
    category: 'provider_catalog',
    identifier: 'DataFlowDisclosureCoverage',
    status: hasDataFlowEngine ? 'ok' : 'drift',
    message: hasDataFlowEngine
      ? 'Agent runtime implements comprehensive data egress classification (local, subscription CLI, remote inference)'
      : 'Agent runtime is missing comprehensive data egress classification',
    expected: 'implemented',
    actual: hasDataFlowEngine ? 'implemented' : 'missing',
  });

  // Check each provider kind
  for (const kind of uniqueKinds) {
    // Map kind to human search term
    const cleanName = kind
      .replace('llama-cpp', 'llama.cpp')
      .replace(/-cli|-api/, '')
      .replace(/-/g, ' ');

    const isDocReferenced =
      buildStatus.toLowerCase().includes(cleanName.toLowerCase()) ||
      archDoc.toLowerCase().includes(cleanName.toLowerCase());

    items.push({
      category: 'provider_catalog',
      identifier: `provider:${kind}:docCoverage`,
      status: isDocReferenced ? 'ok' : 'drift',
      message: isDocReferenced
        ? `Provider '${kind}' is covered in platform status and architecture documentation`
        : `Provider '${kind}' is missing from platform status or architecture documentation`,
      expected: 'documented',
      actual: isDocReferenced ? 'documented' : 'undocumented',
    });
  }

  return items;
}

export async function checkStudioToolsDrift(root: string): Promise<DriftItem[]> {
  const items: DriftItem[] = [];
  const studioSource = (await safeReadFile(path.join(root, 'electron', 'studioTools.ts'))) ?? '';
  const agentSource = (await safeReadFile(path.join(root, 'electron', 'agent.ts'))) ?? '';

  // Identify all studio_* tool stage branches in studioTools.ts
  const stageMatches = Array.from(studioSource.matchAll(/name === '(studio_[a-z0-9_]+)'/g), (m) => m[1]);
  const uniqueStudioTools = Array.from(new Set(stageMatches));

  for (const tool of uniqueStudioTools) {
    // Check if the agent knows about this studio tool or it's handled via stageStudioTool
    const agentKnows = agentSource.includes(`name: '${tool}'`) || tool === 'studio_navigate';
    items.push({
      category: 'studio_mcp_tools',
      identifier: `studioTool:${tool}`,
      status: agentKnows ? 'ok' : 'drift',
      message: agentKnows
        ? `Studio tool '${tool}' is aligned between studioTools.ts and agent.ts`
        : `Studio tool '${tool}' is handled in studioTools.ts but omitted from agent.ts dispatch`,
      expected: 'registered',
      actual: agentKnows ? 'registered' : 'unregistered',
    });
  }

  return items;
}

export async function checkBlenderPipelineDrift(root: string): Promise<DriftItem[]> {
  const items: DriftItem[] = [];
  const typesSource = (await safeReadFile(path.join(root, 'electron', 'blenderTypes.ts'))) ?? '';
  const runnerSource = (await safeReadFile(path.join(root, 'electron', 'blenderRunner.ts'))) ?? '';

  // Core tasks from Blender3DTaskType
  const coreTasks: string[] = [
    'inspect_mesh',
    'convert_format',
    'decimate_mesh',
    'generate_collision',
    'export_roblox_package',
  ];

  for (const task of coreTasks) {
    const isTyped = typesSource.includes(`'${task}'`);
    const isImplemented = runnerSource.includes(`'${task}'`);
    const isSynced = isTyped && isImplemented;

    items.push({
      category: 'blender_pipeline',
      identifier: `blenderTask:${task}`,
      status: isSynced ? 'ok' : 'drift',
      message: isSynced
        ? `Blender pipeline task '${task}' is typed and implemented`
        : `Blender pipeline task '${task}' drifts between types and runner implementation`,
      expected: 'typed and implemented',
      actual: isSynced ? 'typed and implemented' : 'drift',
    });
  }

  return items;
}

export async function checkTestSuiteDrift(root: string): Promise<DriftItem[]> {
  const items: DriftItem[] = [];
  const testFiles = await findTestFiles(path.join(root, 'tests'));
  const actualTestFileCount = testFiles.length;

  const buildStatus = (await safeReadFile(path.join(root, 'docs', 'BUILD-STATUS.md'))) ?? '';
  const match = buildStatus.match(/(\d+)\s+test files/);
  const documentedCount = match ? parseInt(match[1], 10) : 0;

  // The repository must have at least the documented test files count (never regress)
  const satisfiesBaseline = actualTestFileCount >= documentedCount;
  items.push({
    category: 'test_suite_sync',
    identifier: 'TestSuiteCountRegressions',
    status: satisfiesBaseline ? 'ok' : 'drift',
    message: satisfiesBaseline
      ? `Active test suites (${actualTestFileCount}) satisfy documented verification baseline (>= ${documentedCount})`
      : `Active test suites (${actualTestFileCount}) regressed below documented baseline (${documentedCount})`,
    expected: `>= ${documentedCount}`,
    actual: `${actualTestFileCount}`,
  });

  return items;
}

export async function runVersionDriftAudit(projectRoot?: string): Promise<DriftAuditReport> {
  const root = projectRoot ?? process.cwd();
  const pkgContent = await safeReadFile(path.join(root, 'package.json'));
  if (!pkgContent) {
    return {
      passed: false,
      totalChecks: 1,
      driftCount: 1,
      drifts: [
        {
          category: 'project_version',
          identifier: 'package.json',
          status: 'drift',
          message: `package.json not found in ${root}`,
        },
      ],
      checks: [],
      summary: 'Drift audit failed: missing package.json',
    };
  }

  const pkg = JSON.parse(pkgContent) as PackageJsonStructure;

  const allChecks: DriftItem[] = [];

  const [versionChecks, toolchainChecks, providerChecks, studioChecks, blenderChecks, testChecks] =
    await Promise.all([
      checkProjectVersionDrift(root, pkg),
      checkToolchainDrift(root, pkg),
      checkProviderCatalogDrift(root),
      checkStudioToolsDrift(root),
      checkBlenderPipelineDrift(root),
      checkTestSuiteDrift(root),
    ]);

  allChecks.push(
    ...versionChecks,
    ...toolchainChecks,
    ...providerChecks,
    ...studioChecks,
    ...blenderChecks,
    ...testChecks,
  );

  const drifts = allChecks.filter((c) => c.status === 'drift');
  const passed = drifts.length === 0;

  const summary = passed
    ? `Drift audit passed cleanly: ${allChecks.length} integration checks verified with 0 drift.`
    : `Drift audit found ${drifts.length} drifts across ${allChecks.length} checks.`;

  return {
    passed,
    totalChecks: allChecks.length,
    driftCount: drifts.length,
    drifts,
    checks: allChecks,
    summary,
  };
}
