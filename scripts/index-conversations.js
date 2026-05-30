const fs = require('fs');
const path = require('path');
const readline = require('readline');
const http = require('http');

const XAVIER_URL = 'http://localhost:8006';
const XAVIER_TOKEN = process.env.XAVIER_TOKEN;
const MAX_CHUNK_SIZE = 12000;

if (!XAVIER_TOKEN) {
  console.error("Error: XAVIER_TOKEN environment variable is not set.");
  console.error("A secure token is required for all operations.");
  process.exit(1);
}
const MAX_SESSIONS = 8;

let totalIndexed = 0;
let totalBytes = 0;

function sanitizeText(text) {
  if (!text) return '';
  // Remove invalid unicode and control chars
  return text
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g, '')
    .replace(/[\uD800-\uDFFF]/g, '')
    .replace(/[\uFFFE\uFFFF]/g, '');
}

function chunkText(text, maxSize) {
  const chunks = [];
  let current = '';
  const lines = text.split('\n');
  for (const line of lines) {
    if ((current + line).length > maxSize) {
      if (current) chunks.push(current);
      current = line + '\n';
    } else {
      current += line + '\n';
    }
  }
  if (current) chunks.push(current);
  return chunks;
}

function sendToXavier(path, content, metadata) {
  return new Promise((resolve) => {
    const cleanContent = sanitizeText(content);
    const body = JSON.stringify({ path, content: cleanContent, metadata });
    
    const req = http.request(
      `${XAVIER_URL}/memory/add`,
      {
        method: 'POST',
        headers: {
          'X-Xavier-Token': XAVIER_TOKEN,
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(body),
        },
      },
      (res) => {
        let data = '';
        res.on('data', (chunk) => { data += chunk; });
        res.on('end', () => {
          try {
            const json = JSON.parse(data);
            if (json.status === 'ok') {
              totalIndexed++;
              totalBytes += cleanContent.length;
              resolve(true);
            } else {
              console.warn(`  Xavier error: ${json.message || JSON.stringify(json)}`);
              resolve(false);
            }
          } catch (e) {
            console.warn(`  Parse error: ${data.substring(0, 200)}`);
            resolve(false);
          }
        });
      }
    );
    
    req.on('error', (err) => {
      console.warn(`  Request error: ${err.message}`);
      resolve(false);
    });
    req.write(body);
    req.end();
  });
}

async function processCodexSessions() {
  console.log('=== Indexing Codex conversations ===');
  const sessionsDir = path.join(process.env.USERPROFILE, '.codex', 'sessions');
  if (!fs.existsSync(sessionsDir)) {
    console.log('No Codex sessions found');
    return;
  }

  const files = [];
  function walk(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(fullPath);
      else if (entry.name.endsWith('.jsonl')) files.push(fullPath);
    }
  }
  walk(sessionsDir);
  
  files.sort((a, b) => fs.statSync(b).mtime - fs.statSync(a).mtime);
  const selected = files.slice(0, MAX_SESSIONS);

  for (const file of selected) {
    const stats = fs.statSync(file);
    console.log(`Processing Codex: ${path.basename(file)} (${Math.round(stats.size / 1024)} KB)`);
    
    const rl = readline.createInterface({ input: fs.createReadStream(file), crlfDelay: Infinity });
    const messages = [];
    for await (const line of rl) {
      try {
        const obj = JSON.parse(line);
        if (obj.type === 'event_msg' || obj.type === 'response_item') {
          const p = obj.payload;
          if (p && p.type === 'message' && (p.role === 'user' || p.role === 'assistant')) {
            let text = '';
            if (Array.isArray(p.content)) {
              for (const c of p.content) { if (c.text) text += c.text + ' '; }
            } else if (typeof p.content === 'string') {
              text = p.content;
            }
            if (text.length > 5) {
              messages.push(`[${p.role}]: ${text}`);
            }
          }
        }
      } catch (e) {}
    }

    if (messages.length > 0) {
      const sessionText = messages.join('\n\n');
      const chunks = chunkText(sessionText, MAX_CHUNK_SIZE);
      for (let i = 0; i < chunks.length; i++) {
        await sendToXavier(
          `cli-history/codex/${path.basename(file, '.jsonl')}/chunk-${i}`,
          chunks[i],
          {
            tool: 'codex-cli',
            session: path.basename(file),
            date: stats.mtime.toISOString().split('T')[0],
            chunk_index: i,
            total_chunks: chunks.length,
          }
        );
      }
      console.log(`  Indexed ${messages.length} messages in ${chunks.length} chunks`);
    }
  }
}

