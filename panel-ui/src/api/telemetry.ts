
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
 * High-level error reporter that logs the error but does not send it remotely.
 */
export async function reportError(
  _error: Error | null,
  _errorInfo?: { componentStack?: string | null } | null,
  _metadata?: Record<string, unknown>,
): Promise<boolean> {
  console.warn("Remote error reporting is unavailable.");
  return false;
}
