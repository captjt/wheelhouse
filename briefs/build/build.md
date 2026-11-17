# Build {{KEY}}: {{TITLE}}

The plan in `.tmp/plan.md` is approved, and each decision has an answer in
`.tmp/decisions/`. Build it.

1. Implement the plan, following each decision's answer. If the plan turns
   out wrong about something that matters, change course, say so in
   `.tmp/report.md`, and raise a decision if it's one.
2. Add or update tests so each Acceptance item has evidence: a test, or a
   probe whose output you keep in `.tmp/report.md`. Run the repo's checks
   (format, lint, tests) the way its docs say, and fix what fails.
3. Commit on `{{BRANCH}}` as you go: small commits, with Conventional Commit
   messages that match the repo's history (`git log --oneline -20`). Never
   commit `.tmp/`. Don't push.
4. Write `.tmp/pr.json`, the PR's draft:

   ```json
   { "title": "…", "body": "…" }
   ```

   How it reads: {{PR_STYLE}}

   Wheelhouse adds "{{CLOSES}}", the plan, and the footer, so leave those
   out of the body.
5. If the change is something a person can see, make a preview:
   - **An app or a page:** start it in the background on port {{PORT}},
     bound to 127.0.0.1, and leave it running. Then write
     `.tmp/preview.json`:
     `{ "port": {{PORT}}, "title": "The new retry settings", "ask": "Does the error state read right?", "what": "Where to look, and what changed." }`
   - **Anything else worth a look,** such as CLI output: write
     `.tmp/preview.json` with no port, and `what` holding a short transcript
     or the command that shows it.
6. Write `.tmp/report.md` for the checker: what you built, how each
   Acceptance item is shown (commands and their output), what you couldn't
   check, and anything you chose without asking, with why.
7. Write `BUILT` to `.tmp/status.txt`, and stop.

Later prompts may bring a decision's answer, the checker's findings, or
changes {{LOGIN}} asked for. Each says what to do. Finish each by writing
`BUILT` again.

{{BUILD_GUIDE}}
