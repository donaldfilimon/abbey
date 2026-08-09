import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Abbey desktop frontend.
//
// Two constraints are load-bearing and enforced downstream by
// `scripts/verify-bundle.mjs`:
//
//   * every asset is emitted locally — nothing is fetched from a CDN, so the
//     strict `script-src 'self'` CSP declared in `src-tauri/tauri.conf.json`
//     can hold without exceptions;
//   * no asset is inlined as a `data:` URI, so `script-src` never needs
//     `'unsafe-inline'`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // Tauri 2 on macOS runs WKWebView; Linux/Windows use WebKitGTK/WebView2.
    target: ["es2022", "safari15"],
    assetsInlineLimit: 0,
    sourcemap: false,
    emptyOutDir: true,
  },
});
