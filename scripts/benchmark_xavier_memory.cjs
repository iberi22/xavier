// Benchmark Xavier memory search performance
// Pure Node.js - runs standalone
const http = require("http");

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
  console.log("=== Xavier Memory Benchmark ===");
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

  // --- Test Queries ---
  const testQueries = [
    {
      query: "gobernanza token votacion consejo xavier core",
      tags: ["governance"],
    },
    {
      query: "wallet post-cuantica ML-KEM ML-DSA firma digital",
      tags: ["wallet"],
    },
    {
      query: "data commons funnel reputation recompensas",
      tags: ["data-commons"],
    },
    { query: "memoria RAG busqueda indexacion openclaw", tags: ["memory"] },
    {
      query: "xavier data commons architecture features roadmap",
      tags: ["data-commons", "architecture"],
    },
    { query: "configuracion bridge sincronizacion sesiones", tags: ["config"] },
    { query: "compilacion rust error borrow checker fix", tags: ["code"] },
    {
      query: "token supply limit distribucion tokenomics",
      tags: ["wallet", "tokenomics"],
    },
    { query: "benchmark performance latency throughput", tags: ["testing"] },
    {
      query: "reputation proof contribution staked slashing",
      tags: ["reputation"],
    },
    { query: "content creator tiktok receta video viral", tags: ["content"] },
    { query: "openclaw session conversation history", tags: ["session"] },
  ];

  const results = [];
  console.log("Running", testQueries.length, "benchmark queries...\n");

  for (const tq of testQueries) {
    const payload = { query: tq.query, limit: 5 };
    const start = Date.now();

    try {
      const resp = await xavierPost("/memory/search", payload);
      const elapsed = Date.now() - start;

      let resultCount = 0;
      let hasRelevant = false;

      if (resp.results && Array.isArray(resp.results)) {
        resultCount = resp.results.length;
        const allContent = resp.results
          .map((r) => r.content || "")
          .join(" ")
          .toLowerCase();
        for (const tag of tq.tags) {
          if (allContent.includes(tag.toLowerCase())) {
            hasRelevant = true;
            break;
          }
        }
      }

      results.push({
        query: tq.query.substring(0, 60),
        latencyMs: elapsed,
        numResults: resultCount,
        relevant: hasRelevant,
        status: "OK",
      });

      const icon = hasRelevant ? "[target]" : "[warn]";
      console.log(
        `  [OK] "${tq.query.substring(0, 40)}..." -> ${resultCount} results in ${elapsed}ms ${icon}`,
      );
    } catch (e) {
      const elapsed = Date.now() - start;
      results.push({
        query: tq.query.substring(0, 60),
        latencyMs: elapsed,
        numResults: 0,
        relevant: false,
        status: "ERROR: " + e.message,
      });
      console.log(
        `  [FAIL] "${tq.query.substring(0, 40)}..." ERROR: ${e.message}`,
      );
    }

    await sleep(50);
  }

  // --- Summary Stats ---
  console.log("\n" + "=".repeat(40));
  console.log("BENCHMARK RESULTS");
  console.log("=".repeat(40));

  const okResults = results.filter((r) => r.status === "OK");
  const failResults = results.filter((r) => r.status !== "OK");

  const latencies = okResults.map((r) => r.latencyMs);
  const avgLatency = latencies.length
    ? latencies.reduce((a, b) => a + b, 0) / latencies.length
    : 0;
  const minLatency = latencies.length ? Math.min(...latencies) : 0;
  const maxLatency = latencies.length ? Math.max(...latencies) : 0;
  const totalResults = okResults.reduce((sum, r) => sum + r.numResults, 0);
  const relevantCount = okResults.filter((r) => r.relevant).length;
  const recallRate = okResults.length ? relevantCount / okResults.length : 0;

  let latencyGrade, recallGrade;

  if (avgLatency < 100) latencyGrade = "A+";
  else if (avgLatency < 300) latencyGrade = "A";
  else if (avgLatency < 500) latencyGrade = "B";
  else if (avgLatency < 1000) latencyGrade = "C";
  else latencyGrade = "D";

  if (recallRate >= 0.8) recallGrade = "A";
  else if (recallRate >= 0.5) recallGrade = "B";
  else if (recallRate >= 0.3) recallGrade = "C";
  else recallGrade = "D";

  console.log("Total queries:", testQueries.length);
  console.log("Successful:", okResults.length);
  console.log("Failed:", failResults.length);
  console.log("Avg latency:", avgLatency.toFixed(1) + "ms");
  console.log("Min latency:", minLatency.toFixed(1) + "ms");
  console.log("Max latency:", maxLatency.toFixed(1) + "ms");
  console.log("Total results returned:", totalResults);
  console.log(
    "Relevant results:",
    relevantCount,
    "/",
    okResults.length,
    "(" + (recallRate * 100).toFixed(1) + "%)",
  );
  console.log(
    "Avg results per query:",
    okResults.length ? (totalResults / okResults.length).toFixed(1) : 0,
  );
  console.log("");
  console.log(
    "Latency Grade:",
    latencyGrade,
    "(" + avgLatency.toFixed(1) + "ms avg)",
  );
  console.log(
    "Recall Grade:",
    recallGrade,
    "(" + (recallRate * 100).toFixed(1) + "%)",
  );

  // --- Per-query breakdown ---
  console.log("\n--- Per-Query Breakdown ---");
  for (const r of results) {
    const marker = r.status === "OK" ? "[OK]" : "[FAIL]";
    const relev = r.relevant ? "[target]" : "[warn]";
    console.log(
      marker,
      "[" + r.latencyMs + "ms][" + r.numResults + " results]" + relev,
      r.query,
    );
    if (r.status !== "OK") {
      console.log("       Error:", r.status);
    }
  }

  // Save results
  const fs = require("fs");
  const reportPath = "E:\\scripts-python\\xavier\\benchmark_results.json";
  const reportObj = {
    timestamp: new Date().toISOString(),
    queries: results,
    summary: {
      totalQueries: testQueries.length,
      successful: okResults.length,
      failed: failResults.length,
      avgLatencyMs: parseFloat(avgLatency.toFixed(1)),
      minLatencyMs: parseFloat(minLatency.toFixed(1)),
      maxLatencyMs: parseFloat(maxLatency.toFixed(1)),
      totalResults,
      relevantResults: relevantCount,
      recallRate: parseFloat(recallRate.toFixed(3)),
      latencyGrade,
      recallGrade,
    },
  };
  fs.writeFileSync(reportPath, JSON.stringify(reportObj, null, 2), "utf-8");
  console.log("\nResults saved to:", reportPath);
}

main().catch(console.error);
