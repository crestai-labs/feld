import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const daemon = process.env.FELD_API || "http://127.0.0.1:7381";

// The daemon embeds `dist/` as it is committed (see README.md), so the script and the
// stylesheet keep fixed file names: a new build replaces the same files instead of adding
// hashed ones. Images carry their content hash, so a new image is a new URL to a browser
// that holds the old one.
export default defineConfig({
  base: "/ui/",
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    assetsInlineLimit: 0,
    // The page is one script served from the local daemon, so its size warns of nothing.
    chunkSizeWarningLimit: 1024,
    rolldownOptions: {
      output: {
        entryFileNames: "app.js",
        chunkFileNames: "[name].js",
        assetFileNames: (asset) => (asset.names.some((name) => name.endsWith(".css")) ? "app.css" : "assets/[name]-[hash][extname]"),
      },
    },
  },
  // `npm run dev` serves the page from source and passes the daemon API through to the
  // daemon `FELD_API` names. The daemon answers its own address alone, so the proxy sends
  // the requests there, and the `Origin` of this page as the daemon's.
  server: {
    proxy: {
      "/v1": {
        target: daemon,
        changeOrigin: true,
        configure: (proxy) =>
          proxy.on("proxyReq", (request, incoming) => {
            if (incoming.headers.origin === `http://${incoming.headers.host}`) {
              request.setHeader("origin", new URL(daemon).origin);
            }
          }),
      },
    },
  },
  test: {
    include: ["**/*.test.{js,ts,tsx}"],
    exclude: ["node_modules/**", "dist/**"],
  },
});
