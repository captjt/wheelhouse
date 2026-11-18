//! Building and reading Settings' patches (SettingsModel.swift `SettingsPatch`):
//! no window needed. Values go in settings.json, laid over build.lua and
//! review.lua; `null` takes a value set here back out (docs/settings.md).

use serde_json::{Map, Value, json};

/// `{part: {a: {b: value}}}`. `None` sends null: the value set here goes,
/// and the Lua's (or the default) applies again.
pub fn make(part: &str, path: &[&str], value: Option<Value>) -> Value {
    let mut v = value.unwrap_or(Value::Null);
    for k in path.iter().rev() {
        let mut m = Map::new();
        m.insert((*k).to_string(), v);
        v = Value::Object(m);
    }
    let mut m = Map::new();
    m.insert(part.to_string(), v);
    Value::Object(m)
}

/// Two patches as one, merged key by key.
pub fn merge(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Object(mut out), Value::Object(b)) => {
            for (k, v) in b {
                let merged = match out.remove(&k) {
                    Some(x @ Value::Object(_)) if v.is_object() => merge(x, v),
                    _ => v,
                };
                out.insert(k, merged);
            }
            Value::Object(out)
        }
        (_, b) => b,
    }
}

/// Whether settings.json (`set`) has a value there: it's set here, not read
/// from the Lua or the default.
pub fn is_set(set: &Value, part: &str, path: &[&str]) -> bool {
    let mut j = &set[part];
    for k in path {
        j = &j[*k];
    }
    !j.is_null()
}

/// A check as settings.json keeps it: all three, so nothing of the Lua's
/// shows through.
pub fn check(run: &str, required: bool, timeout: i64) -> Value {
    json!({ "run": run.trim(), "required": required, "timeout": timeout.clamp(10, 7200) })
}

/// Comma- (or line-) separated items, trimmed, without the empty ones.
fn items(s: &str) -> Vec<String> {
    s.split([',', '\n']).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

/// A posting rule (`{auto: {…}}`) from its form, or what's wrong with it.
/// Lists are typed comma-separated. docs/review.md#who-posts.
pub fn rule(approve: bool, risk: &str, max_lines: &str, authors: &str, labels: &str, not_touching: &str) -> Result<Value, String> {
    let mut auto = Map::new();
    if approve {
        auto.insert("verdict".into(), json!("approve"));
    }
    if risk == "low" || risk == "medium" {
        auto.insert("merge_risk".into(), json!(risk));
    }
    let n = max_lines.trim();
    if !n.is_empty() {
        match n.parse::<i64>() {
            Ok(lines) if lines >= 1 && n.chars().all(|c| c.is_ascii_digit()) => {
                auto.insert("max_lines".into(), json!(lines));
            }
            _ => return Err("Lines: a whole number, 1 or more.".into()),
        }
    }
    for (k, v) in [("authors", authors), ("labels", labels), ("not_touching", not_touching)] {
        let list = items(v);
        if !list.is_empty() {
            auto.insert(k.into(), json!(list));
        }
    }
    if auto.is_empty() {
        return Err("Pick at least one condition. For a review that always posts itself, choose Auto.".into());
    }
    Ok(json!({ "auto": auto }))
}

/// A check's name: letters, digits, dashes and underscores.
pub fn valid_check_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A guide's file from what you call it: "API review" → "api-review.md".
/// None when nothing usable is left.
pub fn guide_file(title: &str) -> Option<String> {
    let mut out = String::new();
    let mut dash = false;
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        return None;
    }
    Some(format!("{}.md", out.chars().take(60).collect::<String>().trim_matches('-')))
}

/// The `{{VARIABLES}}` a brief uses, each once, in order.
pub fn variables(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = &after[..close];
        if !name.is_empty() && name.chars().all(|c| c.is_uppercase() || c.is_numeric() || c == '_') {
            let v = format!("{{{{{name}}}}}");
            if !out.contains(&v) {
                out.push(v);
            }
        }
        rest = &after[close + 2..];
    }
    out
}

/// The variables the built-in uses that `text` leaves out: what the agents
/// would no longer be told.
pub fn dropped(original: &str, text: &str) -> Vec<String> {
    let kept = variables(text);
    variables(original).into_iter().filter(|v| !kept.contains(v)).collect()
}

/// Where a guide is used, in words.
pub fn used_by(s: &str) -> String {
    let Some((proto, key)) = s.split_once(':') else { return s.to_string() };
    let (proto, key) = (proto.trim(), key.trim());
    let proto = match proto {
        "build" => "Build",
        "review" => "Review",
        p => p,
    };
    if let Some(body) = key.strip_prefix("repos.") {
        // repos.<slug>.guides.<slot>: the slug may have dots of its own.
        if let Some(i) = body.rfind(".guides.") {
            return format!("{}'s {} guide", &body[..i], body[i + ".guides.".len()..].replace('_', " "));
        }
    }
    if let Some(slot) = key.strip_prefix("guides.") {
        return format!("{proto}'s {}", slot.replace('_', " "));
    }
    format!("{proto}: {key}")
}

/// A diff line's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Same,
    Add,
    Del,
}

