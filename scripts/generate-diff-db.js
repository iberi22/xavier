const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const OUTPUT_DIR = path.join(__dirname, '..', 'public', 'devlog');
const OUTPUT_FILE = path.join(OUTPUT_DIR, 'commits-db.json');

function runGit(cmd) {
  try {
    return execSync(cmd, { encoding: 'utf8' }).trim();
  } catch (e) {
    return '';
  }
}

function generateDatabase() {
  console.log('📊 Generando Base de Datos de Diffs Reales para la Maloca (rama MAIN)...');

  // Verify we have a main branch
  let mainBranch = 'main';
  const branches = runGit('git branch --list main');
  if (!branches) {
    // If local main doesn't exist, fallback to current active branch
    mainBranch = runGit('git rev-parse --abbrev-ref HEAD');
  }

  // Get all commits on main
  const logOutput = runGit(`git log ${mainBranch} --pretty=format:"%H|%an|%ae|%ad|%s" --date=short`);
  if (!logOutput) {
    console.error('❌ No se pudieron recuperar los commits de Git.');
    return;
  }

  const commits = [];
  const lines = logOutput.split('\n');

  for (const line of lines) {
    if (!line) continue;
    const [hash, authorName, authorEmail, date, message] = line.split('|');

    // Get files changed in this commit
    const filesOutput = runGit(`git diff-tree --no-commit-id --name-only -r ${hash}`);
    const files = filesOutput ? filesOutput.split('\n').filter(Boolean) : [];

    const diffs = {};
    const fileStats = {};

    for (const file of files) {
      // Get the patch for this file
      const patch = runGit(`git show ${hash} -- ${file}`);
      const patchLines = patch.split('\n');

      const diffLines = [];
      let inDiff = false;
      let lineIndex = 0;

      for (const pLine of patchLines) {
        if (pLine.startsWith('diff --git')) {
          inDiff = true;
          continue;
        }
        if (inDiff) {
          if (pLine.startsWith('---') || pLine.startsWith('+++') || pLine.startsWith('index')) {
            continue;
          }
          if (pLine.startsWith('@@')) {
            diffLines.push({ type: 'info', text: pLine });
          } else if (pLine.startsWith('+')) {
            diffLines.push({ type: 'addition', text: pLine });
          } else if (pLine.startsWith('-')) {
            diffLines.push({ type: 'deletion', text: pLine });
          } else {
            diffLines.push({ type: 'normal', text: pLine });
          }
        }
      }

      diffs[file] = diffLines.length > 0 ? diffLines : [{ type: 'normal', text: '// No se encontraron cambios' }];
    }

    // Determine RAG module connections based on paths
    const ragNodes = files.map(file => {
      const parts = file.split('/');
      const mod = parts.length > 1 ? parts[0] : 'core';
      return {
        path: `chronicle/auto-docs/${mod}`,
        type: 'Módulo Relacionado'
      };
    }).filter((v, i, a) => a.findIndex(t => t.path === v.path) === i).slice(0, 3);

    commits.push({
      hash,
      message,
      date,
      author: `${authorName} <${authorEmail}>`,
      explanation: `<p><strong>Decisión de Ingeniería:</strong> Commit real de Xavier en la rama principal. Modifica ${files.length} archivos.</p>`,
      files,
      diffs,
      rag_nodes: ragNodes
    });
  }

  // Ensure output directory exists
  if (!fs.existsSync(OUTPUT_DIR)) {
    fs.mkdirSync(OUTPUT_DIR, { recursive: true });
  }

  fs.writeFileSync(OUTPUT_FILE, JSON.stringify(commits, null, 2));
  console.log(`✅ Base de datos escrita con éxito en ${OUTPUT_FILE}`);
}

generateDatabase();
