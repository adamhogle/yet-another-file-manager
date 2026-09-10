import pluginVue from 'eslint-plugin-vue';
import {
  configureVueProject,
  defineConfigWithVueTs,
  vueTsConfigs
} from '@vue/eslint-config-typescript';
import eslintConfigPrettier from 'eslint-config-prettier/flat';

// defineConfigWithVueTs injects the vue/block-lang rule (allowNoLang is tied to the
// scriptLangs contents): scriptLangs: ['ts'] keeps the rule active and rejects non-TS
// lang attributes, so App.vue's script block must stay lang="ts".
configureVueProject({ scriptLangs: ['ts'] });

export default defineConfigWithVueTs(
  { name: 'app/files-to-lint', files: ['frontend/src/**/*.{ts,vue}', 'tests/**/*.ts'] },
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  eslintConfigPrettier
);
