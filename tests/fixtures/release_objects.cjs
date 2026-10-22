// Caller configuration exercises the supported Node plugin/writer object API.
const fs = require('node:fs');
const settings = JSON.parse(fs.readFileSync(process.env.CI_RELEASE_SETTINGS, 'utf8'));
const [bridge, options] = settings.plugins[0];
module.exports = {
  repositoryUrl: process.env.CI_RELEASE_TEST_REMOTE,
  plugins: [{
    async analyzeCommits(_, context) {
      return (await import(bridge)).analyzeCommits(options, context);
    },
    async generateNotes(_, context) {
      const metadata = { label() { return 'original-object'; } };
      const commits = context.commits.map(commit => ({
        ...commit,
        fixtureDate: new Date('2026-01-02T00:00:00.000Z'),
        fixtureMetadata: metadata,
        fixturePayload: 'x'.repeat(11 * 1024 * 1024),
      }));
      const notes = {
        ...options.notes,
        writerOpts: {
          transform(commit) {
            return {
              ...commit,
              subject: `date=${commit.fixtureDate instanceof Date};method=${typeof commit.fixtureMetadata.label};payload=${commit.fixturePayload.length}`,
            };
          },
        },
      };
      return (await import(bridge)).generateNotes({...options, notes}, {...context, commits});
    },
  }],
};
