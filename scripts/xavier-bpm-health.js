#!/usr/bin/env node
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');

const XAVIER_URL = process.env.XAVIER_URL || 'http://localhost:8006';
const TOKEN = process.env.XAVIER_TOKEN || (['true', '1'].includes(process.env.XAVIER_DEV_MODE) ? 'dev-token' : null);
const TIMEOUT = 5000;

if (!TOKEN) {
  console.error("Error: XAVIER_TOKEN environment variable is not set.");
  console.error("For development, set XAVIER_DEV_MODE=true to use the default dev-token.");
  process.exit(1);
}

function log(msg) {
  const timestamp = new Date().toISOString();
  console.log(`[${timestamp}] ${msg}`);
}

async function checkHealth() {
  return new Promise((resolve) => {
    const req = http.get(`${XAVIER_URL}/health`, {
      headers: { 'Authorization': '***' + TOKEN },
      timeout: TIMEOUT
    }, (res) => {
      let body = '';
      res.on('data', c => body += c);
      res.on('end', () => {
        try {
          const data = JSON.parse(body);
          resolve({ ok: true, status: res.statusCode, data });
        } catch {
          resolve({ ok: false, error: 'Invalid JSON' });
        }
      });
    });
    req.on('error', (e) => resolve({ ok: false, error: e.message }));
    req.on('timeout', () => {
      req.destroy();
      resolve({ ok: false, error: 'Timeout' });
    });
  });
}

async function checkMemory() {
  return new Promise((resolve) => {
    const payload = JSON.stringify({ query: 'test', limit: 1 });
    const options = {
      hostname: 'localhost',
      port: 8006,
      path: '/memory/search',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': '***' + TOKEN,
        'Content-Length': Buffer.byteLength(payload)
      }
    };
    
    const req = http.request(options, (res) => {
      let body = '';
      res.on('data', c => body += c);
      res.on('end', () => {
        try {
          const data = JSON.parse(body);
          resolve({ ok: true, count: data.count || 0 });
        } catch {
          resolve({ ok: false, error: 'Invalid JSON' });
        }
      });
    });
    req.on('error', (e) => resolve({ ok: false, error: e.message }));
    req.write(payload);
    req.end();
  });
}

async function main() {
  log('=== Xavier BPM Health Check ===');
  
  const health = await checkHealth();
  const memory = await checkMemory();
  
  const report = {
    timestamp: new Date().toISOString(),
    health: health.ok ? 'OK' : 'FAIL',
    healthDetails: health.data || { error: health.error },
    memory: memory.ok ? 'OK' : 'FAIL',
    memoryCount: memory.count || 0,
    alerts: []
  };
  
  if (!health.ok) {
    report.alerts.push('Health check failed: ' + health.error);
  }
  
  if (!memory.ok) {
    report.alerts.push('Memory search failed: ' + memory.error);
  }
  
  if (health.ok && health.data) {
    if (health.data.lag_ms > 5000) {
      report.alerts.push('High lag: ' + health.data.lag_ms + 'ms');
    }
    if (health.data.save_ok_rate && health.data.save_ok_rate < 0.8) {
      report.alerts.push('Low save rate: ' + (health.data.save_ok_rate * 100).toFixed(1) + '%');
    }
  }
  
  // Save report
  const reportDir = path.join(os.homedir(), '.openclaw', 'xavier-reports');
  fs.mkdirSync(reportDir, { recursive: true });
  const reportFile = path.join(reportDir, 'bpm-health-' + new Date().toISOString().slice(0,10) + '.json');
  fs.writeFileSync(reportFile, JSON.stringify(report, null, 2));
  
  log('Health: ' + report.health + ', Memory: ' + report.memory + ', Count: ' + report.memoryCount);
  
  if (report.alerts.length > 0) {
    log('ALERTS:');
    report.alerts.forEach(a => log('  - ' + a));
    process.exit(1);
  } else {
    log('All checks passed');
    process.exit(0);
  }
}

main().catch(e => {
  log('Error: ' + e.message);
  process.exit(1);
});
