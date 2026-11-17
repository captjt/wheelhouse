//! The builder's plan (`.tmp/plan.md`, guides/plan-format.md): its sections,
//! whether it's ready to show, the intent it proposes, and the plan block a
//! PR carries.

/// The plan's sections by name (`Intent`, `Acceptance`, …), in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub title: Option<String>,
    pub sections: Vec<(String, String)>,
}

impl Plan {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.sections.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, b)| b.as_str()).filter(|b| !b.trim().is_empty())
    }

    /// Acceptance items, as written.
    pub fn acceptance(&self) -> Vec<String> {
        items(self.get("Acceptance").unwrap_or(""))
    }

    /// Decision ids the plan lists (`D1`, `D2`).
    #[cfg(test)]
    pub fn decisions(&self) -> Vec<String> {
        let mut out = Vec::new();
        for l in self.get("Decisions").unwrap_or("").lines() {
            let t = l.trim_start_matches(['-', '*', ' ']);
            if let Some(id) = t.split([':', ' ', '.']).next()
                && id.len() > 1
                && id.starts_with('D')
                && id[1..].chars().all(|c| c.is_ascii_digit())
                && !out.contains(&id.to_string())
            {
                out.push(id.to_string());
            }
        }
        out
    }
}

fn items(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in body.lines() {
        let t = l.trim_start();
        let item = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| {
            let d = t.find(|c: char| !c.is_ascii_digit())?;
            (d > 0 && (t[d..].starts_with(". ") || t[d..].starts_with(") "))).then(|| &t[d + 2..])
        });
        match item {
            Some(i) => out.push(i.trim().to_string()),
            None if !t.is_empty() && !out.is_empty() => {
                let last = out.last_mut().expect("checked");
                last.push(' ');
                last.push_str(t.trim());
            }
            None => {}
        }
    }
    out
}

pub fn parse(md: &str) -> Plan {
    let mut p = Plan::default();
    let mut cur: Option<(String, Vec<&str>)> = None;
    for line in md.lines() {
        if let Some(t) = line.strip_prefix("# ") {
            if p.title.is_none() && cur.is_none() {
                p.title = Some(t.trim().trim_start_matches("Plan:").trim().to_string());
            }
            continue;
        }
        if let Some(h) = line.strip_prefix("## ") {
            if let Some((n, b)) = cur.take() {
                p.sections.push((n, b.join("\n").trim().to_string()));
            }
            cur = Some((h.trim().to_string(), Vec::new()));
            continue;
        }
        if let Some((_, b)) = cur.as_mut() {
            b.push(line);
        }
    }
    if let Some((n, b)) = cur {
        p.sections.push((n, b.join("\n").trim().to_string()));
    }
    p
}

/// What's wrong with a plan, before it goes to you; empty when it's ready.
pub fn problems(p: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    if p.get("Intent").is_none() {
        out.push("it has no ## Intent".to_string());
    }
    match p.get("Acceptance") {
        None => out.push("it has no ## Acceptance".to_string()),
        Some(_) if p.acceptance().is_empty() => out.push("## Acceptance isn't a list of items".to_string()),
        Some(_) => {}
    }
    out
}

/// The intent the plan proposes: what it's for, what's done, what's out.
pub fn intent(p: &Plan) -> String {
    let mut out = String::new();
    if let Some(i) = p.get("Intent") {
        out.push_str(i.trim());
        out.push('\n');
    }
    for (name, title) in [("Acceptance", "Acceptance"), ("Out of scope", "Out of scope")] {
        if let Some(b) = p.get(name) {
            out.push_str(&format!("\n## {title}\n\n{}\n", b.trim()));
        }
    }
    out
}

/// A decision as it stands, for the plan block.
pub struct Decided {
    pub id: String,
    pub question: String,
    /// The option's label, or your own words.
    pub answer: Option<String>,
    /// `you` (with a note), or `stood` (nobody answered).
    pub by: Option<String>,
    pub note: Option<String>,
}

pub const MARK: &str = "<!-- wheelhouse:plan v1";
pub const END: &str = "<!-- /wheelhouse:plan -->";

