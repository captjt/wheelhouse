// @ts-check
import { fileURLToPath } from "node:url";
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import csp from "./src/integrations/csp.mjs";
import prune from "./src/integrations/prune.mjs";

// The repo's root: screenshots come from ../docs/images, one source for the
// README and the site.
const repo = fileURLToPath(new URL("..", import.meta.url));

export default defineConfig({
  site: "https://wheelhouse.0utlaw.xyz",
  trailingSlash: "ignore",
  // Starlight highlights code with Expressive Code, not Astro's Shiki.
  markdown: { syntaxHighlight: false },
  vite: {
    server: { fs: { allow: [repo] } },
  },
  integrations: [
    starlight({
      title: "Wheelhouse",
      disable404Route: true,
      description:
        "Every piece of agent work as a docket, on a record you can check. Talk to your agents, review PRs, build issues, and check what happened, from your Mac or your phone.",
      favicon: "/favicon.svg",
      // In the phone's menu; the header draws its own link on wider screens.
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/captjt/wheelhouse" }],
      customCss: ["@fontsource-variable/inter/opsz.css", "./src/styles/tokens.css", "./src/styles/theme.css"],
      components: {
        Header: "./src/components/Header.astro",
        SiteTitle: "./src/components/SiteTitle.astro",
        PageTitle: "./src/components/PageTitle.astro",
        Footer: "./src/components/Footer.astro",
      },
      expressiveCode: {
        themes: ["github-dark-default", "github-light-default"],
        styleOverrides: {
          borderRadius: "8px",
          borderColor: "var(--sl-color-hairline)",
          codeFontFamily: "var(--__sl-font-mono)",
          uiFontFamily: "var(--__sl-font)",
          frames: { shadowColor: "transparent" },
        },
      },
      head: [
        { tag: "meta", attrs: { property: "og:image", content: "https://wheelhouse.0utlaw.xyz/social-preview.png" } },
        { tag: "meta", attrs: { name: "twitter:card", content: "summary_large_image" } },
      ],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "What is Wheelhouse", slug: "start/introduction" },
            { label: "Install", slug: "start/install" },
            { label: "A day with it", slug: "start/tour" },
          ],
        },
        {
          label: "Concepts",
          items: [
            { label: "Dockets", slug: "concepts/dockets" },
            { label: "The record", slug: "concepts/record" },
            { label: "Agents on a docket", slug: "concepts/agents" },
            { label: "Rex and Herdr", slug: "concepts/runtimes" },
          ],
        },
        {
          label: "Protocols",
          items: [
            { label: "Inbox and Chat", slug: "features/inbox-chat" },
            { label: "Review", slug: "features/review" },
            { label: "Build", slug: "features/build" },
            { label: "Verify", slug: "features/verify" },
            { label: "Recap", slug: "features/recap" },
            { label: "Calls", slug: "features/calls" },
            { label: "Images", slug: "features/images" },
          ],
        },
        {
          label: "Apps",
          items: [
            { label: "Wheelhouse.app", slug: "apps/mac" },
            { label: "Web and phone", slug: "apps/web-phone" },
            { label: "Keyboard shortcuts", slug: "apps/shortcuts" },
          ],
        },
        {
          label: "Teams",
          items: [
            { label: "How teams work", slug: "teams/overview" },
            { label: "Start a workspace", slug: "teams/start" },
            { label: "A workspace hub on GCP", slug: "teams/gcp" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "The wh CLI", slug: "reference/cli" },
            { label: "Settings", slug: "reference/settings" },
            { label: "Configuration", slug: "reference/configuration" },
            { label: "Security", slug: "reference/security" },
          ],
        },
      ],
    }),
    // After Starlight: drop the captures no page shows, then a CSP per page,
    // from its own inline scripts and styles.
    prune(),
    csp(),
  ],
});
