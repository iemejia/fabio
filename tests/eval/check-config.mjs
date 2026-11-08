// Validate the authored config without credentials, model calls, or tenant data.
// Usage: node tests/eval/check-config.mjs "$(npm root -g)"
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { runInNewContext } from 'node:vm';

const require = createRequire(join(resolve(process.argv[2]), 'promptfoo', 'package.json'));
const yaml = require('js-yaml');
const config = yaml.load(readFileSync(new URL('./promptfooconfig.yaml', import.meta.url), 'utf8'));
assert.equal(config.providers.length, 1);
assert.equal(config.providers[0], config.defaultTest.options.provider);
assert.equal(config.providers[0].config.apiKeyEnvar, 'AZURE_OPENAI_API_KEY');
assert.equal(config.providers[0].config.apiBaseUrl, '{{env.AZURE_OPENAI_API_BASE_URL}}');
assert.equal(config.providers[0].config.modelName, '{{env.AZURE_OPENAI_MODEL_NAME}}');
assert.equal(config.providers[0].id, 'azureopenai:chat:{{env.AZURE_OPENAI_DEPLOYMENT_NAME}}');
assert.equal(config.providers[0].config.omitDefaults, true);
assert.equal(config.providers[0].config.reasoning_effort, undefined);

function evaluate(code, output) {
  // Match promptfoo's expression-vs-function-body convention, including its
  // trimEnd behavior for YAML block scalars. Compilation alone misses an
  // expression spread across lines that forgets to return a boolean.
  const source = code.trimEnd();
  const body = source.includes('\n') ? source : `return (${source});`;
  return runInNewContext(`(function(output, context) { ${body}\n})(output, context)`,
    { output, context: { vars: {} } }, { timeout: 100 });
}

let count = 0;
for (const test of config.tests) {
  for (const assertion of test.assert ?? []) {
    if (assertion.type !== 'javascript') continue;
    for (const output of ['', 'fabio example --workspace "$WS" --dry-run']) {
      const result = evaluate(assertion.value, output);
      assert.ok(typeof result === 'boolean' || (typeof result === 'number' && Number.isFinite(result)) ||
        (result && typeof result.pass === 'boolean' && Number.isFinite(result.score)),
        `Non-boolean/non-finite JavaScript result: ${test.description}`);
    }
    count++;
  }
}

function check(description, output, expected) {
  const test = config.tests.find(test => test.description === description);
  assert.ok(test, `Missing case: ${description}`);
  const assertion = test.assert.filter(assertion => assertion.type === 'javascript').at(-1);
  assert.equal(evaluate(assertion.value, output), expected);
}

check("Disambiguate: 'materialized view' with no workload context", 'Lakehouse or KQL database?', true);
check("Disambiguate: 'materialized view' with no workload context", 'Use Snowflake.', false);
check('SQL endpoint: selectively refresh table metadata', '--timeout \'{"timeUnit": "Minutes", "value": 10}\'', true);
check('SQL endpoint: selectively refresh table metadata', '--timeout \'{"timeUnit": "Hours", "value": 10}\'', false);
check('Remediation hints: confirmed block yields a guarded preview only', 'fabio context describe capacity delete-blocked-actor', true);
check('Remediation hints: confirmed block yields a guarded preview only', 'I will remove the block now.', false);
check('Git: inspect and commit one changed item file',
  'fabio git status --include-files-details\nfabio git commit --file-selection $ITEM=metadata.json', true);
check('Git: inspect and commit one changed item file',
  'fabio git commit --file-selection $ITEM=metadata.json\nfabio git status --include-files-details', false);
check('Create and enable workspace Monitoring',
  'fabio monitoring create --file MonitoringProperties.json\nfabio monitoring enable-data-collection', true);
check('Create and enable workspace Monitoring', 'fabio monitoring create --file MonitoringProperties.json', false);
check('CI/CD: repair obsolete deployment-plan definition content', 'fabio context schema DeploymentPlan', true);
check('CI/CD: repair obsolete deployment-plan definition content',
  '$schema: https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json\nversion: 1.0.0', true);
check('CI/CD: repair obsolete deployment-plan definition content', 'executableFormat: old\nid: old', false);
check('Spark diagnostics: session-only triage never invents or publishes an environment',
  'fabio environment diagnostics --workspace WS --item-type notebook --item-id NB --livy-id LIVY', true);
check('Spark diagnostics: session-only triage never invents or publishes an environment',
  'fabio environment diagnostics --workspace WS --item-type Notebook --item-id NB --livy-id LIVY', false);
check('Admin hygiene: explicit IDs are bounded admin reads, not a workspace scan',
  'fabio --readonly admin connection-hygiene --id CONN_A --id CONN_B --connection-limit 2 --page-limit 1 --role-limit 10 --timeout 30', true);
check('Admin hygiene: explicit IDs are bounded admin reads, not a workspace scan',
  'fabio context describe admin connection-hygiene', true);
check('Admin hygiene: explicit IDs are bounded admin reads, not a workspace scan',
  'fabio admin connection-hygiene --inventory', false);
check('Admin hygiene: explicit IDs are bounded admin reads, not a workspace scan',
  'fabio --readonly admin connection-hygiene --id "$CONN_A" --id "${CONN_B}" --connection-limit 2 --page-limit 1 --role-limit 10 --timeout 30', true);
const explicitScope = config.tests.find(test => test.description === 'Admin hygiene: explicit IDs are bounded admin reads, not a workspace scan');
const workspaceScopeAssertion = explicitScope.assert.find(assertion => assertion.type === 'javascript');
assert.equal(evaluate(workspaceScopeAssertion.value,
  'fabio --readonly admin connection-hygiene --id CONN_A\nNo --workspace flag is accepted.'), true);
assert.equal(evaluate(workspaceScopeAssertion.value,
  'fabio --readonly admin connection-hygiene \\\n  --id CONN_A --workspace WS'), false);
assert.equal(evaluate(workspaceScopeAssertion.value,
  'fabio admin connection-hygiene --id CONN_A'), false);
assert.equal(evaluate(workspaceScopeAssertion.value,
  'fabio context describe admin connection-hygiene'), true);
console.log(`Validated ${count} JavaScript assertions across ${config.tests.length} cases and the shared Foundry provider.`);
