#!/usr/bin/env node

/**
 * SWAL Husky Guard - Strict Multi-Platform Pre-Push Matrix & Leak Verification
 * Lightweight pre-push / pre-commit quality & security gate using standard Node.js APIs.
 */

import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';

/**
 * Validates a file path for compatibility with Windows / MSVC filesystems.
 * Checks for:
 * 1. Reserved filenames (CON, PRN, AUX, NUL, COM1-9, LPT1-9, CONIN$, CONOUT$, CLOCK$).
 * 2. Illegal characters (< > : " | ? * or control characters 0x00-0x1F).
 * 3. Trailing dots or trailing spaces in path segments.
 * 4. Non-portable characters.
 *
 * @param {string} filePath
 * @returns {string[]} List of validation error messages.
 */
export function validatePathForWindows(filePath) {
  const errors = [];
  if (!filePath || typeof filePath !== 'string') return errors;

  // Normalize path separators
  const normalized = filePath.replace(/\\/g, '/');
  const segments = normalized.split('/');

  const reservedRegex = /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]|CONIN\$|CONOUT\$|CLOCK\$)$/i;
  const illegalCharsRegex = /[<>:"|?*\x00-\x1F]/;

  for (const segment of segments) {
    if (!segment || segment === '.' || segment === '..') continue;

    // Check illegal chars in segment
    if (illegalCharsRegex.test(segment)) {
      errors.push(`Path segment '${segment}' in '${filePath}' contains illegal Windows characters (< > : " | ? * or control chars).`);
    }

    // Check trailing space or dot
    if (/[. ]$/.test(segment)) {
      errors.push(`Path segment '${segment}' in '${filePath}' ends with an illegal trailing space or dot.`);
    }

    // Check reserved names (stem before first dot, or exact match)
    const stem = segment.split('.')[0];
    if (reservedRegex.test(stem)) {
      errors.push(`Path segment '${segment}' in '${filePath}' uses reserved Windows filename '${stem}'.`);
    }
  }

  return errors;
}

/**
 * Audits staged or pushed files for secret leaks and excessive file sizes.
 *
 * @param {string[]} files
 * @returns {string[]} List of audit error messages.
 */
export function auditFiles(files) {
  const errors = [];
  const MAX_FILE_SIZE_BYTES = 25 * 1024 * 1024; // 25 MB max file size gate

  const SECRET_PATTERNS = [
    { name: 'Private Key Header', regex: /-----BEGIN (RSA|EC|OPENSSH|DSA|PGP)? PRIVATE KEY-----/i },
    { name: 'AWS Access Key ID', regex: /\b(AKIA|ASIA)[0-9A-Z]{16}\b/ },
    { name: 'Generic Hardcoded Token', regex: /\b(api[_-]?key|secret[_-]?key|access[_-]?token)\s*=\s*["'][A-Za-z0-9_\-]{20,}["']/i }
  ];

  // List of test files or mock sample files that deliberately contain synthetic test secrets for anonymizer tests
  const ALLOWED_TEST_SECRET_FILES = new Set([
    'tests/telemetry_anonymizer_test.rs',
    'tests/telemetry_anonymizer_edge_test.rs'
  ]);

  for (const file of files) {
    if (!fs.existsSync(file)) continue;

    try {
      const stats = fs.statSync(file);
      if (stats.isDirectory()) continue;

      if (stats.size > MAX_FILE_SIZE_BYTES) {
        errors.push(`File '${file}' exceeds maximum allowed size of 25MB (${(stats.size / 1024 / 1024).toFixed(2)}MB).`);
      }

      // Check text content for obvious secret leaks (skip binary files and allowed synthetic test fixture files)
      const normalizedPath = file.replace(/\\/g, '/');
      if (stats.size < 5 * 1024 * 1024 && !ALLOWED_TEST_SECRET_FILES.has(normalizedPath)) {
        const content = fs.readFileSync(file, 'utf8');
        for (const pattern of SECRET_PATTERNS) {
          if (pattern.regex.test(content)) {
            // Avoid false positive on swal-husky-guard.mjs itself
            if (path.basename(file) === 'swal-husky-guard.mjs') continue;
            errors.push(`File '${file}' contains potential secret leak: ${pattern.name}.`);
          }
        }
      }
    } catch {
      // Ignore unreadable files during audit
    }
  }

  return errors;
}

/**
 * Gets list of files to check from git diff or tracked files.
 * @returns {string[]}
 */
export function getGitFiles() {
  try {
    const output = execSync('git ls-files', { encoding: 'utf8', stdio: ['pipe', 'pipe', 'ignore'] });
    return output.split('\n').map(f => f.trim()).filter(Boolean);
  } catch {
    return [];
  }
}

/**
 * Unit test suite runner for validation functions.
 */
export function runTests() {
  console.log('🧪 Running SWAL Husky Guard Unit Tests...');
  let pass = true;

  const testCases = [
    { path: 'src/valid_file.rs', expectError: false },
    { path: 'panel-ui/src/components/Button.tsx', expectError: false },
    { path: 'docs/guide.md', expectError: false },
    { path: 'src/CON.rs', expectError: true },
    { path: 'src/aux/file.ts', expectError: true },
    { path: 'src/file:name.txt', expectError: true },
    { path: 'src/trailing_dot.', expectError: true },
    { path: 'src/trailing_space ', expectError: true },
    { path: 'src/nul.txt', expectError: true },
    { path: 'src/COM1.json', expectError: true },
    { path: 'src/<illegal>.js', expectError: true }
  ];

  for (const tc of testCases) {
    const errs = validatePathForWindows(tc.path);
    const hasError = errs.length > 0;
    if (hasError !== tc.expectError) {
      console.error(`❌ Test failed for path '${tc.path}': expected error=${tc.expectError}, got errors:`, errs);
      pass = false;
    }
  }

  if (pass) {
    console.log('✅ All unit tests passed successfully!');
    return 0;
  } else {
    console.error('❌ Unit tests failed!');
    return 1;
  }
}

// Main execution CLI entrypoint
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.dirname, 'swal-husky-guard.mjs')) {
  if (process.argv.includes('--test')) {
    process.exit(runTests());
  }

  console.log('🛡️  [SWAL Husky Guard] Executing cross-platform & leak audit...');

  const files = getGitFiles();
  let hasErrors = false;

  // 1. Cross-platform path validation
  for (const file of files) {
    const pathErrors = validatePathForWindows(file);
    if (pathErrors.length > 0) {
      hasErrors = true;
      for (const err of pathErrors) {
        console.error(`❌ [Windows Filename Gate] ${err}`);
      }
    }
  }

  // 2. Secret & file size audit
  const auditErrors = auditFiles(files);
  if (auditErrors.length > 0) {
    hasErrors = true;
    for (const err of auditErrors) {
      console.error(`❌ [Security & Leak Gate] ${err}`);
    }
  }

  if (hasErrors) {
    console.error('❌ [SWAL Husky Guard] Validation failed! Aborting operation.');
    process.exit(1);
  }

  console.log(`✅ [SWAL Husky Guard] Passed! ${files.length} tracked files verified across platforms.`);
  process.exit(0);
}
