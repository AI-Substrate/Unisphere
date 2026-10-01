import { accessSync, constants, realpathSync } from 'node:fs';
import { delimiter, isAbsolute, resolve } from 'node:path';

export const APPROVED = Object.freeze({
  rustc: { release: '1.95.0', commit: '59807616e1fa2540724bfbac14d7976d7e4a3860' },
  cargo: { release: '1.95.0', commit: 'f2d3ce0bd7f24a49f8f72d9000448f8838c4e850' },
  clippy: { release: '0.1.95', commit: '59807616e1' },
  rustfmt: { release: '1.9.0', commit: '59807616e1' },
});

const remediation = 'Select the approved coherent Rust 1.95.0 toolchain for this command (including matching clippy/rustfmt), then rerun harness checks --json. Inspect version evidence; rust-toolchain.toml alone does not enforce PATH. No automatic install or global toolchain change was made.';

export function resolveProgram(name) {
  const suffixes = process.platform === 'win32' ? ['', '.exe', '.cmd'] : [''];
  for (const directory of (process.env.PATH ?? '').split(delimiter)) {
    if (!directory) continue;
    for (const suffix of suffixes) {
      const path = isAbsolute(name) ? name : resolve(directory, name + suffix);
      try { accessSync(path, constants.X_OK); return { invoked: path, resolved: realpathSync(path) }; } catch { /* Try the next PATH entry. */ }
    }
  }
  throw new Error(`Executable ${name} is absent from PATH`);
}

const OUTPUT_TAIL = 2_000;

/** A passing result with stdout/stderr cut to their last OUTPUT_TAIL characters. */
export function boundedOutput(result) {
  const tail = text => (text.length > OUTPUT_TAIL ? text.slice(-OUTPUT_TAIL) : text);
  return {
    ...result,
    stdout: tail(result.stdout ?? ''),
    stderr: tail(result.stderr ?? ''),
    stdout_bytes: (result.stdout ?? '').length,
    stderr_bytes: (result.stderr ?? '').length,
  };
}

export function observeVersion(name, stdout) {
  const release = stdout.match(/^release:\s*(\S+)/m)?.[1]
    ?? stdout.match(/^(?:rustc|cargo|clippy|rustfmt)\s+(\S+)/m)?.[1];
  const normalized = release?.replace(/-stable$/, '');
  const commit = stdout.match(/^commit-hash:\s*([a-f0-9]{7,40})\b/m)?.[1]
    ?? stdout.match(/\(([a-f0-9]{7,40})(?:\s|\))/)?.[1]
    ?? null;
  const expected = APPROVED[name];
  const commitMatches = commit !== null && (expected.commit.startsWith(commit) || commit.startsWith(expected.commit));
  return {
    release: release ?? null,
    normalized_release: normalized ?? null,
    commit,
    commit_metadata: commit === null ? 'not emitted; identity unverified' : 'emitted',
    release_matches: normalized === expected.release,
    commit_matches: commit === null ? null : commitMatches,
    approved: normalized === expected.release && (commitMatches || (name === 'rustfmt' && commit === null)),
  };
}

export async function runChecks(ctx, locate = resolveProgram) {
  const toolchain = {};
  const probes = [
    ['rustc', 'rustc', ['-vV']],
    ['cargo', 'cargo', ['-vV']],
    ['clippy', 'cargo', ['clippy', '--version']],
    ['rustfmt', 'cargo', ['fmt', '--version']],
  ];
  for (const [name, executable, args] of probes) {
    try {
      const provenance = locate(executable);
      const result = await ctx.exec(provenance.invoked, args, { timeoutMs: 15_000 });
      toolchain[name] = { ...observeVersion(name, result.stdout), provenance, args, code: result.code, stdout: result.stdout, stderr: result.stderr };
      if (name === 'rustfmt' && toolchain[name].commit === null) {
        toolchain[name].warning = 'Formatter commit metadata was not emitted; only its release is observed. No formatter hash match is claimed.';
        toolchain[name].optional_next_command = 'rustup run 1.95.0 rustfmt --version (if that equivalent toolchain is already installed)';
      }
      if (name === 'clippy' || name === 'rustfmt') {
        // Record Cargo's subcommand and worker provenance separately from the
        // release/commit that the actual subcommand reported.
        toolchain[name].binaries = Object.fromEntries((name === 'clippy' ? ['cargo-clippy', 'clippy-driver'] : ['cargo-fmt', 'rustfmt']).map(binary => {
          try { return [binary, locate(binary)]; } catch (error) { return [binary, { unavailable: error.message }]; }
        }));
      }
      if (!result.ok) toolchain[name].approved = false;
    } catch (error) {
      toolchain[name] = { approved: false, commit: null, commit_metadata: 'probe unavailable', error: error.message };
    }
  }
  if (Object.values(toolchain).some(tool => !tool.approved)) {
    return ctx.error('E_TOOLCHAIN_PREREQUISITE', 'The observed Rust tools do not prove the approved coherent release/commit tuple.', {
      details: { toolchain, approved: APPROVED }, next_action: remediation,
    });
  }
  const cargo = toolchain.cargo.provenance.invoked;
  const gates = [
    ['format', cargo, ['fmt', '--all', '--check']],
    ['clippy', cargo, ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']],
    ['tests', cargo, ['test', '--workspace', '--all-targets', '--locked']],
    ['rustdoc', cargo, ['test', '--workspace', '--doc', '--locked']],
    ['architecture', cargo, ['run', '--locked', '-p', 'unisphere-testkit', '--bin', 'unisphere-arch-check']],
    ['harness-regression', process.execPath, ['--test', '.harness/extensions/checks/checks.test.mjs', '.harness/extensions/boot/extension.test.mjs']],
  ];
  const evidence = [];
  for (const [name, command, args] of gates) {
    try {
      const result = await ctx.exec(command, args, { timeoutMs: 300_000 });
      // A passing gate keeps a bounded tail: full test logs pushed the envelope
      // past 64 KiB, which older harness runtimes could not parse. Failures
      // keep their complete output.
      evidence.push({ name, command, args, ...(result.ok ? boundedOutput(result) : result) });
      if (!result.ok) {
        return ctx.error('E_PRODUCT_CHECK_FAILED', `${name} failed (exit ${result.code})`, {
          details: { toolchain, gates: evidence }, next_action: `Run ${command} ${args.join(' ')}, fix the failure, then rerun harness checks --json.`,
        });
      }
    } catch (error) {
      return ctx.error('E_PRODUCT_CHECK_EXEC', `${name} could not execute: ${error.message}`, {
        details: { toolchain, gates: evidence }, next_action: 'Repair the named command or timeout, then rerun harness checks --json.',
      });
    }
  }
  return ctx.ok({ scope: 'configuration-and-claude-jsonl', toolchain, provenance_gaps: Object.values(toolchain).filter(tool => tool.warning).map(tool => tool.warning), gates: evidence });
}
