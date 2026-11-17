import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the app with hot reload and sends /api to the hub
// (`wh serve`, on 127.0.0.1:7500).
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5180,
    proxy: { "/api": { target: "http://127.0.0.1:7500", changeOrigin: true } },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