/// The plan block on the PR: the intent in force, and the decisions with
/// who made each. `mode`: `full` (with Notes) or `summary`.
pub fn block(key: &str, intent_md: &str, notes: Option<&str>, decisions: &[Decided], approved_by: &str, mode: &str) -> String {
    let parsed = parse(&format!("## Intent\n{intent_md}"));
    let mut b = format!("{MARK} {key} -->\n<details open><summary><b>Plan</b> · {key} · approved by {approved_by}</summary>\n\n");
    if let Some(i) = parsed.get("Intent") {
        let one = !i.contains('\n');
        b.push_str(&if one { format!("**Intent.** {}\n\n", i.trim()) } else { format!("**Intent**\n\n{}\n\n", i.trim()) });
    }
    if let Some(a) = parsed.get("Acceptance") {
        b.push_str(&format!("**Acceptance**\n\n{}\n\n", a.trim()));
    }
    if !decisions.is_empty() {
        b.push_str("**Decisions**\n\n");
        for d in decisions {
            let what = match (&d.answer, d.by.as_deref()) {
                (Some(a), Some("stood")) => format!("{a}. The recommendation stood; nobody answered."),
                (Some(a), _) => format!("{a}. Chosen by {approved_by}{}.", d.note.as_deref().filter(|n| !n.trim().is_empty()).map(|n| format!(": \"{}\"", n.chars().take(300).collect::<String>())).unwrap_or_default()),
                (None, _) => "Open: not answered yet.".into(),
            };
            b.push_str(&format!("- **{} · {}** {what}\n", d.id, d.question));
        }
        b.push('\n');
    }
    if let Some(o) = parsed.get("Out of scope") {
        b.push_str(&format!("**Out of scope**\n\n{}\n\n", o.trim()));
    }
    if mode == "full"
        && let Some(n) = notes.filter(|n| !n.trim().is_empty())
    {
        b.push_str(&format!("**Notes**\n\n{}\n\n", n.trim()));
    }
    b.push_str("</details>\n");
    b.push_str(END);
    b
}

/// A PR body without any plan block in it.
pub fn strip(body: &str) -> String {
    match (body.find(MARK), body.find(END)) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &body[..a], &body[b + END.len()..]).trim().to_string(),
        _ => body.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Plan: GRD-142 Retry failed webhooks\n\n## Intent\nRetry failed deliveries with backoff, so a\nflaky endpoint doesn't drop events.\n\n## Acceptance\n- A 5xx is retried up to 3 times (`webhooks::retry::backs_off`).\n- A 4xx isn't retried,\n  and is logged.\n\n## Decisions\n- D1: Should retries cover 4xx? Recommended: no.\n- D2. Cap? Recommended: 3.\n\n## Approach\nAdd a retry loop in `deliver`.\n\n## Out of scope\n- Dead-letter queue.\n\n## Notes\n- Backoff doubles, the usual choice.\n";

    #[test]
    fn a_plan_parses_and_proposes_its_intent() {
        let p = parse(PLAN);
        assert_eq!(p.title.as_deref(), Some("GRD-142 Retry failed webhooks"));
        assert!(problems(&p).is_empty(), "{:?}", problems(&p));
        assert_eq!(p.acceptance(), ["A 5xx is retried up to 3 times (`webhooks::retry::backs_off`).", "A 4xx isn't retried, and is logged."]);
        assert_eq!(p.decisions(), ["D1", "D2"]);
        let i = intent(&p);
        assert!(i.starts_with("Retry failed deliveries"));
        assert!(i.contains("\n## Acceptance\n\n- A 5xx") && i.contains("\n## Out of scope\n\n- Dead-letter queue."));
        assert!(!i.contains("Approach") && !i.contains("Notes"));
    }

    #[test]
    fn an_unfinished_plan_says_whats_missing() {
        assert_eq!(problems(&parse("## Intent\nx\n")), ["it has no ## Acceptance"]);
        assert_eq!(problems(&parse("## Acceptance\nsome prose\n")), ["it has no ## Intent", "## Acceptance isn't a list of items"]);
        assert_eq!(parse("## Acceptance\n1. One\n2) Two\n").acceptance(), ["One", "Two"]);
    }

    #[test]
    fn the_plan_block_names_who_decided() {
        let p = parse(PLAN);
        let ds = vec![
            Decided { id: "D1".into(), question: "Should retries cover 4xx?".into(), answer: Some("No".into()), by: Some("you".into()), note: Some("log them".into()) },
            Decided { id: "D2".into(), question: "Cap?".into(), answer: Some("3".into()), by: Some("stood".into()), note: None },
        ];
        let b = block("GRD-142", &intent(&p), p.get("Notes"), &ds, "@captjt", "full");
        assert!(b.starts_with("<!-- wheelhouse:plan v1 GRD-142 -->\n<details open><summary><b>Plan</b> · GRD-142 · approved by @captjt</summary>"), "{b}");
        assert!(b.contains("**Intent**\n\nRetry failed deliveries"), "{b}");
        assert!(b.contains("- **D1 · Should retries cover 4xx?** No. Chosen by @captjt: \"log them\"."), "{b}");
        assert!(b.contains("- **D2 · Cap?** 3. The recommendation stood; nobody answered."), "{b}");
        assert!(b.contains("**Notes**\n\n- Backoff doubles"), "{b}");
        let s = block("GRD-142", &intent(&p), p.get("Notes"), &ds, "@captjt", "summary");
        assert!(!s.contains("**Notes**"));
        let body = format!("Why.\n\n{b}\n\n---\nfooter");
        assert_eq!(strip(&body), "Why.\n\n\n\n---\nfooter");
        assert!(!strip(&body).contains("wheelhouse:plan"));
    }
}