async function processOpenClawSessions() {
  console.log('\n=== Indexing OpenClaw conversations ===');
  const sessionsDir = path.join(process.env.USERPROFILE, '.openclaw', 'agents', 'main', 'sessions');
  if (!fs.existsSync(sessionsDir)) {
    console.log('No OpenClaw sessions found');
    return;
  }

  const files = fs.readdirSync(sessionsDir)
    .filter(f => f.endsWith('.jsonl') && !f.includes('.trajectory') && !f.includes('.checkpoint'))
    .map(f => ({ name: f, path: path.join(sessionsDir, f), mtime: fs.statSync(path.join(sessionsDir, f)).mtime }))
    .sort((a, b) => b.mtime - a.mtime)
    .slice(0, MAX_SESSIONS);

  for (const file of files) {
    const stats = fs.statSync(file.path);
    console.log(`Processing OpenClaw: ${file.name} (${Math.round(stats.size / 1024)} KB)`);
    
    const rl = readline.createInterface({ input: fs.createReadStream(file.path), crlfDelay: Infinity });
    const messages = [];
    for await (const line of rl) {
      try {
        const obj = JSON.parse(line);
        if (obj.type === 'prompt.submitted' && obj.data && obj.data.prompt) {
          messages.push(`[user]: ${obj.data.prompt}`);
        }
        if (obj.type === 'model.completed' && obj.data && obj.data.assistantTexts) {
          for (const txt of obj.data.assistantTexts) {
            if (txt && txt.length > 5) messages.push(`[assistant]: ${txt}`);
          }
        }
      } catch (e) {}
    }

    if (messages.length > 0) {
      const sessionText = messages.join('\n\n');
      const chunks = chunkText(sessionText, MAX_CHUNK_SIZE);
      for (let i = 0; i < chunks.length; i++) {
        await sendToXavier(
          `cli-history/openclaw/${path.basename(file.name, '.jsonl')}/chunk-${i}`,
          chunks[i],
          {
            tool: 'openclaw',
            session: file.name,
            date: stats.mtime.toISOString().split('T')[0],
            chunk_index: i,
            total_chunks: chunks.length,
          }
        );
      }
      console.log(`  Indexed ${messages.length} messages in ${chunks.length} chunks`);
    }
  }
}

async function processClaudeCode() {
  console.log('\n=== Indexing Claude Code operations ===');
  const projectsDir = path.join(process.env.USERPROFILE, '.claude', 'projects');
  if (!fs.existsSync(projectsDir)) {
    console.log('No Claude Code projects found');
    return;
  }

  const files = [];
  function walk(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(fullPath);
      else if (entry.name.endsWith('.jsonl')) files.push(fullPath);
    }
  }
  walk(projectsDir);
  
  files.sort((a, b) => fs.statSync(b).mtime - fs.statSync(a).mtime);
  const selected = files.slice(0, MAX_SESSIONS);

  for (const file of selected) {
    const stats = fs.statSync(file);
    const projectMatch = file.match(/projects\\([^\\]+)/);
    const project = projectMatch ? projectMatch[1] : 'unknown';
    
    const rl = readline.createInterface({ input: fs.createReadStream(file), crlfDelay: Infinity });
    const ops = [];
    for await (const line of rl) {
      try {
        const obj = JSON.parse(line);
        if (obj.content && obj.content.length > 20) {
          ops.push(obj.content);
        }
      } catch (e) {}
    }

    if (ops.length > 0) {
      const text = ops.join('\n---\n');
      const chunks = chunkText(text, MAX_CHUNK_SIZE);
      for (let i = 0; i < chunks.length; i++) {
        await sendToXavier(
          `cli-history/claude/${project}/${path.basename(file, '.jsonl')}/chunk-${i}`,
          chunks[i],
          {
            tool: 'claude-code',
            project: project,
            date: stats.mtime.toISOString().split('T')[0],
            operations: ops.length,
            chunk_index: i,
            total_chunks: chunks.length,
          }
        );
      }
      console.log(`  Indexed ${ops.length} operations from ${project}`);
    }
  }
}

async function main() {
  await processCodexSessions();
  await processOpenClawSessions();
  await processClaudeCode();
  
  console.log('\n=== INDEXING COMPLETE ===');
  console.log(`Total chunks indexed: ${totalIndexed}`);
  console.log(`Total bytes indexed: ${Math.round(totalBytes / 1024)} KB`);
}

main().catch(console.error);
