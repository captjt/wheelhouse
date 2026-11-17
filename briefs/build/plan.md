# Plan {{KEY}}: {{TITLE}}

You're the builder. Read what's asked, decide whether it can be built as
written, and plan it. Don't write code yet.

1. Read `.tmp/context/intent.md`, `.tmp/context/issue.md` if it's there, and
   `.tmp/context/repo-notes.md` if it's there. Then read the code it touches,
   and the repo's guidance for it.
2. **Is it ready?** It can't be built as written when it has no clear goal,
   or when it would need more than {{MAX_DECISIONS}} decisions open at once.
   Then write `.tmp/not-ready.md`: what's missing, the questions it should
   answer, and the assumptions you'd plan on if told to proceed. Write
   `NOT_READY: <one line>` to `.tmp/status.txt`, and stop.
3. **Plan.** Write `.tmp/plan.md` in this format:

{{PLAN_FORMAT}}

   {{PLAN_MODE}}

   Its Intent, Acceptance and Out of scope become the docket's intent when
   {{LOGIN}} approves the plan, so write them as the spec of what's done.
4. **Decisions.** Raise each decision the plan depends on as
   `.tmp/decisions/D<n>.json` (see the ground rules), and list each in the
   plan's Decisions section by id, with your recommendation. Raise them all
   now, together, not one at a time later.
5. Before you finish, read any `.tmp/decisions/*.answer.json` that came in
   while you worked, and make the plan follow them.
6. Write `PLAN_READY` to `.tmp/status.txt`, and stop. {{LOGIN}} reads the
   plan and approves it, asks for changes, or discards it. Their answer
   comes to you as a prompt.

{{PLAN_GUIDE}}
