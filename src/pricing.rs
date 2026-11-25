//! What the agents' tokens cost: each model's price per million tokens,
//! for input, input read from the cache, input written to it, and output.
//! Built in from each provider's own price list, and set or changed in
//! settings.json (`prices`). A model with no price is left unpriced, never
//! guessed: Recap says how many tokens that leaves out.
//!
//! These are API prices. Work done on a subscription (Claude Code on a
//! Claude plan, Codex signed in with ChatGPT) costs what it would through
//! the API, not what the plan bills.
//!
//! Money is kept in whole units, never floats: a price in micro-dollars
//! (millionths of a dollar) per million tokens, and a cost in pico-dollars
//! (10⁻¹² of a dollar), which is tokens times price exactly. Sums don't
//! drift however many minutes they add up, and are rounded once, to the
//! micro-dollar, when they're shown.

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// A model's price: micro-dollars per million tokens, in the order a
/// `usage::Row` counts tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price(pub [i64; 4]);

/// The fields of a price, as settings.json and the usage JSON name them.
pub const FIELDS: [&str; 4] = ["input", "cache_read", "cache_write", "output"];

/// No model costs more than this, in dollars per million tokens: a bigger
/// price is a slip (cents typed as dollars, a price per token).
const MAX_PRICE: f64 = 10_000.0;
/// No settings.json names more models than this.
const MAX_MODELS: usize = 1000;
/// A provider's or model's name is no longer than this.
const MAX_NAME: usize = 200;

impl Price {
    /// What tokens `[input, cache_read, cache_write, output]` cost, in
    /// pico-dollars. Exact: an i64 count times an i64 price always fits an
    /// i128, and a negative count (never kept) costs nothing.
    pub fn cost(&self, tokens: [i64; 4]) -> i128 {
        tokens.iter().zip(self.0).fold(0i128, |a, (n, p)| a.saturating_add(i128::from((*n).max(0)) * i128::from(p)))
    }

    /// Dollars per million tokens, for the apps.
    pub fn json(&self, source: &str) -> Value {
        let mut o: Map<String, Value> = FIELDS.iter().zip(self.0).map(|(k, p)| (k.to_string(), json!(p as f64 / 1e6))).collect();
        o.insert("source".into(), json!(source));
        Value::Object(o)
    }
}

/// Dollars per million tokens as micro-dollars per million: `4.0` → `4_000_000`.
fn micros(d: f64) -> i64 {
    (d * 1e6).round() as i64
}

/// When a price takes effect: 2026-12-01 00:00 UTC. Google's list says
/// "starting December 1, 2026" without a time zone; UTC is the guess the
/// rest of the hub makes, and moves a few hours' calls at most.
const DEC_1_2026: i64 = 1_796_083_200;
/// 2027-01-01 00:00 UTC.
const JAN_1_2027: i64 = 1_798_761_600;

