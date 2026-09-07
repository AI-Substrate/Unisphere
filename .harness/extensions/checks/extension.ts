import { defineExtension } from '@ai-substrate/engineering-harness/contract';

export default defineExtension({
  name: 'checks',
  summary: 'Unisphere product quality gate (not configured).',
  verbs: {
    'checks': {
      summary: 'Report the missing canonical product validation command.',
      run(ctx) {
        return ctx.unconfigured(
          'Unisphere has no canonical product validation command. Establish the product build/test lane, then wire that command into .harness/extensions/checks/extension.ts and rerun `harness checks --json`. Harness setup alone is not product proof.',
        );
      },
    },
  },
});
