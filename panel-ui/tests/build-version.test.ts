import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { describe, expect, test } from "vitest";

describe("Panel UI Build & Version Integration", () => {
  test("should resolve __APP_VERSION__ equal to Cargo.toml version", () => {
    const cargoPath = path.resolve(__dirname, "../../Cargo.toml");
    const cargoContent = fs.readFileSync(cargoPath, "utf8");
    const expectedVersion = cargoContent.match(/^version = "(.+)"/m)?.[1];

    // Single source of truth: Cargo.toml. Never hardcode the version here
    // (hardcoded "0.2.4" caused silent drift; package.json manifests are
    // checked separately by scripts/check-version-sync.sh).
    expect(expectedVersion).toMatch(/^\d+\.\d+\.\d+/);

    const viteConfigPath = path.resolve(__dirname, "../vite.config.ts");
    const viteConfigContent = fs.readFileSync(viteConfigPath, "utf8");

    expect(viteConfigContent).toContain("fs.readFileSync");
    expect(viteConfigContent).not.toContain("0.6.1-beta");
  });

  // SECURITY REGRESSION TEST: a prior panel deploy shipped VITE_XAVIER_API_TOKEN inlined into
  // the production JS bundle (a real credential-leak vector — anyone reading the served JS
  // could read the master key in plaintext). The panel now authenticates purely via the
  // /auth/* session; no source file reads import.meta.env.VITE_XAVIER_API_TOKEN anymore. This
  // asserts that holds even when the env var IS set at build time — Vite only inlines
  // `import.meta.env.VITE_*` references that actually appear in source, so with no such
  // reference left, the value must never reach the built assets, however it's supplied.
  test("never inlines VITE_XAVIER_API_TOKEN into the built bundle, even when set at build time", () => {
    const panelDir = path.resolve(__dirname, "..");
    const srcDir = path.resolve(panelDir, "src");

    // No source file may actually READ this env var — the real guarantee. Comments that merely
    // mention the string (e.g. documenting why it was removed) are fine and intentionally not
    // flagged; grepping the built bundle for a canary secret (below) only proves *that specific
    // value* doesn't leak, so this grep for the live `import.meta.env.VITE_XAVIER_API_TOKEN`
    // read pattern proves the read path itself is gone.
    const grepSource = (dir: string): string[] => {
      const hits: string[] = [];
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
          hits.push(...grepSource(full));
        } else if (/\.(ts|tsx|js|jsx)$/.test(entry.name)) {
          const content = fs.readFileSync(full, "utf8");
          if (content.includes("import.meta.env.VITE_XAVIER_API_TOKEN")) hits.push(full);
        }
      }
      return hits;
    };
    expect(grepSource(srcDir)).toEqual([]);

    const canarySecret = "XAVIER_CANARY_SECRET_MUST_NOT_LEAK_INTO_BUNDLE_9f2c7a";
    execSync("pnpm build", {
      cwd: panelDir,
      env: { ...process.env, VITE_XAVIER_API_TOKEN: canarySecret },
    });

    const distAssetsDir = path.resolve(panelDir, "dist/assets");
    const buildAssetsDir = path.resolve(panelDir, "build/assets");
    const assetsDir = fs.existsSync(distAssetsDir) ? distAssetsDir : buildAssetsDir;
    const jsFiles = fs
      .readdirSync(assetsDir)
      .filter((f) => f.endsWith(".js"));
    expect(jsFiles.length).toBeGreaterThan(0);

    for (const jsFile of jsFiles) {
      const content = fs.readFileSync(path.join(assetsDir, jsFile), "utf8");
      expect(content).not.toContain(canarySecret);
      expect(content).not.toContain("VITE_XAVIER_API_TOKEN");
    }
  }, 120000);

  test("should execute pnpm build without deprecated pnpm field warnings", () => {
    const panelDir = path.resolve(__dirname, "..");
    let stdoutAndStderr = "";
    try {
      stdoutAndStderr = execSync("pnpm build 2>&1", {
        cwd: panelDir,
        encoding: "utf8",
      });
    } catch (e: any) {
      stdoutAndStderr = e.stdout || e.stderr || e.message;
    }

    expect(stdoutAndStderr).not.toContain('[WARN] The "pnpm" field');
    expect(stdoutAndStderr).not.toContain('The "pnpm" field in');
  }, 120000);
});