/// Each built-in price: provider, the model's name as `usage` makes it,
/// when it takes effect (unix seconds; `None` for always), and dollars per
/// million tokens `[input, cache_read, cache_write, output]`.
///
/// Where each comes from, read 2026-11-25:
///
/// - **Anthropic:** platform.claude.com/docs/en/about-claude/pricing.
///   Cache writes are priced at the 5-minute TTL's rate (1.25× input), the
///   API's default: the store keeps how many tokens were written, not to
///   which cache. Work that asked for the 1-hour TTL (2× input) costs
///   more; set `cache_write` in settings.json for it.
///   Claude Haiku 5.5 is at its price for prompts up to 100,000 tokens;
///   longer ones cost 5× and the store can't tell them apart. Fast mode
///   and US-only inference (`inference_geo`) aren't counted either.
/// - **OpenAI:** developers.openai.com/api/docs/pricing and its prompt
///   caching guide. Standard tier, short context (up to 272K input
///   tokens). GPT-5.6 and later charge 1.25× input for a write to the
///   default 30-minute cache; earlier models charge nothing extra for a
///   write, so a written token costs what any input token does.
/// - **Google:** aistudio.google.com/docs/pricing (the Gemini API), paid
///   tier, standard processing. Gemini charges nothing extra to write its
///   cache, so a written token costs what any input token does; the hourly
///   storage of an explicit cache isn't counted. Gemini 3.1 Pro Preview is
///   at its price for prompts up to 200K tokens.
///
/// Models a provider lists without a price for cached input (GPT-5.5 Pro
/// and the like) aren't here: there's no price to give them.
#[rustfmt::skip]
const BUILT_IN: &[(&str, &str, Option<i64>, [f64; 4])] = &[
    ("anthropic", "Fable 5.2", None, [8.0, 0.20, 10.0, 40.0]),
    ("anthropic", "Mythos 5.2", None, [8.0, 0.20, 10.0, 40.0]),
    ("anthropic", "Fable 5.1", None, [10.0, 0.25, 12.5, 50.0]),
    ("anthropic", "Mythos 5.1", None, [10.0, 0.25, 12.5, 50.0]),
    ("anthropic", "Fable 5", None, [10.0, 1.0, 12.5, 50.0]),
    ("anthropic", "Mythos 5", None, [10.0, 1.0, 12.5, 50.0]),
    ("anthropic", "Opus 5.5", None, [4.0, 0.20, 5.0, 20.0]),
    ("anthropic", "Opus 5", None, [5.0, 0.50, 6.25, 25.0]),
    ("anthropic", "Opus 4.8", None, [5.0, 0.50, 6.25, 25.0]),
    ("anthropic", "Opus 4.7", None, [5.0, 0.50, 6.25, 25.0]),
    ("anthropic", "Opus 4.6", None, [5.0, 0.50, 6.25, 25.0]),
    ("anthropic", "Opus 4.5", None, [5.0, 0.50, 6.25, 25.0]),
    ("anthropic", "Opus 4.1", None, [15.0, 1.50, 18.75, 75.0]),
    ("anthropic", "Opus 4", None, [15.0, 1.50, 18.75, 75.0]),
    ("anthropic", "Sonnet 5.5", None, [2.0, 0.10, 2.5, 10.0]),
    ("anthropic", "Sonnet 5", None, [2.0, 0.20, 2.5, 10.0]),
    ("anthropic", "Sonnet 4.6", None, [3.0, 0.30, 3.75, 15.0]),
    ("anthropic", "Sonnet 4.5", None, [3.0, 0.30, 3.75, 15.0]),
    ("anthropic", "Sonnet 4", None, [3.0, 0.30, 3.75, 15.0]),
    ("anthropic", "Haiku 5.5", None, [0.10, 0.01, 0.125, 0.50]),
    ("anthropic", "Haiku 4.5", None, [1.0, 0.10, 1.25, 5.0]),
    ("openai", "GPT-6.2 Astra", None, [8.0, 0.40, 10.0, 40.0]),
    ("openai", "GPT-6.2 Sol", None, [2.0, 0.10, 2.5, 10.0]),
    ("openai", "GPT-6.1 Sol", None, [2.0, 0.10, 2.5, 10.0]),
    ("openai", "GPT-6.1 Terra", None, [1.0, 0.05, 1.25, 5.0]),
    ("openai", "GPT-6 Astra", None, [10.0, 1.0, 12.5, 50.0]),
    ("openai", "GPT-6 Sol", None, [2.0, 0.20, 2.5, 10.0]),
    ("openai", "GPT-6 Luna", None, [0.10, 0.01, 0.125, 0.50]),
    ("openai", "GPT-5.6 Sol", None, [4.0, 0.40, 5.0, 20.0]),
    ("openai", "GPT-5.6 Terra", None, [2.0, 0.20, 2.5, 12.0]),
    ("openai", "GPT-5.6 Luna", None, [0.20, 0.02, 0.25, 1.20]),
    ("openai", "GPT-5.5", None, [5.0, 0.50, 5.0, 30.0]),
    ("openai", "GPT-5.4", None, [2.5, 0.25, 2.5, 15.0]),
    ("openai", "GPT-5.4 Mini", None, [0.75, 0.075, 0.75, 4.5]),
    ("openai", "GPT-5.3 Codex", None, [1.75, 0.175, 1.75, 14.0]),
    ("openai", "GPT-5.2", None, [1.75, 0.175, 1.75, 14.0]),
    ("openai", "GPT-5.1", None, [1.25, 0.125, 1.25, 10.0]),
    ("openai", "GPT-5", None, [1.25, 0.125, 1.25, 10.0]),
    ("openai", "GPT-5 Mini", None, [0.25, 0.025, 0.25, 2.0]),
    ("google", "Gemini 4 Carbon", None, [2.0, 0.10, 2.0, 10.0]),
    ("google", "Gemini 4 Carbon", Some(DEC_1_2026), [4.0, 0.20, 4.0, 20.0]),
    ("google", "Gemini 4 Argon", None, [2.0, 0.10, 2.0, 10.0]),
    ("google", "Gemini 4 Argon", Some(DEC_1_2026), [4.0, 0.20, 4.0, 20.0]),
    ("google", "Gemini 4 Neon", None, [3.0, 0.15, 3.0, 15.0]),
    ("google", "Gemini 4 Hydrogen", None, [1.5, 0.075, 1.5, 7.5]),
    ("google", "Gemini 3.8 Flash", None, [0.75, 0.075, 0.75, 3.75]),
    ("google", "Gemini 3.8 Flash", Some(JAN_1_2027), [1.5, 0.15, 1.5, 7.5]),
    ("google", "Gemini 3.6 Flash", None, [0.75, 0.075, 0.75, 3.75]),
    ("google", "Gemini 3.6 Flash", Some(JAN_1_2027), [1.5, 0.15, 1.5, 7.5]),
    ("google", "Gemini 3.5 Flash Lite", None, [0.30, 0.03, 0.30, 2.5]),
    ("google", "Gemini 3.1 Pro Preview", None, [2.0, 0.20, 2.0, 12.0]),
];

