import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the app with hot reload and sends /api to a hub:
// yours (`wh serve`, on 127.0.0.1:7500), or another with WH_HUB, such as
// the made-up world (`WH_HUB=http://127.0.0.1:7438 npm run dev`, after
// `scripts/screenshots.sh --serve`), so trying a change never touches
// your own dockets or settings.
const hub = process.env.WH_HUB ?? "http://127.0.0.1:7500";

export default defineConfig({
  plugins: [react()],
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
