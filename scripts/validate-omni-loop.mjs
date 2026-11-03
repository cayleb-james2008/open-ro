import { readFile } from 'node:fs/promises';

function fail(message) {
  console.error(message);
  process.exitCode = 1;
}

const genesis = JSON.parse(await readFile('.omni-loop/genesis-dag.json', 'utf8'));
const contract = JSON.parse(await readFile('.omni-loop/current-contract.json', 'utf8'));
const verdict = JSON.parse(await readFile('.omni-loop/last-verdict.json', 'utf8'));
const state = JSON.parse(await readFile('.omni-loop/state.json', 'utf8'));

const harnesses = new Set(['H1_SYSTEMS_CODE', 'H2_DEEP_RESEARCH', 'H3_QUANT_DATA', 'H4_FORMAL_ALGO', 'H5_PUBLICATION_DOCS', 'H6_AUTONOMOUS_OPS']);
const dimensions = new Set(['U01', 'U02', 'U03', 'U04', 'U05', 'U06', 'U07', 'U08', 'U09', 'U10', 'U11', 'U12', 'U13', 'U14', 'U15', 'U16', 'LOOP_D_PROCESS']);

if (genesis.schema_version !== '4.0.0') fail('genesis-dag schema_version');
if (!['0A_ZERO_TO_ONE_GENESIS', '0B_BROWNFIELD_BASELINE'].includes(genesis.entry_mode)) fail('genesis entry_mode');
if (!Array.isArray(genesis.active_harnesses) || genesis.active_harnesses.some((item) => !harnesses.has(item))) fail('genesis harnesses');
for (const key of ['product', 'character', 'world', 'moments', 'signature']) {
  if (!genesis.unslop_direction?.[key]) fail(`genesis direction ${key}`);
}
if (!Array.isArray(genesis.waves) || genesis.waves.length < 1) fail('genesis waves');
for (const wave of genesis.waves) {
  for (const key of ['wave_id', 'name', 'parallel_nodes', 'write_set_files', 'verification_oracle']) {
    if (wave[key] === undefined) fail(`wave missing ${key}`);
  }
}

if (contract.schema_version !== '4.0.0') fail('contract schema_version');
if (!dimensions.has(contract.target_dimension)) fail('contract dimension');
if (contract.atomic_scope_files.length < 1 || contract.atomic_scope_files.length > 5) fail('contract scope size');
if (!/^omni-loop\/iter-[0-9]+-pre$/.test(contract.rollback_tag)) fail('contract rollback tag');
if (contract.ponytail_rung_selected < 1 || contract.ponytail_rung_selected > 7) fail('ponytail rung');
for (const key of ['impact', 'severity', 'confidence', 'unlocks', 'info_gain', 'substructure_reward', 'effort', 'risk']) {
  if (typeof contract.uevam_inputs?.[key] !== 'number' && typeof contract.uevam_inputs?.[key] !== 'number') fail(`uevam ${key}`);
}

if (verdict.schema_version !== '4.0.0' || verdict.clean_room_cot_stripped !== true || verdict.anti_tamper_passed !== true) fail('verdict header');
for (const role of ['product_intent', 'design_visual_unslop', 'systems_analytical_ponytail', 'epistemic_citation_fidelity', 'adversarial_red_team_fuzzer', 'completeness_dod_gatekeeper']) {
  if (!verdict.six_role_verdicts?.[role]) fail(`verdict role ${role}`);
}
if (typeof verdict.score_delta?.net_gain !== 'number') fail('verdict score');

for (const key of ['schema_version', 'incumbent_commit', 'current_iteration', 'current_wave', 'universal_dimensions', 'next_recommended_action']) {
  if (state[key] === undefined) fail(`state missing ${key}`);
}
if (state.schema_version !== '4.0.0') fail('state schema_version');

if (process.exitCode) {
  console.error('OMNI-LOOP v4 records failed validation.');
} else {
  console.log('OMNI-LOOP v4 records match the required Appendix A and Appendix D fields.');
}