type Key = (String, String);

/// A price set in settings.json: the fields it sets, over the built-in
/// one's (a model with none needs all four).
#[derive(Debug, Clone, PartialEq)]
struct Set {
    provider: String,
    model: String,
    fields: [Option<i64>; 4],
}

/// Every model's price, and what settings.json changed.
#[derive(Debug, Clone, Default)]
pub struct Prices {
    /// Each model's prices, oldest first, each from when it takes effect.
    table: HashMap<Key, Vec<(i64, Price)>>,
    /// Models whose price settings.json set or changed.
    set: std::collections::HashSet<Key>,
    /// What in settings.json wasn't used, and why.
    pub notes: Vec<String>,
}

impl Prices {
    /// The built-in prices alone.
    pub fn built_in() -> Prices {
        let mut table: HashMap<Key, Vec<(i64, Price)>> = HashMap::new();
        for (provider, model, from, d) in BUILT_IN {
            table.entry((provider.to_string(), model.to_string())).or_default().push((from.unwrap_or(i64::MIN), Price(d.map(micros))));
        }
        for v in table.values_mut() {
            v.sort_by_key(|(from, _)| *from);
        }
        Prices { table, ..Default::default() }
    }

    /// The built-in prices with settings.json's `prices` over them. What
    /// can't be used is left out and said in `notes`; the rest still counts.
    pub fn with(over: &Value) -> Prices {
        let mut p = Prices::built_in();
        let (sets, notes) = parse(over);
        p.notes = notes;
        for s in sets {
            if let Err(e) = p.apply(s) {
                p.notes.push(e);
            }
        }
        p
    }

    /// The prices in force: built in, with settings.json's. A settings.json
    /// that can't be read leaves the built-in prices, and says so.
    pub fn load() -> Prices {
        match crate::settings::load() {
            Ok(s) => Prices::with(&s["prices"]),
            Err(e) => {
                let mut p = Prices::built_in();
                p.notes.push(format!("{e:#}, so the built-in prices are used"));
                p
            }
        }
    }

