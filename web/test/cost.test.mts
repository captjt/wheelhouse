// What Recap's tokens cost, as the app says it: the same words as the
// hub's Markdown (src/pricing.rs) and the Mac app.
import assert from "node:assert/strict";
import { test } from "node:test";
import { costOf, micros, money, moneyDelta, priceWords } from "../src/cost.ts";

test("money reads like the hub's Markdown", () => {
  assert.equal(money(0), "$0");
  assert.equal(money(null), "$0");
  assert.equal(money(0.004999), "<$0.01");
  assert.equal(money(0.005), "$0.01", "half a cent rounds up");
  assert.equal(money(1.005), "$1.01", "in whole micro-dollars, not floats");
  assert.equal(money(12.345678), "$12.35");
  assert.equal(money(999.994999), "$999.99");
  assert.equal(money(999.995), "$1,000");
  assert.equal(money(1234.56789), "$1,235");
  assert.equal(money(999_999.499999), "$999,999");
  assert.equal(money(1_000_000), "$1M");
  assert.equal(money(1_250_000), "$1.3M");
  assert.equal(money(-3), "$0");
  assert.equal(money(Number.NaN), "$0");
  assert.equal(money(Number.POSITIVE_INFINITY), "$0");
  assert.equal(micros(0.00006), 60);
});

test("a cost's change, and a count's cost", () => {
  assert.equal(moneyDelta(59.2, 2), "+$57.20");
  assert.equal(moneyDelta(2, 59.2), "−$57.20");
  assert.equal(moneyDelta(1, 1), "±$0");
  assert.equal(moneyDelta(1, undefined), "+$1.00");
  assert.equal(costOf({ tokens: 10, cost: 1.5, unpriced: 0 }), "$1.50");
  assert.equal(costOf({ tokens: 10, cost: 1.5, unpriced: 4 }), "$1.50", "what's priced is costed");
  assert.equal(costOf({ tokens: 10, cost: 0, unpriced: 10 }), "unpriced");
  assert.equal(costOf({ tokens: 10, cost: null }), "unpriced", "a model with no price");
  assert.equal(costOf({ tokens: 0, cost: 0, unpriced: 0 }), "$0");
});

test("a model's price, for its tooltip", () => {
  assert.equal(
    priceWords({ input: 4, cache_read: 0.2, cache_write: 5, output: 20, source: "built-in" }),
    "Per million tokens: $4 input, $0.2 from the cache, $5 to the cache, $20 output",
  );
  assert.match(priceWords({ input: 0.075, cache_read: 0, cache_write: 0, output: 1, source: "settings" }), /\$0\.075 input.*\(from settings\.json\)$/);
  assert.equal(priceWords(null), "No price: add one under prices in settings.json");
});
