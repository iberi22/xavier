import { getApiUrl } from "./client";

export interface TelemetryErrorReport {
  id: string;
  message: string;
  stack?: string;
  componentStack?: string;
  timestamp: string;
  userAgent: string;
  url: string;
  workspaceId: string;
  metadata?: Record<string, unknown>;
}

export const TELEMETRY_QUEUE_KEY = "xavier_telemetry_queue";

const SENSITIVE_PATTERNS = [
  /bearer\s+[a-zA-Z0-9_\-\.\~]+/gi,
  /sk-[a-zA-Z0-9]{20,}/g,
  /eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}/g,
  /("?(?:token|api_key|key|password|secret|authorization)"?\s*:\s*")([^"]+)(")/gi,
  /((?:token|key|password|secret|auth)=)([^\s&"']+)/gi,
];

/**
 * Redacts tokens, API keys, JWTs, and passwords from telemetry output.
 */
export function sanitizeTelemetryText(input: string): string {
  if (!input) return "";
  let sanitized = input;
  for (const pattern of SENSITIVE_PATTERNS) {
    sanitized = sanitized.replace(pattern, (match, ...groups) => {
      if (
        groups.length >= 3 &&
        typeof groups[0] === "string" &&
        typeof groups[1] === "string" &&
        typeof groups[2] === "string"
      ) {
        return `${groups[0]}[REDACTED]${groups[2]}`;
      }
      if (
        groups.length >= 2 &&
        typeof groups[0] === "string" &&
        typeof groups[1] === "string"
      ) {
        return `${groups[0]}[REDACTED]`;
      }
      return "[REDACTED_SECRET]";
    });
  }
  return sanitized;
}

/**
 * Deeply sanitizes any object by masking sensitive keys and redacting values.
 */
export function sanitizeObject<T>(obj: T): T {
  if (obj === null || obj === undefined) return obj;
  if (typeof obj === "string") {
    return sanitizeTelemetryText(obj) as unknown as T;
  }
  if (Array.isArray(obj)) {
    return obj.map((item) => sanitizeObject(item)) as unknown as T;
  }
  if (typeof obj === "object") {
    const sanitizedObj: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj)) {
      const lowerKey = key.toLowerCase();
      if (
        lowerKey.includes("token") ||
        lowerKey.includes("key") ||
        lowerKey.includes("secret") ||
        lowerKey.includes("password") ||
        lowerKey.includes("auth") ||
        lowerKey.includes("credential")
      ) {
        sanitizedObj[key] = "[REDACTED]";
      } else {
        sanitizedObj[key] = sanitizeObject(value);
      }
    }
    return sanitizedObj as unknown as T;
  }
  return obj;
}

/**
 * Generates a clean, credential-scrubbed TelemetryErrorReport object.
 */
export function generateDiagnosticReport(
  error: Error | null,
  errorInfo?: { componentStack?: string | null } | null,
  metadata?: Record<string, unknown>,
): TelemetryErrorReport {
  const activeWorkspace =
    typeof localStorage !== "undefined"
      ? localStorage.getItem("xavier_active_workspace") || "default"
      : "default";

  const rawReport: TelemetryErrorReport = {
    id: `err_${Date.now()}_${Math.random().toString(36).substring(2, 7)}`,
    message: error?.message || "Unknown Application Error",
    stack: error?.stack,
    componentStack: errorInfo?.componentStack ?? undefined,
    timestamp: new Date().toISOString(),
    userAgent: typeof navigator !== "undefined" ? navigator.userAgent : "Unknown",
    url: typeof window !== "undefined" ? window.location.href : "Unknown",
    workspaceId: activeWorkspace,
    metadata,
  };

  return sanitizeObject(rawReport);
}

/**
 * Retrieves buffered telemetry error reports from localStorage.
 */
export function getBufferedErrors(): TelemetryErrorReport[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(TELEMETRY_QUEUE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as TelemetryErrorReport[];
  } catch {
    return [];
  }
}

/**
 * Buffers a single telemetry report to localStorage queue.
 */
export function bufferErrorReport(report: TelemetryErrorReport): void {
  if (typeof localStorage === "undefined") return;
  try {
    const current = getBufferedErrors();
    const updated = [...current, report].slice(-50);
    localStorage.setItem(TELEMETRY_QUEUE_KEY, JSON.stringify(updated));
  } catch (e) {
    console.warn("Failed to buffer telemetry report:", e);
  }
}

/**
 * Clears all buffered telemetry reports in localStorage.
 */
export function clearTelemetryBuffer(): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.removeItem(TELEMETRY_QUEUE_KEY);
  } catch (e) {
    console.warn("Failed to clear telemetry buffer:", e);
  }
}

/**
 * Transmits a telemetry report payload to the backend server.
 */
export async function sendTelemetryReport(
  report: TelemetryErrorReport,
): Promise<boolean> {
  const sanitized = sanitizeObject(report);
  try {
    let token: string | null = null;
    if (typeof localStorage !== "undefined") {
      const authRaw = localStorage.getItem("auth-storage");
      if (authRaw) {
        try {
          const authData = JSON.parse(authRaw);
          token = authData?.state?.token ?? null;
        } catch {
          // ignore parse errors
        }
      }
    }

    const response = await fetch(getApiUrl("/v1/telemetry/errors"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(token ? { "X-Xavier-Token": token } : {}),
      },
      body: JSON.stringify(sanitized),
    });

    return response.ok;
  } catch {
    return false;
  }
}

/**
 * High-level error reporter that transmits immediately or queues offline.
 */
export async function reportError(
  error: Error | null,
  errorInfo?: { componentStack?: string | null } | null,
  metadata?: Record<string, unknown>,
): Promise<TelemetryErrorReport> {
  const report = generateDiagnosticReport(error, errorInfo, metadata);

  const isOnline =
    typeof navigator !== "undefined" ? navigator.onLine !== false : true;

  if (!isOnline) {
    bufferErrorReport(report);
    return report;
  }

  const success = await sendTelemetryReport(report);
  if (!success) {
    bufferErrorReport(report);
  }

  return report;
}

/**
 * Attempts to flush queued error reports to the backend server.
 */
export async function flushTelemetryBuffer(): Promise<number> {
  const buffered = getBufferedErrors();
  if (buffered.length === 0) return 0;

  const remaining: TelemetryErrorReport[] = [];
  let flushedCount = 0;

  for (const report of buffered) {
    const success = await sendTelemetryReport(report);
    if (success) {
      flushedCount += 1;
    } else {
      remaining.push(report);
    }
  }

  if (typeof localStorage !== "undefined") {
    if (remaining.length === 0) {
      localStorage.removeItem(TELEMETRY_QUEUE_KEY);
    } else {
      localStorage.setItem(TELEMETRY_QUEUE_KEY, JSON.stringify(remaining));
    }
  }

  return flushedCount;
}

let isListenerAttached = false;

export function setupTelemetryOnlineListener(): void {
  if (typeof window === "undefined" || isListenerAttached) return;

  window.addEventListener("online", () => {
    void flushTelemetryBuffer();
  });
  isListenerAttached = true;
}

if (typeof window !== "undefined") {
  setupTelemetryOnlineListener();
}
