import { appendFileSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const workingDirectory = process.cwd();
const actionDirectory = dirname(fileURLToPath(import.meta.url));
const localConfig = join(workingDirectory, '.jscpd.json');
const selectedConfig = process.env.CPD_CONFIG
  ? resolve(workingDirectory, process.env.CPD_CONFIG)
  : statSync(localConfig, { throwIfNoEntry: false })
    ? localConfig
    : resolve(actionDirectory, '../../../config/jscpd.json');
const config = JSON.parse(readFileSync(selectedConfig, 'utf8'));
const configDirectory = dirname(selectedConfig);
const configuredPaths = [config.path ?? []].flat();

// Upstream runs from the workspace root; keep paths anchored to their original base.
config.path = configuredPaths.length
  ? configuredPaths.map(path => resolve(configDirectory, path))
  : [workingDirectory];

for (const field of ['baseline', 'healthInput', 'health-input']) {
  if (config[field]) {
    config[field] = resolve(configDirectory, config[field]);
  }
}

for (const field of ['deadCode', 'dead-code', 'basta']) {
  if (config[field]?.rustDiagnostics) {
    config[field].rustDiagnostics = resolve(configDirectory, config[field].rustDiagnostics);
  }
}

const output = resolve(workingDirectory, config.output ?? 'report');
const configuredReporters = Array.isArray(config.reporters)
  ? config.reporters
  : (config.reporters ?? 'console').split(',');
const reporters = process.env.CPD_REPORTERS
  || [...new Set([...configuredReporters, 'ai'])].join(',');
const directory = mkdtempSync(join(process.env.RUNNER_TEMP, 'jscpd-config-'));
const preparedConfig = join(directory, 'config.json');
const installPrefix = join(directory, 'bin');
writeFileSync(preparedConfig, JSON.stringify(config));

for (const [name, value] of Object.entries({ config: preparedConfig, reporters, output, 'install-prefix': installPrefix })) {
  if (/[\r\n]/.test(value)) {
    throw new Error(`Invalid newline in JSCPD ${name}`);
  }

  appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${value}\n`);
}

appendFileSync(process.env.GITHUB_PATH, `${installPrefix}\n`);
console.log(`CPD configuration: ${selectedConfig}`);
