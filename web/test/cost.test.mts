// What Recap's tokens cost, as the app says it: the same words as the
// hub's Markdown (src/pricing.rs) and the Mac app.
import assert from "node:assert/strict";
import { test } from "node:test";
import { COST_FLOOR, costOf, costWords, micros, money, moneyDelta, nothingPriced, priceWords, sparkTop } from "../src/cost.ts";

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
  assert.equal(
    priceWords({ input: 4, cache_read: 0.2, cache_write: 5, cache_write_1h: 8, output: 20, source: "built-in" }),
    "Per million tokens: $4 input, $0.2 from the cache, $5 to the cache, $8 to the 1-hour cache, $20 output",
  );
  assert.doesNotMatch(priceWords({ input: 3, cache_read: 0.15, cache_write: 3, cache_write_1h: 3, output: 15, source: "built-in" }), /1-hour/, "only when it costs more");
});

test("nothing priced is said as unpriced, never $0", () => {
  const count = (n: number) => `${n / 1e6}M`;
  assert.equal(costWords({ tokens: 5e6, cost: 0, unpriced: { tokens: 5e6 } }, count), "unpriced, 5M tokens with no price");
  assert.equal(nothingPriced({ tokens: 5e6, unpriced: { tokens: 5e6 } }), true);
  assert.equal(costWords({ tokens: 6e6, cost: 4, unpriced: { tokens: 5e6 } }, count), "$4.00 at API prices");
  assert.equal(nothingPriced({ tokens: 0, unpriced: { tokens: 0 } }), false, "no tokens at all isn't unpriced");
  assert.equal(costWords({ tokens: 10 }, count), null, "an older hub with no cost");
});

test("a cost sparkline draws a few cents a day", () => {
  // Tokens' floor of 1 would squash cents to the bottom; a micro-dollar doesn't.
  assert.equal(sparkTop([0.02, 0.05, 0.01], COST_FLOOR), 0.05);
  assert.equal(sparkTop([0, 0], COST_FLOOR), COST_FLOOR, "never a top of nothing");
  assert.equal(sparkTop([3, Number.NaN], 1), 3);
  assert.equal(sparkTop([0.5], 1), 1, "tokens keep their floor");
});