    fn apply(&mut self, s: Set) -> std::result::Result<(), String> {
        let key = (s.provider, s.model);
        match self.table.get_mut(&key) {
            Some(dated) => {
                for (_, price) in dated.iter_mut() {
                    for (p, f) in price.0.iter_mut().zip(s.fields) {
                        if let Some(f) = f {
                            *p = f;
                        }
                    }
                }
            }
            None => {
                let [Some(a), Some(b), Some(c), Some(d)] = s.fields else {
                    return Err(format!("{} has no built-in price, so its price needs all of {}", at(&key.0, &key.1), FIELDS.join(", ")));
                };
                self.table.insert(key.clone(), vec![(i64::MIN, Price([a, b, c, d]))]);
            }
        }
        self.set.insert(key);
        Ok(())
    }

    /// A model's price at a time (unix seconds), if it has one.
    pub fn at(&self, provider: &str, model: &str, t: i64) -> Option<Price> {
        let dated = self.table.get(&(provider.to_string(), model.to_string()))?;
        dated.iter().rev().find(|(from, _)| *from <= t).or(dated.first()).map(|(_, p)| *p)
    }

    /// Where a model's price comes from: `settings` or `built-in`.
    pub fn source(&self, provider: &str, model: &str) -> &'static str {
        if self.set.contains(&(provider.to_string(), model.to_string())) { "settings" } else { "built-in" }
    }
}

/// `prices.anthropic["Opus 5.5"]`, for a message.
fn at(provider: &str, model: &str) -> String {
    format!("prices.{provider}[\"{model}\"]")
}

/// settings.json's `prices`, `{ provider: { model: { field: dollars } } }`:
/// what can be used, and what can't, with why.
fn parse(v: &Value) -> (Vec<Set>, Vec<String>) {
    let (mut sets, mut bad) = (Vec::new(), Vec::new());
    let Some(providers) = v.as_object() else {
        if !v.is_null() {
            bad.push(format!("prices is an object of providers, not {}", kind(v)));
        }
        return (sets, bad);
    };
    for (provider, models) in providers {
        if provider.trim().is_empty() || provider.len() > MAX_NAME {
            bad.push(format!("prices has a provider named {provider:?}: a name is 1 to {MAX_NAME} characters"));
            continue;
        }
        let Some(models) = models.as_object() else {
            bad.push(format!("prices.{provider} is an object of models, not {}", kind(models)));
            continue;
        };
        for (model, fields) in models {
            if sets.len() >= MAX_MODELS {
                bad.push(format!("prices names more than {MAX_MODELS} models: the rest aren't used"));
                return (sets, bad);
            }
            let here = at(provider, model);
            if model.trim().is_empty() || model.len() > MAX_NAME {
                bad.push(format!("{here}: a model's name is 1 to {MAX_NAME} characters"));
                continue;
            }
            match price(&here, fields) {
                Ok(f) => sets.push(Set { provider: provider.clone(), model: model.clone(), fields: f }),
                Err(e) => bad.push(e),
            }
        }
    }
    (sets, bad)
}

