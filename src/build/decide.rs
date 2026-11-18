//! Decisions (`.tmp/decisions/D<n>.json`) and your answers to every ask:
//! a button, a number, a button and a note, or your own words.

use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub id: String,
    pub question: String,
    pub why: Option<String>,
    pub options: Vec<Opt>,
    pub recommend: usize,
    pub blocking: bool,
    pub meanwhile: Option<String>,
}

/// A decision file, checked: the reason it's not valid, or the decision.
pub fn parse(file_id: &str, v: &Value) -> Result<Decision, String> {
    let id = v["id"].as_str().unwrap_or("");
    if id != file_id {
        return Err(format!("its id is \"{id}\", but the file is {file_id}.json"));
    }
    if !(id.len() > 1 && id.starts_with('D') && id[1..].chars().all(|c| c.is_ascii_digit())) {
        return Err("its id must be D followed by a number".into());
    }
    let question = v["question"].as_str().map(str::trim).filter(|q| !q.is_empty()).ok_or("it has no question")?.to_string();
    let raw = v["options"].as_array().ok_or("it has no options")?;
    if !(2..=5).contains(&raw.len()) {
        return Err(format!("it has {} options; give 2 to 5", raw.len()));
    }
    let mut options: Vec<Opt> = Vec::new();
    for o in raw {
        let (label, detail) = match o {
            Value::String(s) => (s.trim().to_string(), None),
            Value::Object(_) => (o["label"].as_str().unwrap_or("").trim().to_string(), o["detail"].as_str().map(str::to_string)),
            _ => return Err("an option must be a label or { label, detail }".into()),
        };
        if label.is_empty() {
            return Err("an option has no label".into());
        }
        if label.chars().count() > 60 {
            return Err(format!("the label \"{label}\" is over 60 characters; put the rest in detail"));
        }
        if options.iter().any(|x| x.label.eq_ignore_ascii_case(&label)) {
            return Err(format!("two options are labelled \"{label}\""));
        }
        options.push(Opt { label, detail });
    }
    let recommend = v["recommend"].as_u64().unwrap_or(0) as usize;
    if recommend >= options.len() {
        return Err(format!("recommend is {recommend}, but there are {} options (0 is the first)", options.len()));
    }
    let blocking = match v["mode"].as_str().unwrap_or("provisional") {
        "provisional" => false,
        "blocking" => true,
        m => return Err(format!("mode is \"{m}\"; it's provisional or blocking")),
    };
    Ok(Decision { id: id.to_string(), question, why: v["why"].as_str().map(str::to_string), options, recommend, blocking, meanwhile: v["meanwhile"].as_str().map(str::to_string) })
}

/// The ask for a decision: its subject, body and buttons.
pub fn ask(label: &str, d: &Decision) -> (String, String, Vec<String>) {
    let subject = format!("{label} · {} · {}", d.id, d.question);
    let mut body = d.why.clone().unwrap_or_default();
    body.push_str("\n\n");
    for (i, o) in d.options.iter().enumerate() {
        body.push_str(&format!(
            "{}. {}{}{}\n",
            i + 1,
            o.label,
            if i == d.recommend { " (recommended)" } else { "" },
            o.detail.as_deref().map(|x| format!(": {x}")).unwrap_or_default()
        ));
    }
    body.push('\n');
    body.push_str(&if d.blocking {
        "The builder is waiting for this answer.".to_string()
    } else {
        format!("Meanwhile: {}", d.meanwhile.clone().unwrap_or_else(|| "the builder goes on with the recommendation.".into()))
    });
    body.push_str("\n\nAnswer with a button, a number, or your own words.");
    (subject.trim().to_string(), body.trim().to_string(), d.options.iter().map(|o| o.label.clone()).collect())
}

/// An answer, read: the option it picks (if any), and the words with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub choice: Option<usize>,
    pub text: String,
}

/// Read a reply against the ask's options: a button (its label), a number
/// (`2`, `2.`, `2, but cap it at 3`), the longest label the reply starts
/// with (followed by a non-word character), or else your own words.
pub fn read(options: &[String], button: Option<&str>, body: &str) -> Reply {
    if let Some(b) = button
        && let Some(i) = options.iter().position(|o| o.eq_ignore_ascii_case(b.trim()))
    {
        let rest = body.trim();
        let text = if rest.eq_ignore_ascii_case(&options[i]) { String::new() } else { rest.to_string() };
        return Reply { choice: Some(i), text };
    }
    let t = body.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &t[digits.len()..];
        let ends = after.is_empty() || after.starts_with(['.', ',', ' ', ')', ':', ';']);
        let word_follows = after.trim_start_matches(['.', ')']).trim_start().starts_with(|c: char| c.is_alphanumeric()) && !after.starts_with([',', '.', ')', ':', ';']);
        if let Ok(n) = digits.parse::<usize>()
            && (1..=options.len()).contains(&n)
            && ends
            && !word_follows
        {
            let note = after.trim_start_matches(['.', ',', ')', ':', ';']).trim();
            return Reply { choice: Some(n - 1), text: note.to_string() };
        }
    }
    let lower = t.to_lowercase();
    let mut best: Option<usize> = None;
    for (i, o) in options.iter().enumerate() {
        let l = o.to_lowercase();
        if lower.starts_with(&l) {
            let next = lower[l.len()..].chars().next();
            if next.is_none_or(|c| !c.is_alphanumeric()) && best.is_none_or(|b| options[b].len() < o.len()) {
                best = Some(i);
            }
        }
    }
    if let Some(i) = best {
        // The words after the label: by its length in the lowercased text,
        // where it matched (lowercasing can change a letter's byte length).
        let cut = lower.char_indices().take_while(|(b, _)| *b < options[i].to_lowercase().len()).count();
        let rest: String = t.chars().skip(cut).collect();
        let note = rest.trim_start_matches([',', '.', ':', ';', '-', ' ']).trim();
        return Reply { choice: Some(i), text: note.to_string() };
    }
    Reply { choice: None, text: t.to_string() }
}

