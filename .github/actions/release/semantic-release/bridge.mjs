// Required adapter: semantic-release and caller plugins expose a Node API.
import {readFileSync, writeFileSync} from "node:fs";
import {createRequire} from "node:module";
import {pathToFileURL} from "node:url";
import semanticRelease from "semantic-release";

const require = createRequire(import.meta.url);
const {configFile, extraPlugins, ...defaults} = JSON.parse(readFileSync(process.env.CI_RELEASE_SETTINGS, "utf8"));
const config = configFile ? (await import(pathToFileURL(configFile).href)).default : {};
const settings = {
  ...config,
  plugins: [...(config.plugins || defaults.plugins).map(plugin => {
    const [name, options] = Array.isArray(plugin) ? plugin : [plugin];
    return [require.resolve(name), options];
  }), ...extraPlugins.map(name => require.resolve(name))],
  branches: defaults.branches,
  tagFormat: defaults.tagFormat,
  dryRun: process.env.CI_RELEASE_DRY_RUN === "true",
};
const result = await semanticRelease(settings, {cwd: process.cwd()});
writeFileSync(process.env.CI_RELEASE_RESULT, JSON.stringify(result || null));
