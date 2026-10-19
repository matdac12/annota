import { defineConfig } from "vitest/config";

// Tauri loads the dev server from this exact port (src-tauri/tauri.conf.json, build.devUrl).
export default defineConfig({
  clearScreen: false,
  server: { host: "127.0.0.1", port: 3180, strictPort: true },
  build: { target: "es2022" },
  test: { environment: "jsdom", include: ["test/**/*.test.ts"] },
});
