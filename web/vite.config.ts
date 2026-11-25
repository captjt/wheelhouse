import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the app with hot reload and sends /api to a hub:
// yours (`wh serve`, on 127.0.0.1:7500), or another with WH_HUB, such as
// the made-up world (`WH_HUB=http://127.0.0.1:7438 npm run dev`, after
// `scripts/screenshots.sh --serve`), so trying a change never touches
// your own dockets or settings.
const hub = process.env.WH_HUB ?? "http://127.0.0.1:7500";

/**
 * The Changes view's highlighter loads its grammars from shiki/langs. The
 * page resolves them and hands them to the worker, so the page gets only
 * the ones the hub carries (src/shiki-langs.ts, not all 200-odd) and the
 * worker none (src/shiki-langs-none.ts). Shiki's WebAssembly engine stays
 * out (src/shiki-wasm-none.ts): the JavaScript one is the default.
 */
const shikiLangs = (forWorker: boolean): Plugin => ({
  name: "wheelhouse-shiki-langs",
  enforce: "pre",
  resolveId(source) {
    const local = (f: string) => fileURLToPath(new URL(`./src/${f}`, import.meta.url));
    if (source === "shiki/langs") return local(forWorker ? "shiki-langs-none.ts" : "shiki-langs.ts");
    // Its WebAssembly engine, which it uses only when asked for by name.
    if (source === "shiki/wasm" || source === "shiki/engine/oniguruma") return local("shiki-wasm-none.ts");
    return null;
  },
});

export default defineConfig({
  plugins: [shikiLangs(false), react()],
  // The highlighter's worker, split, so it shares the page's chunks.
  worker: { format: "es", plugins: () => [shikiLangs(true)] },
  server: {
    port: Number(process.env.PORT ?? 5180),
    // The hub takes a write only from its own origin: it checks the page's
    // Origin against the host it was asked for, so the proxy says which
    // host that was (this server's), as `x-forwarded-host`.
    proxy: {
      "/api": {
        target: hub,
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on("proxyReq", (out, req) => {
            if (req.headers.host) out.setHeader("x-forwarded-host", req.headers.host);
          });
        },
      },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