/// The answer file the builder reads: `.tmp/decisions/D<n>.answer.json`.
pub fn answer_file(d: &Decision, r: &Reply, stood: bool, at: &str) -> Value {
    let mut v = json!({ "id": d.id, "text": r.text, "by": if stood { "wheelhouse" } else { "you" }, "via": "wheelhouse", "at": at });
    match r.choice {
        Some(i) => {
            v["choice"] = json!(i);
            v["label"] = json!(d.options[i].label);
        }
        None => v["label"] = json!("Another answer"),
    }
    if stood {
        v["stood"] = json!(true);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d() -> Value {
        json!({ "id": "D1", "question": "Should retries cover 4xx?", "why": "The issue doesn't say.",
                "options": [{ "label": "5xx and timeouts only", "detail": "Safer." }, "Every non-2xx"],
                "recommend": 0, "mode": "provisional", "meanwhile": "Building with 5xx only." })
    }

    #[test]
    fn decision_files_are_checked() {
        let x = parse("D1", &d()).unwrap();
        assert_eq!((x.options.len(), x.recommend, x.blocking), (2, 0, false));
        assert!(parse("D2", &d()).unwrap_err().contains("D2.json"));
        let mut v = d();
        v["options"] = json!(["a"]);
        assert!(parse("D1", &v).unwrap_err().contains("give 2 to 5"));
        let mut v = d();
        v["options"] = json!(["Same", "same"]);
        assert!(parse("D1", &v).unwrap_err().contains("two options"));
        let mut v = d();
        v["recommend"] = json!(5);
        assert!(parse("D1", &v).is_err());
        let mut v = d();
        v["mode"] = json!("urgent");
        assert!(parse("D1", &v).is_err());
        let mut v = d();
        v["options"] = json!(["x".repeat(61), "b"]);
        assert!(parse("D1", &v).unwrap_err().contains("over 60"));
    }

    #[test]
    fn the_ask_reads_well() {
        let (s, b, o) = ask("ENG-142", &parse("D1", &d()).unwrap());
        assert_eq!(s, "ENG-142 · D1 · Should retries cover 4xx?");
        assert!(b.contains("1. 5xx and timeouts only (recommended): Safer.\n2. Every non-2xx\n\nMeanwhile: Building with 5xx only."), "{b}");
        assert_eq!(o, ["5xx and timeouts only", "Every non-2xx"]);
    }

    #[test]
    fn replies_read_like_a_person_wrote_them() {
        let o: Vec<String> = ["Approve", "Revise", "Discard"].iter().map(|s| s.to_string()).collect();
        assert_eq!(read(&o, Some("Approve"), "Approve"), Reply { choice: Some(0), text: "".into() });
        assert_eq!(read(&o, Some("revise"), "tighten D2"), Reply { choice: Some(1), text: "tighten D2".into() });
        assert_eq!(read(&o, None, "2"), Reply { choice: Some(1), text: "".into() });
        assert_eq!(read(&o, None, "2, but cap it at 3"), Reply { choice: Some(1), text: "but cap it at 3".into() });
        assert_eq!(read(&o, None, "1. looks good"), Reply { choice: Some(0), text: "looks good".into() });
        assert_eq!(read(&o, None, "7"), Reply { choice: None, text: "7".into() });
        assert_eq!(read(&o, None, "3 attempts is plenty"), Reply { choice: None, text: "3 attempts is plenty".into() });
        assert_eq!(read(&o, None, "approve, ship it"), Reply { choice: Some(0), text: "ship it".into() });
        assert_eq!(read(&o, None, "Approved by the team?"), Reply { choice: None, text: "Approved by the team?".into() });
        let p: Vec<String> = ["Open", "Open PR"].iter().map(|s| s.to_string()).collect();
        assert_eq!(read(&p, None, "open pr now"), Reply { choice: Some(1), text: "now".into() });
        // A letter whose lowercase is a different length isn't a panic.
        let k: Vec<String> = ["İstanbul", "Paris"].iter().map(|s| s.to_string()).collect();
        assert_eq!(read(&k, None, "İstanbul, please").choice, Some(0));
    }
}
