# Wheelhouse's docs site

The product docs at [wheelhouse.0utlaw.xyz](https://wheelhouse.0utlaw.xyz):
[Astro](https://astro.build) with [Starlight](https://starlight.astro.build),
themed like the app (Cloudflare's Kumo colors, Inter, System/Light/Dark).
Static files, served by a Cloudflare Worker.

```sh
just site           # hot reload on http://localhost:4321
just check-site     # types, the CSP's tests, a build that fails on any warning,
                    # and every page's CSP checked (part of just check)
```

Node 22.23.4 or later, which Astro 7 needs; `.nvmrc` pins the one it's
tested on.

## Where things are

| | |
| --- | --- |
| `src/content/docs/` | The pages. The path is the URL: `features/review.mdx` is `/features/review/`. |
| `astro.config.mjs` | The sidebar and the site's URL |
| `src/integrations/csp.mjs` | Each page's Content Security Policy, from its own inline scripts and styles |
| `scripts/check-csp.mjs`, `test/` | The CSP checked after a build, and its tests |
| `src/styles/theme.css` | Kumo's colors laid onto Starlight's, light and dark |
| `src/components/Shot.astro` | A screenshot from `../docs/images`, the right one for the theme |
| `public/_headers` | The response headers Cloudflare sends with every file |
| `wrangler.jsonc` | The Worker that serves `dist/` |

Screenshots aren't copied: `<Shot name="mac-inbox" alt="…" />` reads
`docs/images/mac-inbox-light.png` and `-dark.png`, the README's own, so
`just screenshots` updates both. A missing name fails the build.

The pages are written for people using Wheelhouse. The repo's `docs/*.md`
and `DESIGN.md` stay the design and the protocol; when a feature changes,
change both.

## Cloudflare

A Workers project connected to `captjt/wheelhouse` (Workers Builds), serving
`dist/` as static assets. `wrangler.jsonc` says so; without it, `wrangler
deploy` configures the project itself, adding Astro's Cloudflare adapter and
server rendering, and the build fails.

| Setting | Value |
| --- | --- |
| Production branch | `main` |
| Root directory | `site` |
| Build command | `npm run build` (dependencies install from the lockfile first) |
| Deploy command | `npx wrangler deploy` (the `wrangler` pinned in `package.json`) |
| Node | `site/.nvmrc`, 24.22.1 (Astro 7 needs 22.23.4 or later) |
| Custom domain | `wheelhouse.0utlaw.xyz`, under the Worker's **Domains & Routes** |

The build clones the whole repo, so `../docs/images` is there at build time.
Under **Build watch paths**, include `site/*` and `docs/images/*`, so a
change to the Rust or the Mac app doesn't rebuild the site.

`npx wrangler deploy --dry-run` after a build checks the config without
deploying. A missing page gets `dist/404.html` with a 404 status, and
`/start/install` serves `/start/install/`.

Headers: after the build, `src/integrations/csp.mjs` writes a `<meta>` CSP
into every page with a hash of each of its inline scripts and styles, so a
script nobody wrote can't run. `public/_headers` adds `frame-ancestors`, HSTS
and the rest, which a `<meta>` can't carry.

Keep two Cloudflare features off for this hostname, or the CSP blocks them
and the site breaks: **Web Analytics**' automatic injection (its beacon is a
script from another origin) and **Rocket Loader** (it rewrites inline scripts,
so their hashes no longer match).
