import assert from 'node:assert/strict';
import test from 'node:test';
import { APPROVED, boundedOutput, observeVersion, runChecks } from './checks.mjs';

const versions = [
  'rustc 1.95.0 (59807616e 2026-04-14)\nrelease: 1.95.0\ncommit-hash: 59807616e1fa2540724bfbac14d7976d7e4a3860\n',
  'cargo 1.95.0 (f2d3ce0bd 2026-03-21)\nrelease: 1.95.0\ncommit-hash: f2d3ce0bd7f24a49f8f72d9000448f8838c4e850\n',
  'clippy 0.1.95 (59807616e1 2026-04-14)\n',
  'rustfmt 1.9.0-stable (59807616e1 2026-04-14)\n',
];
const locate = name => ({ invoked: `/fixture/bin/${name}`, resolved: `/fixture/toolchain/${name}` });
function context(run) {
  return {
    exec: run,
    ok: data => ({ status: 'ok', data }),
    error: (code, message, extra) => ({ status: 'error', error: { code, message, ...extra } }),
  };
}

test('release and commit identity ignore distribution wording and installation path', () => {
  for (const [index, name] of Object.keys(APPROVED).entries()) assert.equal(observeVersion(name, versions[index]).approved, true);
  assert.equal(observeVersion('rustfmt', 'rustfmt 1.9.0 (59807616e1 2026-04-14)').approved, true);
  assert.equal(observeVersion('rustc', versions[0].replace('1.95.0', '1.94.0').replace('release: 1.95.0', 'release: 1.94.0')).approved, false);
  assert.equal(observeVersion('clippy', 'clippy 0.1.95 (1111111111 2026-04-14)').approved, false);
});

test('missing formatter commit metadata is named, not invented as a match', () => {
  const observed = observeVersion('rustfmt', 'rustfmt 1.9.0\n');
  assert.equal(observed.release_matches, true);
  assert.equal(observed.commit, null);
  assert.equal(observed.commit_matches, null);
  assert.equal(observed.approved, true);
  assert.match(observed.commit_metadata, /not emitted/);
});

test('mixed or missing tool identity records all tools and prevents quality gates', async () => {
  let calls = 0;
  const result = await runChecks(context(async () => {
    const index = calls++;
    return { ok: true, code: 0, stdout: index === 3 ? 'rustfmt 1.8.0\n' : versions[index], stderr: '' };
  }), locate);
  assert.equal(calls, 4);
  assert.equal(result.error.code, 'E_TOOLCHAIN_PREREQUISITE');
  assert.equal(Object.keys(result.error.details.toolchain).length, 4);
  assert.equal(result.error.details.toolchain.rustfmt.commit, null);
  assert.match(result.error.next_action, /coherent/);
});

test('unavailable formatter commit permits gates while retaining an explicit provenance gap', async () => {
  let calls = 0;
  const result = await runChecks(context(async () => {
    const index = calls++;
    return { ok: true, code: 0, stdout: index === 3 ? 'rustfmt 1.9.0\n' : versions[index] ?? 'passed', stderr: '' };
  }), locate);
  assert.equal(result.status, 'ok');
  assert.equal(result.data.toolchain.rustfmt.commit_matches, null);
  assert.equal(result.data.provenance_gaps.length, 1);
  assert.match(result.data.toolchain.rustfmt.optional_next_command, /already installed/);
  assert.equal(result.data.gates.length, 6);
});

test('failing product check preserves child evidence and stops later gates', async () => {
  const calls = [];
  const result = await runChecks(context(async (command, args) => {
    calls.push([command, args]);
    const index = calls.length - 1;
    if (index < 4) return { ok: true, code: 0, stdout: versions[index], stderr: '' };
    return { ok: false, code: 17, stdout: 'product-output', stderr: 'product-failure' };
  }), locate);
  assert.equal(calls.length, 5);
  assert.equal(result.error.code, 'E_PRODUCT_CHECK_FAILED');
  assert.deepEqual(result.error.details.gates[0].args, ['fmt', '--all', '--check']);
  assert.equal(result.error.details.gates[0].code, 17);
  assert.equal(result.error.details.gates[0].stdout, 'product-output');
  assert.equal(result.error.details.gates[0].stderr, 'product-failure');
});

test('all gates run once after coherent observations without boot recursion', async () => {
  const calls = [];
  const result = await runChecks(context(async (command, args) => {
    const index = calls.length;
    calls.push([command, args]);
    return { ok: true, code: 0, stdout: index < 4 ? versions[index] : 'passed', stderr: '' };
  }), locate);
  assert.equal(result.status, 'ok');
  assert.deepEqual(result.data.gates.map(gate => gate.name), ['format', 'clippy', 'tests', 'rustdoc', 'architecture', 'harness-regression']);
  assert.ok(calls.every(([, args]) => !args.includes('boot')));
});

test('passing gates keep a bounded output tail; the envelope stays small', () => {
  const long = 'x'.repeat(10_000) + 'END';
  const bounded = boundedOutput({ ok: true, code: 0, stdout: long, stderr: '' });
  assert.equal(bounded.stdout.length, 2_000);
  assert.ok(bounded.stdout.endsWith('END'));
  assert.equal(bounded.stdout_bytes, long.length);
  assert.equal(bounded.stderr, '');
});
