```markdown
# Plan: KEY Title

## Intent
One short paragraph: what changes, for whom, and why.

## Acceptance
- One item for each thing that must be true when this is done, with how it's
  shown: a test (`webhooks::retry::backs_off`) or a probe.

## Decisions
- D1: Should retries cover 4xx responses? Recommended: 5xx and timeouts only.

## Approach
How you'll build it, in a few steps: the files and functions you'll change.
This is for the person approving the plan; it doesn't go on the PR.

## Out of scope
- What a reader might expect that this leaves out, and where it goes instead.

## Notes
- Assumptions, and choices you made without asking because they have a
  conventional answer, each with why.
```

Intent, Acceptance, Out of scope, and Notes go on the PR as its plan, and
Wheelhouse writes the Decisions there itself, with who decided each one.
Write them for the PR's reviewers: plain sentences, no first person.