/// One model's fields: each a price in dollars per million tokens.
fn price(here: &str, v: &Value) -> std::result::Result<[Option<i64>; 4], String> {
    let Some(o) = v.as_object() else { return Err(format!("{here} is an object of {}, not {}", FIELDS.join(", "), kind(v))) };
    let mut out = [None; 4];
    for (k, x) in o {
        let Some(i) = FIELDS.iter().position(|f| f == k) else {
            return Err(format!("{here} has {k}, which isn't a price: they're {}", FIELDS.join(", ")));
        };
        let d = x.as_f64().filter(|d| d.is_finite() && (0.0..=MAX_PRICE).contains(d));
        let Some(d) = d else {
            return Err(format!("{here}.{k} is dollars per million tokens, from 0 to {MAX_PRICE}, not {x}"));
        };
        out[i] = Some(micros(d));
    }
    if out.iter().all(Option::is_none) {
        return Err(format!("{here} sets no price: give one or more of {}", FIELDS.join(", ")));
    }
    Ok(out)
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// Whether settings.json's `prices` can all be used, for Settings to
/// refuse a change that couldn't.
pub fn check(v: &Value) -> Result<()> {
    let p = Prices::with(v);
    if !p.notes.is_empty() {
        bail!("{}", p.notes.join("; "));
    }
    Ok(())
}

/// What some tokens cost: exactly, in pico-dollars, and how many tokens
/// had no price to cost them at.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Spend {
    pub pico: i128,
    pub unpriced: i64,
}

impl Spend {
    /// Some tokens at a price, or `n` unpriced ones.
    pub fn add(&mut self, cost: Option<i128>, n: i64) {
        match cost {
            Some(c) => self.pico = self.pico.saturating_add(c),
            None => self.unpriced = self.unpriced.saturating_add(n),
        }
    }

    pub fn merge(&mut self, o: Spend) {
        self.pico = self.pico.saturating_add(o.pico);
        self.unpriced = self.unpriced.saturating_add(o.unpriced);
    }

    /// Rounded to the micro-dollar, half up.
    pub fn micros(&self) -> i64 {
        let m = self.pico.max(0).saturating_add(500_000) / 1_000_000;
        i64::try_from(m).unwrap_or(i64::MAX)
    }

    /// US dollars, to the micro-dollar, for the JSON.
    pub fn usd(&self) -> f64 {
        self.micros() as f64 / 1e6
    }
}

/// Micro-dollars as people read them: `$12.34`, `<$0.01`, `$1,234`,
/// `$1.2M`. The apps' `money` says the same.
pub fn money(micros: i64) -> String {
    let m = micros.max(0);
    let cents = m.saturating_add(5_000) / 10_000;
    if m == 0 {
        "$0".into()
    } else if cents == 0 {
        "<$0.01".into()
    } else if cents < 100_000 {
        format!("${}.{:02}", cents / 100, cents % 100)
    } else {
        let dollars = m.saturating_add(500_000) / 1_000_000;
        if dollars < 1_000_000 {
            format!("${}", grouped(dollars))
        } else {
            let tenths = m.saturating_add(50_000_000_000) / 100_000_000_000;
            let s = format!("{}.{}", grouped(tenths / 10), tenths % 10);
            format!("${}M", s.trim_end_matches(".0"))
        }
    }
}

