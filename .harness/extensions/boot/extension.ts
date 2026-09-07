import { defineExtension } from '@ai-substrate/engineering-harness/contract';

export default defineExtension({
  name: 'boot',
  summary: 'Report Unisphere readiness through its product quality gate.',
  verbs: {
    'boot': {
      summary: 'Run checks without treating absent product proof as success.',
      async run(ctx) {
        if (!ctx.fs.exists('.harness/extensions/checks')) {
          return ctx.degraded(
            { ready: false },
            'No checks extension exists. Create it with `harness new checks` and wrap the canonical product validation command before claiming readiness.',
          );
        }
        const result = await ctx.exec('harness', ['checks', '--json'], { timeoutMs: 120_000 });
        if (result.code === 2) {
          return ctx.unconfigured(
            'Product readiness is unproven because `harness checks --json` is unconfigured. Establish the product build/test lane and wire it into .harness/extensions/checks/extension.ts; then configure the real product readiness/smoke command here and rerun `harness boot --json`.',
          );
        }
        if (!result.ok) {
          return ctx.error('E_CHECKS_FAILED', `harness checks failed (exit ${result.code})`, {
            details: { stdout: result.stdout, stderr: result.stderr },
            next_action: 'Run `harness checks --json`, fix the reported failure, then rerun `harness boot --json`.',
          });
        }
        return ctx.degraded(
          { ready: false, checks: JSON.parse(result.stdout) },
          'Checks completed, but no product startup or smoke command is configured. Wire the canonical readiness command into .harness/extensions/boot/extension.ts before claiming the telemetry collector runs.',
        );
      },
    },
  },
});