/// A line diff (longest common subsequence), as Intent.swift's `lineDiff`:
/// enough for a guide against its built-in.
pub fn line_diff(a: &str, b: &str) -> Vec<(Op, String)> {
    let x: Vec<&str> = if a.is_empty() { vec![] } else { a.split('\n').collect() };
    let y: Vec<&str> = if b.is_empty() { vec![] } else { b.split('\n').collect() };
    let (n, m) = (x.len(), y.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if x[i] == y[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut out = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if x[i] == y[j] {
            out.push((Op::Same, x[i].to_string()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push((Op::Del, x[i].to_string()));
            i += 1;
        } else {
            out.push((Op::Add, y[j].to_string()));
            j += 1;
        }
    }
    out.extend(x[i..].iter().map(|l| (Op::Del, l.to_string())));
    out.extend(y[j..].iter().map(|l| (Op::Add, l.to_string())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(v: &Value) -> String {
        v.to_string()
    }

    #[test]
    fn builds_posting_rules() {
        let r = rule(true, "medium", "400", "alice, bob", " safe ,, docs\n", "db/migrations/").unwrap();
        assert_eq!(
            canon(&r),
            r#"{"auto":{"authors":["alice","bob"],"labels":["safe","docs"],"max_lines":400,"merge_risk":"medium","not_touching":["db/migrations/"],"verdict":"approve"}}"#
        );
        assert_eq!(rule(true, "", "40.5", "", "", "").unwrap_err(), "Lines: a whole number, 1 or more.");
        assert_eq!(rule(true, "", "0", "", "", "").unwrap_err(), "Lines: a whole number, 1 or more.");
        assert!(rule(false, "", " ", "", ",", "").unwrap_err().contains("choose Auto"));
        let patch = make("review", &["repos", "o/a", "post"], Some(r));
        assert!(canon(&patch).starts_with(r#"{"review":{"repos":{"o/a":{"post":{"auto":{"#));
    }

    #[test]
    fn builds_settings_patches() {
        assert_eq!(canon(&make("build", &["gates", "open"], Some(json!("auto")))), r#"{"build":{"gates":{"open":"auto"}}}"#);
        assert_eq!(canon(&make("build", &["repos", "acme/api", "checks", "lint"], None)), r#"{"build":{"repos":{"acme/api":{"checks":{"lint":null}}}}}"#);
        assert_eq!(canon(&make("build", &["repos", "acme/api", "plan"], Some(json!(false)))), r#"{"build":{"repos":{"acme/api":{"plan":false}}}}"#);
        let both = merge(make("build", &["repos", "o/a", "notes"], Some(json!("n"))), make("review", &["repos", "o/a", "notes"], Some(json!("n"))));
        assert_eq!(canon(&both), r#"{"build":{"repos":{"o/a":{"notes":"n"}}},"review":{"repos":{"o/a":{"notes":"n"}}}}"#);
        let deeper = merge(make("build", &["gates", "open"], Some(json!("auto"))), make("build", &["gates", "push"], None));
        assert_eq!(canon(&deeper), r#"{"build":{"gates":{"open":"auto","push":null}}}"#);
        assert_eq!(canon(&check(" cargo test ", false, 5)), r#"{"required":false,"run":"cargo test","timeout":10}"#);
        let set: Value = serde_json::from_str(r#"{"build": {"gates": {"open": "auto"}, "repos": {"o/a": {"plan": false}}}}"#).unwrap();
        assert!(is_set(&set, "build", &["gates", "open"]));
        assert!(!is_set(&set, "build", &["gates", "plan"]));
        // `false` is a value set here.
        assert!(is_set(&set, "build", &["repos", "o/a", "plan"]));
        assert!(!is_set(&set, "review", &["repos", "o/a", "notes"]));
    }

    #[test]
    fn names_guides_and_checks() {
        assert_eq!(guide_file("API review").as_deref(), Some("api-review.md"));
        assert_eq!(guide_file("  Storefront: checkout & cart! ").as_deref(), Some("storefront-checkout-cart.md"));
        assert_eq!(guide_file("¿?"), None);
        assert!(valid_check_name("e2e_web-1"));
        assert!(!valid_check_name("run tests"));
        assert!(!valid_check_name(""));
    }

    #[test]
    fn finds_brief_variables() {
        let original = "# Plan {{KEY}}: {{TITLE}}\n{{PLAN_FORMAT}}\n{{PLAN_GUIDE}} and {{KEY}} again, not {{lower}} or {{ SPACED }}";
        assert_eq!(variables(original), ["{{KEY}}", "{{TITLE}}", "{{PLAN_FORMAT}}", "{{PLAN_GUIDE}}"]);
        let edited = "# Plan {{KEY}}: {{TITLE}}\n{{PLAN_FORMAT}}\nMy own words.";
        assert_eq!(dropped(original, edited), ["{{PLAN_GUIDE}}"]);
        assert!(dropped(original, original).is_empty());
    }

    #[test]
    fn words_where_guides_are_used() {
        assert_eq!(used_by("review: repos.acme/api.guides.review"), "acme/api's review guide");
        assert_eq!(used_by("review: repos.my.dotted/repo.guides.merge_risk"), "my.dotted/repo's merge risk guide");
        assert_eq!(used_by("build: guides.plan_format"), "Build's plan format");
        assert_eq!(used_by("odd"), "odd");
    }

    #[test]
    fn diffs_lines() {
        let d = line_diff("a\nb\nc", "a\nc\nd");
        assert_eq!(d, vec![(Op::Same, "a".into()), (Op::Del, "b".into()), (Op::Same, "c".into()), (Op::Add, "d".into())]);
        assert!(line_diff("", "").is_empty());
    }
}
