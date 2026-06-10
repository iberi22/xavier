// Index OpenClaw session .jsonl and trajectory files into Xavier
// Pure Node.js - runs standalone
const fs = require("fs");
const path = require("path");
const http = require("http");
const os = require("os");

const XAVIER_URL = "http://localhost:8006";
const TOKEN = "dev-token";
const MAX_FILES_PER_AGENT = 5; // quick test batch
const MAX_LINES_PER_FILE = 20; // recent messages per session
const DELAY_MS = 5;

function xavierPost(endpoint, payload) {
  return new Promise((resolve, reject) => {
    const url = new URL(endpoint, XAVIER_URL);
    const body = JSON.stringify(payload);
    const opts = {
      hostname: url.hostname,
      port: url.port,
      path: url.pathname,
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Xavier-Token": TOKEN,
        "Content-Length": Buffer.byteLength(body),
      },
      timeout: 30000,
    };
    const req = http.request(opts, (res) => {
      let data = "";
      res.on("data", (chunk) => (data += chunk));
      res.on("end", () => {
        try {
          resolve(JSON.parse(data));
        } catch {
          resolve({ raw: data, status: res.statusCode });
        }
      });
    });
    req.on("error", reject);
    req.on("timeout", () => {
      req.destroy();
      reject(new Error("timeout"));
    });
    req.write(body);
    req.end();
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function extractMessagesFromLines(lines) {
  const msgs = [];
  for (const line of lines) {
    try {
      const entry = JSON.parse(line);
      // Check various formats
      const msg = entry.message || entry;
      const role = msg.role;
      if (role !== "user" && role !== "assistant") continue;

      let text = "";
      if (typeof msg.content === "string") text = msg.content;
      else if (Array.isArray(msg.content)) {
        text = msg.content
          .filter((c) => c.type === "text")
          .map((c) => c.text)
          .join(" ");
      }
      if (text && text.length > 5 && !text.startsWith("/")) {
        msgs.push({ role, text: text.substring(0, 800) });
      }
    } catch {
      /* skip malformed */
    }
  }
  return msgs;
}

function extractTrajectoryInfo(line) {
  try {
    const entry = JSON.parse(line);
    if (entry.type === "conversation_start" || entry.type === "session") {
      return {
        ts: entry.timestamp || entry.time,
        id: entry.id || entry.sessionId,
      };
    }
  } catch {}
  return null;
}

function buildPayload(agentName, content, sessionId, tags, dateStr) {
  const slug =
    (sessionId || "session").replace(/[^a-zA-Z0-9_-]/g, "").substring(0, 20) ||
    "session";
  return {
    path: `sessions/openclaw/${agentName}/${dateStr}/${slug}`,
    content: content.substring(0, 3000),
    metadata: {
      agent_id: agentName,
      source: "openclaw",
      entry_type: "conversation",
      tags: tags.join(","),
      session_id: sessionId || "",
      indexed_at: new Date().toISOString(),
    },
  };
}

function inferTags(content, baseTags) {
  const tags = new Set(baseTags || ["openclaw"]);
  const lower = content.toLowerCase();
  if (/data(\s+)?commons/.test(lower)) tags.add("data-commons");
  if (/wallet|token|blockchain|crypto|btc|bitcoin/.test(lower))
    tags.add("wallet");
  if (/governance|vote|dao|proposal|council|bicameral/.test(lower))
    tags.add("governance");
  if (/rust|compil|error|bug|fix|borrow/.test(lower)) tags.add("code");
  if (/memory|rag|search|embed|index/.test(lower)) tags.add("memory");
  if (/docker|deploy|containers|devops/.test(lower)) tags.add("devops");
  if (/api|endpoint|rest|http|server/.test(lower)) tags.add("api");
  if (/config|configure|setting|hook|bridge/.test(lower)) tags.add("config");
  if (/pr|merge|commit|pull|git/.test(lower)) tags.add("git");
  if (/reputation|proof|contribution|stak/.test(lower)) tags.add("reputation");
  if (/funnel|tier|level|data quality/.test(lower)) tags.add("funnel");
  if (/receta|food|tiktok|viral|contenido/.test(lower)) tags.add("content");
  if (/mounjaro|ozempic|peso|dieta/.test(lower)) tags.add("health");
  if (/trading|market|price|exchange/.test(lower)) tags.add("trading");
  if (/postgres|sql|query|database/.test(lower)) tags.add("database");
  if (/examen|exam|worldexams|question/.test(lower)) tags.add("education");
  return Array.from(tags);
}

async function main() {
  console.log("=== Xavier OpenClaw Session Indexer v2 (JSONL) ===");
  console.log("Target:", XAVIER_URL);
  console.log("");

  // Check health
  try {
    await xavierPost("/health", {});
    console.log("✅ Xavier OK");
  } catch (e) {
    console.log("❌ Xavier not responding:", e.message);
    process.exit(1);
  }

  const AGENTS = [
    "lasantacruz",
    "xavier",
    "main",
    "worldexams",
    "ventas",
    "pgheart",
    "inventario",
    "coder",
    "ghost",
    "trading",
  ];
  const homeDir = os.homedir();
  const stats = {
    totalFiles: 0,
    totalIndexed: 0,
    totalFailed: 0,
    totalSkipped: 0,
    agents: {},
  };

  for (const agent of AGENTS) {
    const sessDir = path.join(
      homeDir,
      ".openclaw",
      "agents",
      agent,
      "sessions",
    );
    if (!fs.existsSync(sessDir)) {
      console.log(`⚠️  No sessions dir for ${agent}`);
      continue;
    }

    // Collect .jsonl files (not trajectories, not reset files)
    let files = fs
      .readdirSync(sessDir)
      .filter(
        (f) =>
          f.endsWith(".jsonl") &&
          !f.includes(".trajectory") &&
          !f.includes(".reset"),
      )
      .sort()
      .reverse(); // most recent first

    if (files.length === 0) {
      // Try trajectory files as fallback
      files = fs
        .readdirSync(sessDir)
        .filter((f) => f.endsWith(".trajectory.jsonl") && !f.includes(".reset"))
        .sort()
        .reverse();
    }

    files = files.slice(0, MAX_FILES_PER_AGENT);
    console.log(`📄 ${agent}: ${files.length} files to process`);

    let agentIndexed = 0,
      agentFailed = 0,
      agentSkipped = 0;

    for (let fi = 0; fi < files.length; fi++) {
      const filePath = path.join(sessDir, files[fi]);
      const lines = fs.readFileSync(filePath, "utf-8").trim().split("\n");

      // Get session identifier
      const headerInfo = extractTrajectoryInfo(lines[0]);
      const sessionId =
        headerInfo?.id ||
        files[fi].replace(".jsonl", "").replace(".trajectory", "");
      const dateStr = headerInfo?.ts
        ? headerInfo.ts.match(/\d{4}-\d{2}-\d{2}/)?.[0] || "unknown"
        : new Date().toISOString().slice(0, 10);

      // Extract messages
      const msgs = extractMessagesFromLines(lines);
      if (msgs.length === 0) {
        agentSkipped++;
        continue;
      }

      // Build content summary
      const content = msgs.map((m) => m.role + ": " + m.text).join("\n");
      if (content.length < 50) {
        agentSkipped++;
        continue;
      }

      const tags = inferTags(content, ["openclaw", "session", agent]);
      const payload = buildPayload(agent, content, sessionId, tags, dateStr);

      try {
        const result = await xavierPost("/memory/add", payload);
        if (result.status === "ok" || result.id) {
          agentIndexed++;
          stats.totalIndexed++;
        } else {
          agentFailed++;
          stats.totalFailed++;
          console.log(
            `    FAIL[${fi}]:`,
            JSON.stringify(result).substring(0, 200),
          );
        }
      } catch (e) {
        agentFailed++;
        stats.totalFailed++;
        console.log(`    ERROR[${fi}]:`, e.message);
      }

      if ((fi + 1) % 10 === 0) {
        console.log(
          `  ⏳ ${agent}: ${fi + 1}/${files.length} (${agentIndexed} OK, ${agentFailed} fail, ${agentSkipped} skip)`,
        );
      }
      await sleep(DELAY_MS);
    }

    console.log(
      `  ✅ ${agent}: ${agentIndexed} OK, ${agentFailed} fail, ${agentSkipped} skip`,
    );
    stats.agents[agent] = {
      files: files.length,
      indexed: agentIndexed,
      failed: agentFailed,
      skipped: agentSkipped,
    };
    stats.totalFiles += files.length;
  }

  console.log("\n==============================");
  console.log("INDEXING COMPLETE");
  console.log("==============================");
  console.log("Files processed:", stats.totalFiles);
  console.log("Total indexed:", stats.totalIndexed);
  console.log("Total failed:", stats.totalFailed);
  console.log("Total skipped:", stats.totalSkipped);
  console.log("\nBy agent:");
  for (const [a, s] of Object.entries(stats.agents)) {
    console.log(
      `  ${a}: ${s.files} files, ${s.indexed} OK, ${s.failed} fail, ${s.skipped} skip`,
    );
  }
}

main().catch(console.error);
