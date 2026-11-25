import Foundation

/// What tokens cost, as Recap shows it: the hub sends US dollars to the
/// micro-dollar (src/pricing.rs), and these say them the way its Markdown
/// and the web app (web/src/cost.ts) do.
enum Cost {
    /// Dollars as whole micro-dollars, so rounding to the cent never drifts
    /// with floats. Nothing, a negative or a non-number is zero.
    static func micros(_ dollars: Double?) -> Int64 {
        guard let d = dollars, d.isFinite, d > 0 else { return 0 }
        let m = (d * 1e6).rounded()
        return m >= Double(Int64.max / 2) ? Int64.max / 2 : Int64(m)
    }

    /// `$12.34`, `<$0.01`, `$1,234`, `$1.2M`: the same as `pricing::money`.
    static func money(_ dollars: Double?) -> String {
        money(micros: micros(dollars))
    }

    static func money(micros m: Int64) -> String {
        let m = max(0, m)
        let cents = (m + 5_000) / 10_000
        if m == 0 { return "$0" }
        if cents == 0 { return "<$0.01" }
        if cents < 100_000 { return "$\(cents / 100).\(String(format: "%02lld", cents % 100))" }
        let whole = (m + 500_000) / 1_000_000
        if whole < 1_000_000 { return "$\(grouped(whole))" }
        let tenths = (m + 50_000_000_000) / 100_000_000_000
        let s = "\(grouped(tenths / 10)).\(tenths % 10)"
        return "$\(s.hasSuffix(".0") ? String(s.dropLast(2)) : s)M"
    }

    private static func grouped(_ n: Int64) -> String {
        let s = String(n)
        var out = ""
        for (i, c) in s.enumerated() {
            if i > 0 && (s.count - i) % 3 == 0 { out.append(",") }
            out.append(c)
        }
        return out
    }

    /// A cost's change, as the tiles show one: `+$1.20`, `−$0.40`, `±$0`.
    static func delta(_ now: Double?, _ before: Double?) -> String {
        let d = micros(now) - micros(before)
        return "\(d > 0 ? "+" : d < 0 ? "−" : "±")\(money(micros: abs(d)))"
    }

    /// A count's cost: `$1.23`, or `unpriced` when none of its tokens has a
    /// price. What has a price is costed; the rest is said beside it.
    static func of(_ x: JSON) -> String {
        let tokens = x["tokens"].int ?? 0, unpriced = x["unpriced"].int ?? 0
        guard let cost = x["cost"].double, !(tokens > 0 && unpriced >= tokens) else { return "unpriced" }
        return money(cost)
    }

    /// A model's price, for its tooltip: dollars per million tokens of each kind.
    static func priceWords(_ p: JSON) -> String {
        guard p.exists else { return "No price: add one under prices in settings.json" }
        func d(_ k: String) -> String {
            let v = ((p[k].double ?? 0) * 1e6).rounded() / 1e6
            guard v.isFinite, abs(v) < 1e12 else { return "$?" }
            return "$\(v == v.rounded() ? String(Int64(v)) : String(v))"
        }
        let from = p["source"].str == "settings" ? " (from settings.json)" : ""
        // Only Anthropic's 1-hour cache costs more to write to than its 5-minute one.
        let hour = p["cache_write_1h"].double.map { $0 != (p["cache_write"].double ?? 0) } == true ? ", \(d("cache_write_1h")) to the 1-hour cache" : ""
        return "Per million tokens: \(d("input")) input, \(d("cache_read")) from the cache, \(d("cache_write")) to the cache\(hour), \(d("output")) output\(from)"
    }

    /// Nothing has a price: every token is unpriced, so there's no cost to
    /// give, not a cost of $0.
    static func nothingPriced(_ u: JSON) -> Bool {
        let off = u["unpriced"]["tokens"].int ?? 0
        return off > 0 && off >= (u["tokens"].int ?? 0)
    }

    /// The cost in the Tokens line: `$12.34 at API prices`, or `unpriced,
    /// 5M tokens with no price`.
    static func words(_ u: JSON, count: (Int) -> String) -> String? {
        if nothingPriced(u) { return "unpriced, \(count(u["unpriced"]["tokens"].int ?? 0)) tokens with no price" }
        return u["cost"].double.map { "\(money($0)) at API prices" }
    }
}
