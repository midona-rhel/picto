import { defineConfig } from "vite";
import { resolve } from "path";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig(async ({ command }) => ({
  // Packaged Electron loads from file://, so production assets must be relative.
  base: command === "serve" ? "/" : "./",
  plugins: [react()],
  resolve: {
    alias: {
      "#desktop": resolve(__dirname, "src/desktop"),
      "#features": resolve(__dirname, "src/features"),
      "#ui": resolve(__dirname, "src/components/ui"),
    },
  },

  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        detail: resolve(__dirname, "detail.html"),
        settings: resolve(__dirname, "settings.html"),
        subscriptions: resolve(__dirname, "subscriptions.html"),
        "library-manager": resolve(__dirname, "library-manager.html"),
      },
    },
  },

  // Dev server for Electron renderer process.
  clearScreen: false,
  server: {
    port: 8080,
    strictPort: true,
  },
}));
