import fs from "node:fs";
import path from "node:path";
import { describe, expect, test } from "vitest";

describe("Cloudflare Pages Deployment Configuration", () => {
  const panelDir = path.resolve(__dirname, "..");

  test("wrangler.toml exists with valid Cloudflare Pages configuration", () => {
    const wranglerPath = path.join(panelDir, "wrangler.toml");
    expect(fs.existsSync(wranglerPath)).toBe(true);

    const content = fs.readFileSync(wranglerPath, "utf8");
    expect(content).toContain('name = "xavier-panel"');
    expect(content).toContain('compatibility_date = "2026-09-01"');
    expect(content).toContain('pages_build_output_dir = "dist"');
  });

  test("public/_redirects exists with SPA fallback rewrite rule", () => {
    const redirectsPath = path.join(panelDir, "public/_redirects");
    expect(fs.existsSync(redirectsPath)).toBe(true);

    const content = fs.readFileSync(redirectsPath, "utf8");
    expect(content.trim()).toContain("/*  /index.html  200");
  });

  test("dist/_redirects exists in build output after build", () => {
    const distRedirectsPath = path.join(panelDir, "dist/_redirects");
    expect(fs.existsSync(distRedirectsPath)).toBe(true);

    const content = fs.readFileSync(distRedirectsPath, "utf8");
    expect(content.trim()).toContain("/*  /index.html  200");
  });
});
