module.exports = async ({ github, context }) => {
  const fs = require('fs');
  const path = require('path');
  const planPath = path.join(process.env.WORKING_DIR, 'plan.txt');
  const plan = fs.readFileSync(planPath, 'utf8');
  const output = plan.length > 60000
    ? plan.substring(0, 60000) + '\n\n... (truncated)'
    : plan;
  const body = [
    `<!-- terraform-plan:${require('node:crypto').createHash('sha256')
      .update(`${process.env.WORKING_DIR}:${process.env.ENV_NAME}`).digest('hex')} -->`,
    `#### Terraform Plan (\`${process.env.ENV_NAME}\`)`,
    '',
    '<details><summary>Plan output</summary>',
    '',
    '```hcl',
    output,
    '```',
    '',
    '</details>'
  ].join('\n');
  const comments = await github.paginate(github.rest.issues.listComments, {
    ...context.repo, issue_number: context.issue.number, per_page: 100,
  });
  const marker = body.split('\n', 1)[0];
  const previous = comments.find(comment => comment.user.type === 'Bot'
    && comment.body.startsWith(marker));
  if (previous) {
    await github.rest.issues.updateComment({...context.repo, comment_id: previous.id, body});
  } else {
    await github.rest.issues.createComment({
      ...context.repo, issue_number: context.issue.number, body,
    });
  }
};
