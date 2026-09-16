import React, { Component, type ErrorInfo, type ReactNode } from "react";
import {
  generateDiagnosticReport,
  reportError,
  type TelemetryErrorReport,
} from "../api/telemetry";

export interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: ReactNode | ((error: Error, resetError: () => void) => ReactNode);
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
  onReload?: () => void;
}

export interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
  copied: boolean;
  telemetryReport: TelemetryErrorReport | null;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  public override state: ErrorBoundaryState = {
    hasError: false,
    error: null,
    errorInfo: null,
    copied: false,
    telemetryReport: null,
  };

  public static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return {
      hasError: true,
      error,
    };
  }

  public override componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    this.setState({ errorInfo });

    if (this.props.onError) {
      this.props.onError(error, errorInfo);
    }

    void reportError(error, errorInfo).then((report) => {
      this.setState({ telemetryReport: report });
    });
  }

  private handleReset = (): void => {
    this.setState({
      hasError: false,
      error: null,
      errorInfo: null,
      copied: false,
      telemetryReport: null,
    });
  };

  private handleReload = (): void => {
    if (this.props.onReload) {
      this.props.onReload();
    } else if (typeof window !== "undefined") {
      window.location.reload();
    }
    this.handleReset();
  };

  private handleCopyDiagnostic = async (): Promise<void> => {
    const report =
      this.state.telemetryReport ||
      generateDiagnosticReport(this.state.error, this.state.errorInfo);

    const formattedPayload = JSON.stringify(report, null, 2);

    try {
      if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(formattedPayload);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = formattedPayload;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        document.body.removeChild(textarea);
      }
      this.setState({ copied: true });
      setTimeout(() => {
        this.setState({ copied: false });
      }, 2000);
    } catch (e) {
      console.warn("Failed to copy error diagnostic:", e);
    }
  };

  public override render(): ReactNode {
    if (this.state.hasError) {
      if (this.props.fallback) {
        if (typeof this.props.fallback === "function") {
          return this.props.fallback(
            this.state.error || new Error("Unknown error"),
            this.handleReset,
          );
        }
        return this.props.fallback;
      }

      const errorMessage =
        this.state.error?.message || "An unexpected error has occurred in the application interface.";

      return (
        <div className="w-full h-screen bg-[#111215] text-[#e5e7eb] font-sans flex items-center justify-center p-4 relative overflow-hidden select-none">
          <div className="absolute inset-0 bg-radial from-red-500/5 via-transparent to-transparent pointer-events-none" />

          <div className="glass-panel bg-[#191a1e]/90 border border-red-500/30 rounded-2xl p-6 sm:p-8 max-w-lg w-full text-center shadow-2xl space-y-6 backdrop-blur-xl relative z-10">
            <div className="w-14 h-14 rounded-2xl bg-red-500/10 border border-red-500/30 flex items-center justify-center mx-auto text-red-400 shadow-inner">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="w-7 h-7"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
                <line x1="12" y1="9" x2="12" y2="13" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
              </svg>
            </div>

            <div className="space-y-2">
              <h2 className="text-lg sm:text-xl font-bold uppercase tracking-wider text-white">
                Application Rendering Exception
              </h2>
              <p className="text-xs sm:text-sm text-[#8c909b] leading-relaxed">
                The interface encountered an unhandled exception. Diagnostic telemetry has been captured locally.
              </p>
            </div>

            <div className="bg-[#111215]/80 border border-white/10 rounded-xl p-3 sm:p-4 text-left font-mono text-xs text-red-300/90 overflow-x-auto max-h-36 studio-dark-scrollbar">
              <span className="text-red-400 font-semibold block mb-1">
                {this.state.error?.name || "Error"}:
              </span>
              <p className="break-words whitespace-pre-wrap">{errorMessage}</p>
            </div>

            <div className="flex flex-col sm:flex-row items-center justify-center gap-3 pt-2">
              <button
                type="button"
                onClick={this.handleReload}
                className="w-full sm:w-auto flex-1 py-2.5 px-4 rounded-lg bg-red-500/20 text-red-300 border border-red-500/40 hover:bg-red-500/30 active:scale-98 font-semibold text-xs tracking-wider uppercase transition-all duration-150 flex items-center justify-center gap-2 cursor-pointer"
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  className="w-4 h-4"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                  <path d="M3 3v5h5" />
                  <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
                  <path d="M16 16h5v5" />
                </svg>
                Reload Interface
              </button>

              <button
                type="button"
                onClick={this.handleCopyDiagnostic}
                className="w-full sm:w-auto flex-1 py-2.5 px-4 rounded-lg bg-white/5 text-white/90 border border-white/10 hover:bg-white/10 active:scale-98 font-semibold text-xs tracking-wider uppercase transition-all duration-150 flex items-center justify-center gap-2 cursor-pointer"
              >
                {this.state.copied ? (
                  <>
                    <svg
                      xmlns="http://www.w3.org/2000/svg"
                      className="w-4 h-4 text-emerald-400"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden="true"
                    >
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                    <span className="text-emerald-400">Copied!</span>
                  </>
                ) : (
                  <>
                    <svg
                      xmlns="http://www.w3.org/2000/svg"
                      className="w-4 h-4"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden="true"
                    >
                      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
                      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
                    </svg>
                    Copy Error Diagnostic
                  </>
                )}
              </button>
            </div>

            <div className="text-[10px] text-[#8c909b] pt-2 border-t border-white/5 font-mono">
              Antigravity Studio Dark · Xavier Telemetry Subsystem
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
