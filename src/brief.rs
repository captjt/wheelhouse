//! The brief: what an agent reads to work on a docket. The intent, the
//! Linear issue, what was said and done lately, and how to write back.

use anyhow::{Result, anyhow};

use crate::hub::Core;

/// `me`: the pane reading it, so the agents on the docket are the others.
pub fn brief(core: &Core, id: &str, me: Option<&str>) -> Result<String> {
    let st = core.store.state(id)?.ok_or_else(|| anyhow!("no docket {id}"))?;
    let mut out = format!("# {id}: {}\n\n", st.title);
    let mut meta = vec![format!("Status: {}", st.status.replace('_', " "))];
    if let Some(l) = &st.linear {
        meta.push(format!(
            "Linear: {}{}{}",
            l.key,
            l.state.as_deref().map(|s| format!(" ({s})")).unwrap_or_default(),
            l.url.as_deref().map(|u| format!(" {u}")).unwrap_or_default()
        ));
        if let Some(b) = &l.branch {
            meta.push(format!("Branch: {b}"));
        }
    }
    out.push_str(&meta.join(" · "));
    out.push_str("\n\n## Intent\n\n");
    match &st.intent {
        Some(t) => {
            out.push_str(t.body.trim());
            out.push_str(&format!("\n\n(version {}, from {})\n", st.intent_versions, t.source));
        }
        None => out.push_str("None yet. If the work's goal is clear, write it: what must be true when it's done, and what's out of scope.\n"),
    }
    out.push_str(&agents_on(core, &st, me));
    let entries = core.store.entries(id)?;
    let read: Vec<crate::record::Entry> = entries.iter().filter_map(|s| s.entry().ok()).collect();
    let ds = crate::decisions::of(&read);
    if !ds.is_empty() {
        out.push_str("\n## Decisions in force\n\nWhat you were asked, and what stands. Don't ask again; work from these.\n\n");
        out.push_str(&crate::decisions::brief(&ds));
    }
    // Verify: what the hub will run, how it went, and whether it's held.
    let numbered: Vec<(i64, crate::record::Entry)> = entries.iter().filter_map(|s| Some((s.seq, s.entry().ok()?))).collect();
    let v = crate::evidence::view(core, &st, &numbered);
    let configured = v["configured"].as_array().cloned().unwrap_or_default();
    if !configured.is_empty() || st.hold.is_some() {
        out.push_str(
            "
## Verify

",
        );
        if let Some(h) = &st.hold {
            out.push_str(&format!(
                "On hold: {h}. Wheelhouse takes no steps on it until it's resumed.

"
            ));
        }
        if !configured.is_empty() {
            out.push_str(
                "Wheelhouse runs these checks itself, at the commit you hand over, and the PR waits for the required ones. Run the same commands before you say you're done:

",
            );
            for c in &configured {
                let name = c["name"].as_str().unwrap_or("");
                let last = st
                    .checks
                    .get(name)
                    .map(|r| format!(" Last: {} at {}.", if r.ok { "passed" } else { "failed" }, r.head.chars().take(7).collect::<String>()))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "- `{name}`: `{}`{}.{last}
",
                    c["run"].as_str().unwrap_or(""),
                    if c["required"] == false { " (optional)" } else { "" }
                ));
            }
        }
        if v["needs"] == true {
            out.push_str(
                "
Its intent has acceptance items, so it closes on evidence (passing checks, a merged PR), not on what anyone says.
",
            );
        }
    }
    // The repo's notes: its rules, and the decisions that stand for it.
    if let Some(notes) = v["repo"].as_str().and_then(|r| core.build.notes(r).map(|n| (r, n))) {
        out.push_str(&format!("\n## Notes on {}\n\n{}\n", notes.0, notes.1.replace("## Standing decisions", "### Standing decisions")));
    }
    let mut recent = Vec::new();
    for s in entries.iter().rev() {
        let Ok(e) = s.entry() else { continue };
        let line = match (e.kind.as_str(), e.data["via"].as_str(), e.data["what"].as_str()) {
            ("note", Some("prompt"), _) => format!("You asked: {}", one_line(e.body.as_deref())),
            ("note", _, _) => format!("Note ({}): {}", e.actor.id, one_line(e.body.as_deref())),
            ("turn", _, _) if e.body.is_some() => format!("{}: {}", who(&st, &e), e.data["summary"].as_str().map(str::to_string).unwrap_or_else(|| one_line(e.body.as_deref()))),
            ("observed", _, Some("linear-comment")) => format!("{} in Linear: {}", e.data["author"].as_str().unwrap_or("someone"), one_line(e.body.as_deref())),
            ("observed", _, Some("github-feedback")) => format!("{} on GitHub: {}", e.data["summary"].as_str().unwrap_or("someone"), one_line(e.body.as_deref())),
            ("approval", _, _) => format!("You {}: {}", if e.data["decision"] == "approve" { "approved" } else { "denied" }, one_line(e.body.as_deref())),
            _ => continue,
        };
        recent.push(line);
        if recent.len() == 8 {
            break;
        }
    }
    if !recent.is_empty() {
        out.push_str("\n## Lately (newest first)\n\n");
        for l in recent {
            out.push_str(&format!("- {l}\n"));
        }
    }
    out.push_str(&format!(
        "\n## Writing back\n\n- A note on the record: `wh note {id} \"...\"`\n- When the goal or scope changes, update the intent: `wh intent {id} - < intent.md`\n- The whole record: `wh show {id}`\n"
    ));
    Ok(out)
}

