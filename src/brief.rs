//! The brief: what an agent reads to work on a docket. The intent, the
//! Linear issue, what was said and done lately, and how to write back.

use anyhow::{Result, anyhow};

use crate::hub::Core;

pub fn brief(core: &Core, id: &str) -> Result<String> {
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
    let mut recent = Vec::new();
    for s in entries.iter().rev() {
        let Ok(e) = s.entry() else { continue };
        let line = match (e.kind.as_str(), e.data["via"].as_str(), e.data["what"].as_str()) {
            ("note", Some("prompt"), _) => format!("You asked: {}", one_line(e.body.as_deref())),
            ("note", _, _) => format!("Note ({}): {}", e.actor.id, one_line(e.body.as_deref())),
            ("turn", _, _) if e.body.is_some() => format!("Agent: {}", e.data["summary"].as_str().map(str::to_string).unwrap_or_else(|| one_line(e.body.as_deref()))),
            ("observed", _, Some("linear-comment")) => format!("{} in Linear: {}", e.data["author"].as_str().unwrap_or("someone"), one_line(e.body.as_deref())),
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

fn one_line(s: Option<&str>) -> String {
    let s = s.unwrap_or_default().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s.to_string() }
}
