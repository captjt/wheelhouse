import Foundation
import Testing
@testable import Wheelhouse

// What Recap's tokens cost, as the app says it: the same words as the
// hub's Markdown (src/pricing.rs) and the web app (web/src/cost.ts).

private func json(_ s: String) throws -> JSON { try JSON.parse(Data(s.utf8)) }

@Test func moneyReadsLikeTheHub() {
    #expect(Cost.money(0) == "$0")
    #expect(Cost.money(nil) == "$0")
    #expect(Cost.money(0.004999) == "<$0.01")
    #expect(Cost.money(0.005) == "$0.01")
    #expect(Cost.money(1.005) == "$1.01")
    #expect(Cost.money(12.345678) == "$12.35")
    #expect(Cost.money(999.994999) == "$999.99")
    #expect(Cost.money(999.995) == "$1,000")
    #expect(Cost.money(1234.56789) == "$1,235")
    #expect(Cost.money(999_999.499999) == "$999,999")
    #expect(Cost.money(1_000_000) == "$1M")
    #expect(Cost.money(1_250_000) == "$1.3M")
    #expect(Cost.money(-3) == "$0")
    #expect(Cost.money(.nan) == "$0")
    #expect(Cost.money(.infinity) == "$0")
    #expect(Cost.money(1e300) != "", "huge never traps")
    #expect(Cost.micros(0.00006) == 60)
}

@Test func aCostsChangeAndACountsCost() throws {
    #expect(Cost.delta(59.2, 2) == "+$57.20")
    #expect(Cost.delta(2, 59.2) == "−$57.20")
    #expect(Cost.delta(1, 1) == "±$0")
    #expect(Cost.delta(1, nil) == "+$1.00")
    #expect(Cost.of(try json(#"{"tokens": 10, "cost": 1.5, "unpriced": 0}"#)) == "$1.50")
    #expect(Cost.of(try json(#"{"tokens": 10, "cost": 1.5, "unpriced": 4}"#)) == "$1.50")
    #expect(Cost.of(try json(#"{"tokens": 10, "cost": 0, "unpriced": 10}"#)) == "unpriced")
    #expect(Cost.of(try json(#"{"tokens": 10, "cost": null}"#)) == "unpriced")
    #expect(Cost.of(try json(#"{"tokens": 0, "cost": 0, "unpriced": 0}"#)) == "$0")
}

@Test func aModelsPriceForItsTooltip() throws {
    #expect(Cost.priceWords(try json(#"{"input": 4, "cache_read": 0.2, "cache_write": 5, "output": 20, "source": "built-in"}"#))
        == "Per million tokens: $4 input, $0.2 from the cache, $5 to the cache, $20 output")
    #expect(Cost.priceWords(try json(#"{"input": 0.075, "cache_read": 0, "cache_write": 0, "output": 1, "source": "settings"}"#)).hasSuffix("$1 output (from settings.json)"))
    #expect(Cost.priceWords(.null) == "No price: add one under prices in settings.json")
}
