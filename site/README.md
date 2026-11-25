# Wheelhouse's docs site

The product docs at [wheelhouse.0utlaw.xyz](https://wheelhouse.0utlaw.xyz):
[Astro](https://astro.build) with [Starlight](https://starlight.astro.build),
themed like the app (Cloudflare's Kumo colors, Inter, System/Light/Dark).
Static files, served by Cloudflare Pages.

```sh
just site           # hot reload on http://localhost:4321
just check-site     # types, and a build that fails on any warning (part of just check)
```

Node 22.18 or later (`.nvmrc` says which).

## Where things are

| | |
| --- | --- |
| `src/content/docs/` | The pages. The path is the URL: `features/review.mdx` is `/features/review/`. |
| `astro.config.mjs` | The sidebar and the site's URL |
| `src/integrations/csp.mjs` | Each page's Content Security Policy, from its own inline scripts and styles |
| `src/styles/theme.css` | Kumo's colors laid onto Starlight's, light and dark |
| `src/components/Shot.astro` | A screenshot from `../docs/images`, the right one for the theme |
| `public/_headers` | Cloudflare Pages' response headers |

Screenshots aren't copied: `<Shot name="mac-inbox" alt="…" />` reads
`docs/images/mac-inbox-light.png` and `-dark.png`, the README's own, so
`just screenshots` updates both. A missing name fails the build.

The pages are written for people using Wheelhouse. The repo's `docs/*.md`
and `DESIGN.md` stay the design and the protocol; when a feature changes,
change both.

## Cloudflare Pages

A Pages project connected to `captjt/wheelhouse`:

| Setting | Value |
| --- | --- |
| Production branch | `main` |
| Root directory | `site` |
| Build command | `npm ci && npm run build` |
| Build output directory | `dist` |
| Environment variable | `NODE_VERSION` = `22.18.0` (or later) |
| Custom domain | `wheelhouse.0utlaw.xyz` |

Pages clones the whole repo, so `../docs/images` is there at build time.
Under **Build watch paths**, include `site/*` and `docs/images/*`, so a
change to the Rust or the Mac app doesn't rebuild the site.

The custom domain: in the Pages project, **Custom domains → Set up a
domain**, `wheelhouse.0utlaw.xyz`. With `0utlaw.xyz` on Cloudflare DNS,
Pages adds the CNAME itself.

Headers: after the build, `src/integrations/csp.mjs` writes a `<meta>` CSP
into every page with a hash of each of its inline scripts and styles, so a
script nobody wrote can't run. `public/_headers` adds `frame-ancestors`, HSTS
and the rest, which a `<meta>` can't carry.
