import fs from "node:fs";
import path from "node:path";
import { isValidCron } from "cron-validator";
import yaml from "js-yaml";

const WORKFLOWS_DIR = ".github/workflows";

function getAllFiles(dirPath, arrayOfFiles = []) {
  const files = fs.readdirSync(dirPath);

  files.forEach((file) => {
    if (fs.statSync(path.join(dirPath, file)).isDirectory()) {
      arrayOfFiles = getAllFiles(path.join(dirPath, file), arrayOfFiles);
    } else {
      arrayOfFiles.push(path.join(dirPath, file));
    }
  });

  return arrayOfFiles;
}

function validateWorkflows() {
  let hasError = false;
  const files = fs
    .readdirSync(WORKFLOWS_DIR)
    .filter((file) => file.endsWith(".yml") || file.endsWith(".yaml"));

  console.log(`Checking workflows in ${WORKFLOWS_DIR}...`);

  files.forEach((file) => {
    const filePath = path.join(WORKFLOWS_DIR, file);
    const content = fs.readFileSync(filePath, "utf8");

    try {
      const data = yaml.load(content);
      if (data && data.on && data.on.schedule) {
        data.on.schedule.forEach((schedule, index) => {
          if (schedule.cron) {
            const cron = schedule.cron;
            if (isValidCron(cron)) {
              console.log(
                `✅ ${filePath} [schedule ${index}]: "${cron}" is valid.`,
              );
            } else {
              console.error(
                `❌ ${filePath} [schedule ${index}]: "${cron}" is INVALID.`,
              );
              hasError = true;
            }
          }
        });
      }
    } catch (e) {
      console.error(`⚠️ Error parsing ${filePath}: ${e.message}`);
    }
  });

  return !hasError;
}

// Also check for other cron expressions in the repo (e.g. docs, configs)
// This is a bit more complex as we don't have a fixed schema.
// We can use regex to find potential cron expressions.
function validateOtherFiles() {
  let hasError = false;
  // We'll skip some directories to avoid noise
  const excludeDirs = [
    ".git",
    "node_modules",
    "target",
    "dist",
    ".agents",
    ".kiro",
    ".openclaw",
    ".claude",
  ];
  const extensions = [".md", ".json", ".yml", ".yaml", ".rs"];

  console.log("\nChecking other files for potential cron expressions...");

  // Simple regex for cron: 5 or 6 parts
  const cronRegex = /['"]((?:[0-9*,\-/]+ ){4,5}[0-9*,\-/]+)['"]/g;

  function processDir(dir) {
    const files = fs.readdirSync(dir);
    files.forEach((file) => {
      const fullPath = path.join(dir, file);
      if (fs.statSync(fullPath).isDirectory()) {
        if (!excludeDirs.includes(file)) {
          processDir(fullPath);
        }
      } else if (extensions.includes(path.extname(file))) {
        // Skip the validation script itself and workflows (already checked)
        if (
          fullPath === "scripts/validate-cron.js" ||
          fullPath.startsWith(WORKFLOWS_DIR)
        )
          return;

        const content = fs.readFileSync(fullPath, "utf8");
        let match;
        while ((match = cronRegex.exec(content)) !== null) {
          const cron = match[1];
          // Basic check to avoid false positives with version numbers or similar
          if (cron.split(" ").length >= 5) {
            if (isValidCron(cron, { seconds: true, alias: true })) {
              // We don't fail CI for docs usually, but we log it.
              // Actually criteria says "valida todas las expresiones cron en el repo"
              // and "Falla con error claro si hay expresión inválida".
              // So we should fail.
              console.log(`✅ ${fullPath}: potential cron "${cron}" is valid.`);
            } else {
              // Check if it's likely a false positive (e.g. "1.2.3.4.5" or similar)
              // cron-validator is quite strict.
              console.error(
                `❌ ${fullPath}: potential cron "${cron}" is INVALID.`,
              );
              hasError = true;
            }
          }
        }
      }
    });
  }

  processDir(".");
  return !hasError;
}

const workflowsValid = validateWorkflows();
const otherFilesValid = validateOtherFiles();

if (!workflowsValid || !otherFilesValid) {
  process.exit(1);
} else {
  console.log("\nAll cron expressions are valid!");
}
