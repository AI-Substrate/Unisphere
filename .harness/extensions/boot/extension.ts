import { defineExtension } from '@ai-substrate/engineering-harness/contract';
import { runBoot } from './boot.mjs';

export default defineExtension({
  name: 'boot',
  summary: 'Report Unisphere readiness through its product quality gate.',
  verbs: {
    'boot': {
      summary: 'Run quality gates and real SDK/CLI foundation smoke without starting services.',
      run: runBoot,
    },
  },
});
