// Screenshots are found by name (src/lib/shots.ts), so Vite bundles every
// capture in docs/images, lazy or not: the ones no page shows ship too.
// After the build, this removes each image in _astro that no built file
// (a page, a stylesheet, a script) names.

import { readdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

async function* files(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* files(path);
    else yield path;
  }
}

const IMAGE = /\.(png|jpe?g|webp|avif|gif)$/i;
const TEXT = /\.(html|css|js|mjs|json|xml|txt)$/i;

export default function prune() {
  return {
    name: "wheelhouse:prune",
    hooks: {
      "astro:build:done": async ({ dir, logger }) => {
        const root = fileURLToPath(dir);
        const assets = join(root, "_astro");
        const images = (await readdir(assets).catch(() => [])).filter((f) => IMAGE.test(f));
        if (images.length === 0) return;
        const named = new Set();
        for await (const file of files(root)) {
          if (!TEXT.test(file)) continue;
          const text = await readFile(file, "utf8");
          for (const image of images) if (text.includes(image)) named.add(image);
        }
        const unused = images.filter((f) => !named.has(f));
        await Promise.all(unused.map((f) => rm(join(assets, f))));
        if (unused.length) logger.info(`removed ${unused.length} images no page shows`);
      },
    },
  };
}
