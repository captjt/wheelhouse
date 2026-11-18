# Address round {{ROUND}} on {{KEY}}: {{TITLE}}

The PR is open: {{PR_URL}}. Since it opened, or since the last round, it got
what's in `.tmp/address/round-{{ROUND}}/open.json`:

- `threads`: review threads still open, each with its file, line and
  comments, oldest first. `comment_id` is the thread's first comment.
- `reviews`: review bodies, and `comments`: conversation comments, that
  nothing of {{LOGIN}}'s has answered yet. A comment with a `note` came from
  {{LOGIN}} with the go-ahead: act on it, but it gets no answer on GitHub.
- `checks`: CI checks that failed on the PR's head. The failing part of each
  GitHub Actions job's log is in `.tmp/address/round-{{ROUND}}/ci-*.log`
  when Wheelhouse could fetch it.
- `base_state`: `behind` or `conflicts` when {{BASE}} moved on, else null.
- `rerequest`: reviewers to ask for a review again once this is pushed.

Do this:

1. For each thread, review and comment: fix what's valid, with a test where
   the change has behavior. When a point doesn't hold, leave the code: the
   reply says why. A question gets a direct answer.
2. For each failing check: find the cause in its log and the code, fix it,
   and run that check locally until it passes.
3. {{UPDATE}}
4. Run the repo's checks (format, lint, tests) the way its docs say, and fix
   what fails. Commit on `{{BRANCH}}`: one commit per area, with Conventional
   Commit messages, such as `fix(api): address review on #{{PR}}`. Never
   commit `.tmp/`. Don't push.
5. Write the replies to `.tmp/address/round-{{ROUND}}/replies.json`.
   Wheelhouse posts them from {{LOGIN}}'s account, with a footer naming you,
   once {{LOGIN}} has read them and approved the push:

   ```json
   {
     "replies": [
       { "comment_id": 4393788824, "body": "Fixed in 78b6dd5. …" }
     ],
     "answers": [
       {
         "url": "https://github.com/acme/api/pull/45#pullrequestreview-5673328467",
         "label": "dana's review",
         "quote": "Optional: the sentence being answered",
         "body": "…"
       }
     ],
     "rerequest": ["dana"]
   }
   ```

   - `replies`: one per thread in open.json, on its `comment_id`.
   - `answers`: one per review body and comment in open.json, with its
     `url`. They post together as one conversation comment, each under a
     link to what it answers. Answer a review body's point in its thread
     instead when there is one, and say so here in a line.
   - `rerequest`: open.json's `rerequest`, less anyone whose points this
     round didn't address.
   - Every thread, review and comment gets an answer, including the ones
     that changed nothing. Wheelhouse refuses a draft that skips one.
6. Write `BUILT` to `.tmp/status.txt`, and stop.

## How the replies read

- Lead with the outcome and the commit: "Fixed in 1a2b3c4.", "Done in
  1a2b3c4.", "Agreed, and fixed in 1a2b3c4.", or "Kept as is:" with the
  reason. The commit is one of this round's on the branch, short form.
- Then the concrete change in the code's own terms (functions, fields,
  error text, in code spans) and the failure it prevents. Don't repeat the
  reviewer's comment back.
- Then the test that covers it, or the suite and its count.
- The footer says who wrote it, so never "I" or "we": write "the
  description now says so", not "I updated the description". Address the
  reviewer as "you". No thanks, apologies, or "great catch".
- One point, one answer: when another thread answers it, point there.
- Plain American English. Never mention local files, `.tmp/` paths, or
  anything a reviewer can't see.

Later prompts may bring the checker's findings or changes {{LOGIN}} asked
for. Each says what to do. Finish each by writing `BUILT` again.

{{BUILD_GUIDE}}
