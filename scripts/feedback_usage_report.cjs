// Usage Feedback Report - Analyze Xavier memory coverage
// Pure Node.js
const http = require("http");
const fs = require("fs");

const XAVIER_URL = "http://localhost:8006";
const TOKEN = "dev-token";

function xavierGet(path) {
  return new Promise((resolve, reject) => {
    const opts = {
      hostname: "localhost",
      port: 8006,
      path,
      method: "GET",
      headers: { "X-Xavier-Token": TOKEN },
      timeout: 10000,
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
    req.end();
  });
}

function xavierPost(endpoint, payload) {
  return new Promise((resolve, reject) => {
    const body = JSON.stringify(payload);
    const opts = {
      hostname: "localhost",
      port: 8006,
      path: endpoint,
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

async function main() {
  console.log("=== Usage Feedback Report ===");
  console.log("Target:", XAVIER_URL);
  console.log("");

  // Health check
  try {
    await xavierGet("/health");
    console.log("[OK] Xavier is alive");
  } catch (e) {
    console.log("[FAIL] Xavier not responding:", e.message);
    process.exit(1);
  }

  // --- 1. Agent Session Coverage ---
  console.log("\n--- 1. Agent Session Coverage ---");

  const agentQueries = [
    { query: "lasantacruz content tiktok video", label: "LaSantacruz" },
    { query: "xavier data commons governance wallet", label: "Xavier Core" },
    { query: "openclaw session conversation", label: "OpenClaw Sessions" },
    { query: "worldexams exam practice questions", label: "WorldExams" },
    { query: "ventas inventory sales products", label: "Ventas" },
    { query: "pgheart postgres database query", label: "PGHeart" },
  ];

  for (const aq of agentQueries) {
    try {
      const resp = await xavierPost("/memory/search", {
        query: aq.query,
        limit: 3,
      });
      const count = resp.results ? resp.results.length : 0;
      console.log(
        "  " +
          aq.label +
          ": " +
          count +
          " results " +
          (count > 0 ? "[ok]" : "[warn]"),
      );
    } catch (e) {
      console.log("  " + aq.label + ": ERROR - " + e.message);
    }
    await sleep(30);
  }

  // --- 2. Topic Density ---
  console.log("\n--- 2. Topic Density ---");

  const topicQueries = [
    { query: "data commons architecture design", label: "Data Commons" },
    {
      query: "wallet ML-KEM post-quantum cryptography",
      label: "Post-Quantum Wallet",
    },
    {
      query: "governance bicameral vote council proposal",
      label: "Governance",
    },
    { query: "reputation proof contribution staking", label: "Reputation" },
    { query: "memory RAG search indexing embedding", label: "Memory/RAG" },
    { query: "Rust compilation error fix borrow checker", label: "Rust Code" },
    { query: "API endpoint HTTP REST server", label: "API" },
    { query: "config bridge sync session hook", label: "Bridge Config" },
    {
      query: "performance latency throughput benchmark",
      label: "Benchmarking",
    },
    {
      query: "token supply economic distribution incentive",
      label: "Tokenomics",
    },
    { query: "content creator social media viral", label: "Content Strategy" },
    { query: "session history conversation", label: "Session History" },
  ];

  const topicResults = [];
  for (const tq of topicQueries) {
    try {
      const resp = await xavierPost("/memory/search", {
        query: tq.query,
        limit: 5,
      });
      const count = resp.results ? resp.results.length : 0;
      topicResults.push({ label: tq.label, count });
      console.log("  " + tq.label + ": " + count + " entries");
    } catch (e) {
      topicResults.push({ label: tq.label, count: 0 });
      console.log("  " + tq.label + ": ERROR - " + e.message);
    }
    await sleep(30);
  }

  // --- 3. Cross-Data Commons Knowledge ---
  console.log("\n--- 3. Cross-Topic Queries Performance ---");

  const crossQueries = [
    "gobernanza DAO wallet post-cuantica xavier",
    "data commons funnel reputation staking tokenomics",
    "rules bicameral council vote weight consensus",
    "compilacion Rust gobernanza governance memory",
    "openclaw session index bridge sync config",
  ];

  for (const cq of crossQueries) {
    try {
      const start = Date.now();
      const resp = await xavierPost("/memory/search", { query: cq, limit: 3 });
      const elapsed = Date.now() - start;
      const count = resp.results ? resp.results.length : 0;
      console.log(
        "  [" +
          elapsed +
          'ms] "' +
          cq.substring(0, 40) +
          '..." -> ' +
          count +
          " results",
      );
    } catch (e) {
      console.log("  ERROR: " + cq);
    }
    await sleep(20);
  }

  // --- 4. Summary ---
  console.log("\n" + "=".repeat(40));
  console.log("USAGE FEEDBACK SUMMARY");
  console.log("=".repeat(40));

  const topicsCovered = topicResults.filter((t) => t.count > 0).length;
  const totalTopics = topicResults.length;
  const coverageRate = ((topicsCovered / totalTopics) * 100).toFixed(1);

  console.log("Topics with indexed data:", topicsCovered, "/", totalTopics);
  console.log("Coverage rate:", coverageRate + "%");

  console.log("\nMost dense topics:");
  topicResults
    .sort((a, b) => b.count - a.count)
    .slice(0, 5)
    .forEach((t) => {
      console.log("  " + t.label + ": " + t.count + " results");
    });

  console.log("\nLeast dense topics (gaps):");
  topicResults
    .filter((t) => t.count === 0)
    .forEach((t) => {
      console.log("  [gap] " + t.label);
    });

  // Save report
  const reportPath = "E:\\scripts-python\\xavier\\feedback_usage_report.json";
  const report = {
    timestamp: new Date().toISOString(),
    topics: topicResults,
    summary: {
      topicsCovered,
      totalTopics,
      coverageRate: parseFloat(coverageRate),
    },
  };
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2), "utf-8");
  console.log("\nReport saved to:", reportPath);
}

main().catch(console.error);