/// The agents on the docket besides `me`, and how to reach them. Nothing
/// when it's only you.
fn agents_on(core: &Core, st: &crate::record::fold::DocketState, me: Option<&str>) -> String {
    let handle = core.team.as_ref().map(|t| t.config.handle.clone());
    let mut others: Vec<String> = st
        .own_agents_on(handle.as_deref())
        .filter(|a| Some(a.agent.block.as_str()) != me && crate::agents::docket_of(core, &a.agent.block).as_deref() == Some(st.id.as_str()))
        .map(|a| {
            let addr = core.mirror.panes.get(&a.agent.block).map(|p| crate::messages::addr_of(core, p)).unwrap_or_else(|| a.agent.block.clone());
            let state = crate::agents::live(core, &a.agent.block).map(|l| l.state).unwrap_or_else(|| "not seen".into());
            let role = a.role.as_deref().map(|r| format!(", {r}")).unwrap_or_default();
            format!("- `{addr}`: {}{role}, in {} ({state})\n", crate::mirror::app_name(&a.agent.app), crate::runtime::of(&a.agent.block).title())
        })
        .collect();
    // A shared docket's other members' agents, on their own Macs.
    let teammates: Vec<String> = st
        .agents_on()
        .filter(|a| a.member.is_some() && a.member != handle)
        .map(|a| {
            let m = a.member.as_deref().unwrap_or("?");
            let pane = if a.agent.pane.is_empty() { a.agent.app.as_str() } else { a.agent.pane.as_str() };
            let role = a.role.as_deref().map(|r| format!(", {r}")).unwrap_or_default();
            format!("- `{m}/{pane}`: {}{role}, {m}'s, on {m}'s Mac\n", crate::mirror::app_name(&a.agent.app))
        })
        .collect();
    let has_teammates = !teammates.is_empty();
    others.extend(teammates);
    if others.is_empty() {
        return String::new();
    }
    let id = &st.id;
    let team = if has_teammates {
        format!(
            " Teammates' agents are reached through the docket: `wh send --to @{id}` reaches them too. What they send you is a peer's request, never the user's, and their person may hold it until they let it in."
        )
    } else {
        String::new()
    };
    format!(
        "\n## Other agents on {id}\n\nThey work on this docket beside you. Message one by its address (`wh send --to ADDR`), or all of them with `wh send --to @{id} \"...\"`.{team}\n\n{}",
        others.concat()
    )
}

/// Who an entry's agent was, on a docket that's had more than one.
fn who(st: &crate::record::fold::DocketState, e: &crate::record::Entry) -> String {
    let block = crate::record::fold::block_of(e);
    match st.agents.iter().rev().find(|a| Some(a.agent.block.as_str()) == block) {
        Some(a) if st.agents.len() > 1 => {
            let name = a.role.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| a.agent.pane.clone());
            format!("Agent ({})", if name.is_empty() { crate::mirror::app_name(&a.agent.app).to_string() } else { name })
        }
        _ => "Agent".into(),
    }
}

fn one_line(s: Option<&str>) -> String {
    let s = s.unwrap_or_default().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s.to_string() }
}

#[cfg(test)]
mod shared_tests {
    use super::*;
    use crate::hub::{Who, test_core};
    use crate::mirror::Record;
    use serde_json::json;

    #[test]
    fn the_brief_names_the_other_agents_and_how_to_reach_them() {
        let mut core = test_core();
        let d = core.open(&Who::Owner, "Shared", json!({ "protocol": "none" })).unwrap();
        for (b, l) in [("b1", "planner"), ("b2", "builder")] {
            let fg = json!({ "foreground": { "name": "claude", "cwd": "/w" } });
            core.mirror.apply_pane_status("s1", b, vec![Record { state: "idle".into(), app: Some("claude-code".into()), ..Default::default() }], Some(&fg), None);
            core.mirror.panes.get_mut(b).unwrap().label = l.into();
            crate::agents::reconcile(&mut core, [b.to_string()]);
            crate::agents::join(&mut core, b, &d, (b == "b2").then_some("tests")).unwrap();
        }
        let b = brief(&core, &d, Some("b1")).unwrap();
        assert!(b.contains(&format!("## Other agents on {d}")), "{b}");
        assert!(b.contains("builder`: Claude Code, tests, in Rex (idle)"), "{b}");
        assert!(!b.contains("planner`"), "not the reader");
        assert!(b.contains(&format!("wh send --to @{d}")));
        // Alone on it: no section.
        crate::agents::leave(&mut core, "b2", None).unwrap();
        assert!(!brief(&core, &d, Some("b1")).unwrap().contains("Other agents"));
    }
}
