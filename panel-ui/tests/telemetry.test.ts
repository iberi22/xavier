import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  bufferErrorReport,
  clearTelemetryBuffer,
  flushTelemetryBuffer,
  generateDiagnosticReport,
  getBufferedErrors,
  reportError,
  sanitizeObject,
  sanitizeTelemetryText,
  TELEMETRY_QUEUE_KEY,
} from "../src/api/telemetry";

describe("Telemetry Subsystem", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  describe("Sanitization Guard", () => {
    it("redacts sensitive bearer tokens, API keys, and passwords in text", () => {
      const rawText =
        "Error with bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c and sk-proj1234567890abcdef1234567890 and password=Secret123Pass!";
      const sanitized = sanitizeTelemetryText(rawText);

      expect(sanitized).not.toContain("eyJhbGciOiJIUzI1Ni");
      expect(sanitized).not.toContain("sk-proj1234567890abcdef");
      expect(sanitized).not.toContain("Secret123Pass!");
      expect(sanitized).toContain("[REDACTED");
    });

    it("deeply redacts sensitive keys in objects", () => {
      const sensitiveObj = {
        message: "Failed request",
        apiKey: "sk-secretkey1234567890123",
        token: "bearer-xyz-123456",
        user: {
          name: "Alice",
          password: "my-super-secret-password",
        },
      };

      const sanitized = sanitizeObject(sensitiveObj);

      expect(sanitized.message).toBe("Failed request");
      expect(sanitized.apiKey).toBe("[REDACTED]");
      expect(sanitized.token).toBe("[REDACTED]");
      expect(sanitized.user.name).toBe("Alice");
      expect(sanitized.user.password).toBe("[REDACTED]");
    });
  });

  describe("Diagnostic Report Generation", () => {
    it("generates a clean telemetry error report without credentials", () => {
      const err = new Error("Failed to render component sk-secret123456789012345678");
      err.stack = "Error: Failed\n    at Component (sk-secret123456789012345678)";

      const report = generateDiagnosticReport(err, {
        componentStack: "\n    in FaultyComponent",
      });

      expect(report.id).toMatch(/^err_\d+_/);
      expect(report.message).not.toContain("sk-secret123456789012345678");
      expect(report.stack).not.toContain("sk-secret123456789012345678");
      expect(report.componentStack).toContain("FaultyComponent");
      expect(report.timestamp).toBeTruthy();
    });
  });

  describe("Offline Queueing and Buffering", () => {
    it("buffers error reports in localStorage when offline", async () => {
      // Mock offline state
      vi.stubGlobal("navigator", { onLine: false, userAgent: "Vitest" });

      const err = new Error("Network offline test");
      const report = await reportError(err);

      const buffered = getBufferedErrors();
      expect(buffered).toHaveLength(1);
      expect(buffered[0].message).toBe("Network offline test");
      expect(report.id).toBe(buffered[0].id);
    });

    it("buffers error report when online API request fails", async () => {
      vi.stubGlobal("navigator", { onLine: true, userAgent: "Vitest" });
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));

      const err = new Error("Backend server down");
      await reportError(err);

      const buffered = getBufferedErrors();
      expect(buffered).toHaveLength(1);
      expect(buffered[0].message).toBe("Backend server down");
    });

    it("flushes buffered error reports upon successful reconnection", async () => {
      vi.stubGlobal("navigator", { onLine: true, userAgent: "Vitest" });

      const mockReport = generateDiagnosticReport(new Error("Queued error"));
      bufferErrorReport(mockReport);

      expect(getBufferedErrors()).toHaveLength(1);

      // Mock successful fetch
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: async () => ({ status: "ok" }),
        }),
      );

      const flushedCount = await flushTelemetryBuffer();

      expect(flushedCount).toBe(1);
      expect(getBufferedErrors()).toHaveLength(0);
      expect(localStorage.getItem(TELEMETRY_QUEUE_KEY)).toBeNull();
    });

    it("flushes buffer automatically when window fires online event", async () => {
      const mockReport = generateDiagnosticReport(new Error("Event queued error"));
      bufferErrorReport(mockReport);

      const fetchMock = vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ status: "ok" }),
      });
      vi.stubGlobal("fetch", fetchMock);

      window.dispatchEvent(new Event("online"));

      await vi.waitFor(() => {
        expect(fetchMock).toHaveBeenCalled();
        expect(getBufferedErrors()).toHaveLength(0);
      });
    });
  });
});
