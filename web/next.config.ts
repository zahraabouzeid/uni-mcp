import type { NextConfig } from "next";
import { PHASE_DEVELOPMENT_SERVER } from "next/constants";

// Where `unimcp dashboard --dev` listens.
const DEV_BACKEND = "http://127.0.0.1:4517";

export default function config(phase: string): NextConfig {
  const dev = phase === PHASE_DEVELOPMENT_SERVER;
  return {
    // A build exports static files to `out/`, which `unimcp` embeds. Rewrites
    // are not allowed in an export, so they exist only in dev.
    output: dev ? undefined : "export",
    rewrites: dev
      ? async () => [{ source: "/api/:path*", destination: `${DEV_BACKEND}/api/:path*` }]
      : undefined,
    turbopack: {
      root: __dirname,
      rules: {
        "*.css": {
          loaders: ["@tailwindcss/turbopack"],
          as: "*.css",
        },
      },
    },
  };
}
