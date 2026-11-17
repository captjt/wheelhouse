//! The briefs agents get, copied from rex-reviewer as text. A file of the
//! same name in `~/.config/wheelhouse/briefs/review/` (or rex-reviewer's
//! `briefs/`) replaces the built-in one.

use std::collections::BTreeMap;

const BUILT_IN: [(&str, &str); 14] = [
    ("ground-rules.md", include_str!("../../briefs/review/ground-rules.md")),
    ("round-1.md", include_str!("../../briefs/review/round-1.md")),
    ("round-n.md", include_str!("../../briefs/review/round-n.md")),
    ("delta.md", include_str!("../../briefs/review/delta.md")),
    ("check.md", include_str!("../../briefs/review/check.md")),
    ("merge-risk.md", include_str!("../../briefs/review/merge-risk.md")),
    ("parts/merge-drafts.md", include_str!("../../briefs/review/parts/merge-drafts.md")),
    ("parts/merge-risk-add.md", include_str!("../../briefs/review/parts/merge-risk-add.md")),
    ("parts/merge-risk-start.md", include_str!("../../briefs/review/parts/merge-risk-start.md")),
    ("parts/parallel.md", include_str!("../../briefs/review/parts/parallel.md")),
    ("parts/post-ask.md", include_str!("../../briefs/review/parts/post-ask.md")),
    ("parts/post-auto.md", include_str!("../../briefs/review/parts/post-auto.md")),
    ("parts/review-default.md", include_str!("../../briefs/review/parts/review-default.md")),
    ("review-draft.md", include_str!("../../briefs/review/review-draft.md")),
];

pub const REVIEW_STYLE: &str = include_str!("../../guides/review-style.md");
pub const MERGE_RISK: &str = include_str!("../../guides/merge-risk.md");

pub fn brief(name: &str) -> String {
    let home = crate::paths::home();
    for dir in [home.join(".config/wheelhouse/briefs/review"), home.join(".config/rex-reviewer/briefs")] {
        if let Ok(s) = std::fs::read_to_string(dir.join(name)) {
            return s;
        }
    }
    BUILT_IN.iter().find(|(n, _)| *n == name).map(|(_, s)| s.to_string()).unwrap_or_else(|| panic!("no brief {name}"))
}

pub type Vars = BTreeMap<&'static str, String>;

/// `{{NAME}}` → its value, in one pass; unknown names stay as they are.
pub fn render(tpl: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let name_len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        let name = &after[..name_len];
        if name_len > 0 && after[name_len..].starts_with("}}") {
            match vars.get(name) {
                Some(v) => out.push_str(v),
                None => {
                    out.push_str("{{");
                    out.push_str(name);
                    out.push_str("}}");
                }
            }
            rest = &after[name_len + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_fill_and_unknown_ones_stay() {
        let mut v = Vars::new();
        v.insert("PR", "7".into());
        v.insert("X", "{{PR}}".into());
        assert_eq!(render("#{{PR}} {{NOPE}} {{X}} {{ PR }} {{", &v), "#7 {{NOPE}} {{PR}} {{ PR }} {{");
        for (name, _) in BUILT_IN {
            assert!(!brief(name).is_empty());
        }
    }
}
