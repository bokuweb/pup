import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `../public` holds the published data (`pup build`) and wiki JSON (`pup wiki-export`);
// it is copied into the build as-is and fetched with relative URLs, so the app works under /pup/.
export default defineConfig({
  base: "./",
  publicDir: "../public",
  plugins: [react()],
});
