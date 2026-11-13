import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
    watch: { ignored: ["**/src-tauri/**", "**/core/target/**"] }
  },
  build: { outDir: "dist", emptyOutDir: true }
});
