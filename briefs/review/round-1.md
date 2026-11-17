# Round {{ROUND}} of {{REPO}} PR #{{PR}}: {{TITLE}}

- Author: {{AUTHOR}}
- Head: {{HEAD}} (this worktree), against `{{BASE}}`
- Context, fetched for this round: `{{CONTEXT}}/`. It holds `pr.json` (body,
  files, commits), `pr.diff`, `reviews.json`, `review-comments.json`,
  `issue-comments.json`, `threads.json`, and `ci.json`. When the PR
  addresses a Linear issue, `linear.md` holds it: what it asks for and its
  acceptance criteria.
- Your output folder: `{{OUT}}/`

1. Read `.tmp/ground-rules.md`. Then read the PR body, `linear.md` if it's
   there, the diff, and the repo's guidance for the code it touches. Hold
   the PR to what its issue asks of this PR, and report a gap as a finding.
2. Review the change. {{REVIEW_GUIDE}}
3. Check each candidate finding yourself. Reproduce it with a probe or a test
   where you can, and keep probes in `.tmp/probes/`. Drop anything you can't
   back with evidence.
4. Read the other reviewers' comments, so you don't repeat them.
5. Write `{{OUT}}/review.json` and `{{OUT}}/report.md` as the ground rules
   describe.
6. Write `READY_TO_POST` to `.tmp/status.txt`, then stop.
