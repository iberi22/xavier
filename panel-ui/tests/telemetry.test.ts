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
