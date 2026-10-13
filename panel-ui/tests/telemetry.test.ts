import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  generateDiagnosticReport,
  reportError,
  sanitizeObject,
  sanitizeTelemetryText,
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
        "Error with bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c and REDACTED and password=Secret123Pass!";
      const sanitized = sanitizeTelemetryText(rawText);

      expect(sanitized).not.toContain("eyJhbGciOiJIUzI1Ni");
      expect(sanitized).not.toContain("REDACTED");
      expect(sanitized).not.toContain("Secret123Pass!");
      expect(sanitized).toContain("[REDACTED");
    });

    it("deeply redacts sensitive keys in objects", () => {
      const sensitiveObj = {
        message: "Failed request",
        apiKey: "REDACTED",
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
      const err = new Error("Failed to render component REDACTED");
      err.stack = "Error: Failed\n    at Component (REDACTED)";

      const report = generateDiagnosticReport(err, {
        componentStack: "\n    in FaultyComponent",
      });

      expect(report.id).toMatch(/^err_\d+_/);
      expect(report.message).not.toContain("REDACTED");
      expect(report.stack).not.toContain("REDACTED");
      expect(report.componentStack).toContain("FaultyComponent");
      expect(report.timestamp).toBeTruthy();
    });
  });

  describe("Error Reporting", () => {
    it("generates a report locally and returns false without sending", async () => {
      const consoleWarnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      const err = new Error("Test error reporting");
      const result = await reportError(err);

      expect(result).toBe(false);
      expect(consoleWarnSpy).toHaveBeenCalledWith("Remote error reporting is unavailable.");

      consoleWarnSpy.mockRestore();
    });
  });
});
