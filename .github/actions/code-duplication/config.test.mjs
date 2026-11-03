import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const script = new URL('./config.mjs', import.meta.url);

function configurationRun(t, files = {}, inputs = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'jscpd action ')));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const project = join(root, 'consumer project');
  mkdirSync(project);

  for (const [file, config] of Object.entries(files)) {
    writeFileSync(join(project, file), JSON.stringify(config));
  }

  const outputFile = join(root, 'outputs');
  const pathFile = join(root, 'paths');
  const result = spawnSync(process.execPath, [fileURLToPath(script)], {
    cwd: project,
    env: { ...process.env, CPD_CONFIG: '', CPD_REPORTERS: '', RUNNER_TEMP: root,
      GITHUB_OUTPUT: outputFile, GITHUB_PATH: pathFile, ...inputs },
    encoding: 'utf8',
  });

  if (result.status !== 0) {
    return { result };
  }

  const outputs = Object.fromEntries(readFileSync(outputFile, 'utf8').trim().split('\n')
    .map(line => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)]));
  return { result, outputs, project, config: JSON.parse(readFileSync(outputs.config, 'utf8')),
    installedPath: readFileSync(pathFile, 'utf8').trim() };
}

test('shared defaults scan only the consumer working directory', t => {
  const { result, outputs, project, config, installedPath } = configurationRun(t);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(config.path, [project]);
  assert.equal(config.threshold, 5);
  assert.equal(config.failOnEmpty, true);
  assert.equal(outputs.reporters, 'threshold,ai');
  assert.equal(outputs.output, join(project, 'report'));
  assert.equal(installedPath, outputs['install-prefix']);
});

test('local paths, exclusions, baseline and report location survive config relocation', t => {
  const local = { path: ['source files'], ignore: ['**/generated/**'], baseline: 'baseline.json',
    output: 'custom reports', reporters: ['console'], threshold: 0 };
  const { result, outputs, project, config } = configurationRun(t, { '.jscpd.json': local });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(config.path, [join(project, 'source files')]);
  assert.deepEqual(config.ignore, local.ignore);
  assert.equal(config.baseline, join(project, 'baseline.json'));
  assert.equal(config.threshold, 0);
  assert.equal(outputs.output, join(project, 'custom reports'));
  assert.equal(outputs.reporters, 'console,ai');
});

test('explicit config and reporters override the local configuration', t => {
  const files = { '.jscpd.json': { threshold: 0 }, 'explicit.json': { threshold: 20 } };
  const { result, outputs, config } = configurationRun(t, files,
    { CPD_CONFIG: 'explicit.json', CPD_REPORTERS: 'silent' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(config.threshold, 20);
  assert.equal(outputs.reporters, 'silent');
});

test('missing explicit config fails instead of using a local config', t => {
  const { result } = configurationRun(t, { '.jscpd.json': {} }, { CPD_CONFIG: 'missing.json' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing\.json/);
});
