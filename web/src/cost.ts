// What tokens cost, as Recap shows it: the hub sends US dollars to the
// micro-dollar (src/pricing.rs), and these say them the way its Markdown
// and the Mac app do.

/** Dollars as whole micro-dollars, so rounding to the cent never drifts with floats. */
export function micros(dollars?: number | null): number {
  if (dollars == null || !Number.isFinite(dollars) || dollars <= 0) return 0;
  return Math.min(Math.round(dollars * 1e6), Number.MAX_SAFE_INTEGER);
}

/** `$12.34`, `<$0.01`, `$1,234`, `$1.2M`: the same as `pricing::money`. */
export function money(dollars?: number | null): string {
  const m = micros(dollars);
  const cents = Math.floor((m + 5_000) / 10_000);
  if (m === 0) return "$0";
  if (cents === 0) return "<$0.01";
  if (cents < 100_000) return `$${Math.floor(cents / 100)}.${String(cents % 100).padStart(2, "0")}`;
  const whole = Math.floor((m + 500_000) / 1_000_000);
  if (whole < 1_000_000) return `$${grouped(whole)}`;
  const tenths = Math.floor((m + 50_000_000_000) / 100_000_000_000);
  return `$${`${grouped(Math.floor(tenths / 10))}.${tenths % 10}`.replace(/\.0$/, "")}M`;
}

function grouped(n: number): string {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

/** A cost's change, as the tiles show one: `+$1.20`, `−$0.40`, `±$0`. */
export function moneyDelta(now?: number | null, before?: number | null): string {
  const d = micros(now) - micros(before);
  return `${d > 0 ? "+" : d < 0 ? "−" : "±"}${money(Math.abs(d) / 1e6)}`;
}

/** A model's price, for its tooltip: dollars per million tokens of each kind. */
export type Price = { input: number; cache_read: number; cache_write: number; output: number; source: string };

export function priceWords(p?: Price | null): string {
  if (!p) return "No price: add one under prices in settings.json";
  const d = (n: number) => `$${Number(n.toFixed(6))}`;
  return `Per million tokens: ${d(p.input)} input, ${d(p.cache_read)} from the cache, ${d(p.cache_write)} to the cache, ${d(p.output)} output${p.source === "settings" ? " (from settings.json)" : ""}`;
}

/**
 * A count's cost: `$1.23`, or `unpriced` when none of its tokens has a
 * price. What has a price is costed; the rest is said beside it.
 */
export function costOf(x: { tokens: number; cost?: number | null; unpriced?: number | null }): string {
  if (x.cost == null || (x.tokens > 0 && (x.unpriced ?? 0) >= x.tokens)) return "unpriced";
  return money(x.cost);
}