/// `12345` → `12,345`.
fn grouped(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Dollars from the JSON, as `money` takes them.
pub fn usd_micros(d: f64) -> i64 {
    if d.is_finite() { (d * 1e6).round().clamp(0.0, i64::MAX as f64) as i64 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: i64 = 1_000_000;

    #[test]
    fn each_field_at_its_own_price() {
        let p = Prices::built_in();
        let opus = p.at("anthropic", "Opus 5.5", 0).unwrap();
        assert_eq!(opus, Price([4 * M, 200_000, 5 * M, 20 * M]));
        // A million of each: $4 + $0.20 + $5 + $20.
        assert_eq!(opus.cost([M, 0, 0, 0]), 4 * 10i128.pow(12));
        assert_eq!(opus.cost([0, M, 0, 0]), 2 * 10i128.pow(11));
        assert_eq!(opus.cost([0, 0, M, 0]), 5 * 10i128.pow(12));
        assert_eq!(opus.cost([0, 0, 0, M]), 20 * 10i128.pow(12));
        assert_eq!(opus.cost([M, M, M, M]), 29_200_000 * 10i128.pow(6));
        // One token of Haiku 5.5's input is a tenth of a micro-dollar, kept exactly.
        assert_eq!(p.at("anthropic", "Haiku 5.5", 0).unwrap().cost([1, 0, 0, 0]), 100_000);
        assert_eq!(p.at("openai", "GPT-6.2 Astra", 0).unwrap(), Price([8 * M, 400_000, 10 * M, 40 * M]));
        assert_eq!(p.at("google", "Gemini 4 Neon", 0).unwrap(), Price([3 * M, 150_000, 3 * M, 15 * M]));
        assert_eq!(opus.cost([-5, 0, 0, 0]), 0, "a negative count costs nothing");
    }

    #[test]
    fn every_built_in_price_is_sane() {
        for (provider, model, _, d) in BUILT_IN {
            assert!(d.iter().all(|x| x.is_finite() && (0.0..=MAX_PRICE).contains(x)), "{provider} {model}");
            // Reading from the cache never costs more than input; writing to it never less.
            assert!(d[1] <= d[0] && d[2] >= d[0], "{provider} {model}");
        }
    }

    #[test]
    fn a_model_with_no_price_is_unpriced_not_guessed() {
        let p = Prices::built_in();
        assert_eq!(p.at("anthropic", "Opus 9", 0), None);
        assert_eq!(p.at("openrouter", "Sonnet 5.5", 0), None, "another provider's price isn't Anthropic's");
        assert_eq!(p.at("ollama", "Llama 4", 0), None);
        assert_eq!(p.at("anthropic", "opus 5.5", 0), None, "names match exactly");
    }

    #[test]
    fn a_price_changes_on_its_day() {
        let p = Prices::built_in();
        assert_eq!(p.at("google", "Gemini 4 Carbon", DEC_1_2026 - 60).unwrap().0[0], 2 * M);
        assert_eq!(p.at("google", "Gemini 4 Carbon", DEC_1_2026).unwrap().0[0], 4 * M);
        assert_eq!(p.at("google", "Gemini 3.8 Flash", JAN_1_2027 + 86400).unwrap().0[3], 7_500_000);
        assert_eq!(p.at("google", "Gemini 3.8 Flash", 0).unwrap().0[3], 3_750_000);
    }

    #[test]
    fn settings_change_a_field_or_add_a_model() {
        let p = Prices::with(&json!({
            "anthropic": { "Opus 5.5": { "cache_write": 8 } },
            "ollama": { "Llama 4": { "input": 0, "cache_read": 0, "cache_write": 0, "output": 0 } },
            "openrouter": { "Sonnet 5.5": { "input": 2.1, "cache_read": 0.105, "cache_write": 2.625, "output": 10.5 } },
            "google": { "Gemini 4 Carbon": { "output": 12 } },
        }));
        assert!(p.notes.is_empty(), "{:?}", p.notes);
        // The 1-hour TTL's writes, the rest still built in.
        assert_eq!(p.at("anthropic", "Opus 5.5", 0), Some(Price([4 * M, 200_000, 8 * M, 20 * M])));
        assert_eq!(p.source("anthropic", "Opus 5.5"), "settings");
        assert_eq!(p.source("anthropic", "Sonnet 5.5"), "built-in");
        assert_eq!(p.at("ollama", "Llama 4", 0), Some(Price([0; 4])), "free is a price");
        assert_eq!(p.at("openrouter", "Sonnet 5.5", 0), Some(Price([2_100_000, 105_000, 2_625_000, 10_500_000])));
        // A field set over a dated price is set at every date.
        assert_eq!(p.at("google", "Gemini 4 Carbon", 0).unwrap().0, [2 * M, 100_000, 2 * M, 12 * M]);
        assert_eq!(p.at("google", "Gemini 4 Carbon", DEC_1_2026).unwrap().0, [4 * M, 200_000, 4 * M, 12 * M]);
    }

    #[test]
    fn bad_prices_are_refused_and_never_break_the_rest() {
        let p = Prices::with(&json!({
            "anthropic": {
                "Opus 5.5": { "input": -1 },
                "Sonnet 5.5": { "output": 1e9 },
                "Haiku 5.5": { "input": "cheap" },
                "Fable 5.2": { "cached": 0.2 },
                "Opus 5": {},
                "Haiku 4.5": { "output": 6 },
            },
            "ollama": { "Llama 4": { "input": 1 }, "Qwen 3": 4 },
            "mistral": "free",
        }));
        assert_eq!(p.notes.len(), 8, "{:#?}", p.notes);
        let all = p.notes.join("\n");
        assert!(all.contains("prices.anthropic[\"Opus 5.5\"].input is dollars per million tokens, from 0 to 10000, not -1"), "{all}");
        assert!(all.contains("prices.anthropic[\"Fable 5.2\"] has cached, which isn't a price"), "{all}");
        assert!(all.contains("prices.ollama[\"Llama 4\"] has no built-in price, so its price needs all of input, cache_read, cache_write, output"), "{all}");
        assert!(all.contains("prices.mistral is an object of models, not a string"), "{all}");
        // What was refused keeps its built-in price; what was fine is used.
        assert_eq!(p.at("anthropic", "Opus 5.5", 0).unwrap().0[0], 4 * M);
        assert_eq!(p.at("anthropic", "Haiku 4.5", 0).unwrap().0[3], 6 * M);
        assert_eq!(p.at("ollama", "Llama 4", 0), None);
        assert!(check(&json!({ "anthropic": { "Opus 5.5": { "input": -1 } } })).is_err());
        assert!(check(&json!({ "anthropic": { "Opus 5.5": { "input": 4.5 } } })).is_ok());
        assert!(check(&Value::Null).is_ok(), "nothing set");
        assert!(check(&json!([1])).is_err());
        let long = "x".repeat(MAX_NAME + 1);
        assert!(check(&json!({ "anthropic": { long: { "input": 1 } } })).is_err());
    }

    #[test]
    fn sums_are_exact_and_rounded_once() {
        let haiku = Prices::built_in().at("anthropic", "Haiku 5.5", 0).unwrap();
        // Ten minutes of one input token each: a tenth of a micro-dollar
        // each, a micro-dollar together. Rounding each would make it zero.
        let mut s = Spend::default();
        for _ in 0..10 {
            s.add(Some(haiku.cost([1, 0, 0, 0])), 1);
        }
        assert_eq!(s.micros(), 1);
        assert_eq!(Spend { pico: 499_999, unpriced: 0 }.micros(), 0);
        assert_eq!(Spend { pico: 500_000, unpriced: 0 }.micros(), 1, "half up");
        s.add(None, 7);
        assert_eq!((s.micros(), s.unpriced), (1, 7));
        // Billions of tokens at the dearest price don't overflow.
        let dear = Price([micros(MAX_PRICE); 4]);
        let mut big = Spend::default();
        for _ in 0..1000 {
            big.add(Some(dear.cost([i64::MAX; 4])), i64::MAX);
        }
        assert!(big.pico > 0 && big.micros() == i64::MAX);
        big.add(None, i64::MAX);
        big.add(None, i64::MAX);
        assert_eq!(big.unpriced, i64::MAX);
    }

    #[test]
    fn money_reads_like_the_apps() {
        assert_eq!(money(0), "$0");
        assert_eq!(money(4_999), "<$0.01");
        assert_eq!(money(5_000), "$0.01", "half a cent rounds up");
        assert_eq!(money(12_345_678), "$12.35");
        assert_eq!(money(999_994_999), "$999.99");
        assert_eq!(money(999_995_000), "$1,000");
        assert_eq!(money(1_234_567_890), "$1,235");
        assert_eq!(money(999_999_499_999), "$999,999");
        assert_eq!(money(1_000_000_000_000), "$1M");
        assert_eq!(money(1_250_000_000_000), "$1.3M");
        assert_eq!(money(-5), "$0");
        assert_eq!(usd_micros(12.345678), 12_345_678);
        assert_eq!(usd_micros(f64::NAN), 0);
        assert_eq!(usd_micros(-1.0), 0);
    }
}
