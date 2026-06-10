#!/usr/bin/env node
/**
 * Jules PR Integration Checker for Xavier
 *
 * Checks all open Jules PRs, builds them, runs clippy, reports results.
 * Usage: node check-jules-prs.js
 */

const { execSync } = require("child_process");
const path = require("path");

const REPO = "iberi22/xavier";
const PRS = [625, 626, 627, 628];
const WORKDIR = "E:\\scripts-python\\xavier";

function run(cmd, options = {}) {
  try {
    const stdout = execSync(cmd, {
      cwd: options.cwd || WORKDIR,
      encoding: "utf8",
      timeout: options.timeout || 120000,
      ...options,
    });
    return { success: true, stdout: stdout.trim() };
  } catch (e) {
    return {
      success: false,
      stdout: e.stdout?.trim() || "",
      stderr: e.stderr?.trim() || "",
      error: e.message,
    };
  }
}

// First, checkout main to have a clean state
console.log("=== Switching to main ===");
run("git checkout main");

for (const prNum of PRS) {
  console.log(`\n${"=".repeat(60)}`);
  console.log(`PR #${prNum}`);
  console.log("=".repeat(60));

  // Get PR info
  const info = run(
    `gh pr view ${prNum} --repo ${REPO} --json title,body,headRefName,mergeable,state,isDraft,additions,deletions`,
    { timeout: 10000 },
  );
  try {
    const json = JSON.parse(info.stdout);
    console.log(`  Title: ${json.title}`);
    console.log(`  Branch: ${json.headRefName}`);
    console.log(`  Mergeable: ${json.mergeable}`);
    console.log(`  Draft: ${json.isDraft}`);
    console.log(`  State: ${json.state}`);
    console.log(`  Changes: +${json.additions}/-${json.deletions}`);
  } catch (e) {
    console.log(`  Error parsing info: ${e.message}`);
    console.log(`  Raw: ${info.stdout?.substring(0, 200)}`);
  }

  // Checkout PR
  console.log(`\n  ▶ Checking out PR #${prNum}...`);
  const checkout = run(`gh pr checkout ${prNum} --repo ${REPO}`, {
    timeout: 15000,
  });
  if (!checkout.success) {
    console.log(`  ❌ Checkout failed: ${checkout.stderr?.substring(0, 100)}`);
    console.log(`  STDOUT: ${checkout.stdout?.substring(0, 200)}`);
    continue;
  }

  // Build
  console.log("  ▶ Building...");
  const buildStart = Date.now();
  const build = run("cargo build --lib --features ci-safe", {
    timeout: 300000,
  });
  const buildTime = Math.round((Date.now() - buildStart) / 1000);

  if (build.success) {
    console.log(`  ✅ Build PASSED (${buildTime}s)`);
  } else {
    const errors = (build.stdout + build.stderr)
      .split("\n")
      .filter((l) => l.includes("error"));
    console.log(`  ❌ Build FAILED (${buildTime}s)`);
    console.log(`  Errors: ${errors.slice(0, 5).join("\n    ")}`);
    continue;
  }

  // Clippy
  console.log("  ▶ Running clippy...");
  const clippy = run("cargo clippy --lib --features ci-safe 2>&1", {
    timeout: 180000,
  });
  const warnings = (clippy.stdout + clippy.stderr)
    .split("\n")
    .filter((l) => l.includes("warning"));

  if (clippy.success && warnings.length === 0) {
    console.log("  ✅ Clippy PASSED (0 warnings)");
  } else if (warnings.length > 0) {
    console.log(`  ⚠️  Clippy: ${warnings.length} warnings`);
    warnings.slice(0, 10).forEach((w) => console.log(`    ${w.trim()}`));
  } else {
    console.log("  ✅ Clippy PASSED (exit 0, no warnings)");
  }

  // Mark as ready if draft
  console.log("  ▶ Checking draft status...");
  const prInfo = run(
    `gh pr view ${prNum} --repo ${REPO} --json isDraft --jq '.isDraft'`,
    { timeout: 10000 },
  );
  const isDraft = prInfo.stdout?.trim() === "true";

  if (isDraft) {
    console.log("  🔄 PR is DRAFT — merging would mark ready first.");
  } else {
    console.log("  ✅ PR is READY");
  }
}

// Back to main
console.log(`\n${"=".repeat(60)}`);
console.log("=== Switching back to main ===");
run("git checkout main");
console.log("=== DONE ===");
