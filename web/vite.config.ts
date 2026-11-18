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
    proxy: { "/api": { target: hub, changeOrigin: true } },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
