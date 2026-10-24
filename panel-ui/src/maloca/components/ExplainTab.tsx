import React, { useState } from "react";
import { FileCode, AlertTriangle } from "lucide-react";
import {
  malocaApi,
  ExplainAudience,
  ExplainRequest,
  ExplainResponse,
  MalocaHttpError,
} from "../api";
import { ErrorBoundary } from "../../components/ErrorBoundary";
import { RenderSurface } from "../../components/OpenUi/RenderSurface";
import { parseOpenUi, type OpenUiNode } from "../../utils/openuiParser";

export interface ExplainFormState {
  file: string;
  startLine: string;
  endLine: string;
  symbol: string;
  audience: ExplainAudience;
}

export const EMPTY_EXPLAIN_FORM: ExplainFormState = {
  file: "",
  startLine: "",
  endLine: "",
  symbol: "",
  audience: "newcomer",
};

/**
 * Builds the POST body from the form. Returns null while the required fields are
 * missing or not whole numbers. Range checks stay with the server (400).
 */
export function buildExplainRequest(form: ExplainFormState): ExplainRequest | null {
  const file = form.file.trim();
  const startLine = Number(form.startLine);
  const endLine = Number(form.endLine);
  if (!file || form.startLine.trim() === "" || form.endLine.trim() === "") return null;
  if (!Number.isInteger(startLine) || !Number.isInteger(endLine)) return null;

  const symbol = form.symbol.trim();
  return {
    file,
    start_line: startLine,
    end_line: endLine,
    audience: form.audience,
    ...(symbol ? { symbol } : {}),
  };
}

/** Maps a failed explain call to the message shown to the user. */
export function explainErrorMessage(err: unknown): string {
  const message = err instanceof Error ? err.message : "";
  if (err instanceof MalocaHttpError) {
    switch (err.status) {
      case 400:
        return message || "The request was rejected by the server.";
      case 404:
        return "Explain is not available on this daemon.";
      case 502:
        return "Model unavailable. Try again later.";
    }
  }
  return message || "Explain failed.";
}

/** Non-empty parsed OpenUI nodes, or null when the markup should fall back to plain text. */
export function renderableOpenUi(markup: string): OpenUiNode[] | null {
  const nodes = parseOpenUi(markup);
  return nodes && nodes.length > 0 ? nodes : null;
}

const INPUT_CLASS =
  "w-full px-3 py-2 text-xs font-mono bg-black/50 border border-white/10 rounded-lg text-white placeholder:text-white/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500";

function PlainTextFallback({ text }: { text: string }) {
  return (
    <div className="text-sm leading-relaxed text-white/80 whitespace-pre-wrap" data-testid="explain-plain-text">
      {text}
    </div>
  );
}

function ExplainResult({ result }: { result: ExplainResponse }) {
  const nodes = renderableOpenUi(result.openui_lang);
  const citedRefs = result.cited_refs ?? [];
  const unknownRefs = result.unknown_refs ?? [];
  const skippedSources = result.skipped_sources ?? [];

  return (
    <div className="space-y-4">
      <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
        {nodes ? (
          <ErrorBoundary fallback={<PlainTextFallback text={result.plain_text} />}>
            <RenderSurface nodes={nodes} />
          </ErrorBoundary>
        ) : (
          <PlainTextFallback text={result.plain_text} />
        )}
      </div>

      {citedRefs.length > 0 && (
        <div>
          <h4 className="text-xs font-mono uppercase text-white/50 mb-2">Cited references</h4>
          <ul aria-label="Cited references" className="space-y-1">
            {citedRefs.map((ref) => (
              <li key={ref} className="text-xs font-mono text-emerald-300 break-all">
                {ref}
              </li>
            ))}
          </ul>
        </div>
      )}

      {unknownRefs.length > 0 && (
        <div
          role="alert"
          className="p-3 bg-amber-950/30 border border-amber-800/50 rounded-lg text-amber-300 text-xs"
        >
          <div className="flex items-center gap-2 font-semibold mb-1">
            <AlertTriangle size={14} aria-hidden="true" />
            Not backed by evidence
          </div>
          <ul aria-label="Unverified references" className="space-y-0.5 font-mono break-all">
            {unknownRefs.map((ref) => (
              <li key={ref}>{ref}</li>
            ))}
          </ul>
        </div>
      )}

      {skippedSources.length > 0 && (
        <ul aria-label="Skipped sources" className="space-y-0.5 text-[11px] text-white/40 font-mono">
          {skippedSources.map(([source, reason]) => (
            <li key={`${source}:${reason}`}>
              Skipped {source}: {reason}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ExplainTab() {
  const [form, setForm] = useState<ExplainFormState>(EMPTY_EXPLAIN_FORM);
  const [result, setResult] = useState<ExplainResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const body = buildExplainRequest(form);

  const update = <K extends keyof ExplainFormState>(key: K, value: ExplainFormState[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
  };

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!body || loading) return;
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      setResult(await malocaApi.explain(body));
    } catch (err) {
      setError(explainErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold text-white flex items-center gap-2">
          <FileCode size={18} className="text-emerald-400" />
          Explain code
        </h3>
        <p className="text-xs text-white/50 mt-1">
          Explain a line range of a repository file. Citations are checked against the evidence.
        </p>
      </div>

      <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2" aria-label="Explain request">
        <label className="sm:col-span-2 space-y-1 text-xs text-white/60">
          File path
          <input
            className={INPUT_CLASS}
            value={form.file}
            onChange={(e) => update("file", e.target.value)}
            placeholder="src/server/explain_route.rs"
            required
          />
        </label>
        <label className="space-y-1 text-xs text-white/60">
          Start line
          <input
            className={INPUT_CLASS}
            type="number"
            min={1}
            value={form.startLine}
            onChange={(e) => update("startLine", e.target.value)}
            required
          />
        </label>
        <label className="space-y-1 text-xs text-white/60">
          End line
          <input
            className={INPUT_CLASS}
            type="number"
            min={1}
            value={form.endLine}
            onChange={(e) => update("endLine", e.target.value)}
            required
          />
        </label>
        <label className="space-y-1 text-xs text-white/60">
          Symbol (optional)
          <input
            className={INPUT_CLASS}
            value={form.symbol}
            onChange={(e) => update("symbol", e.target.value)}
          />
        </label>
        <label className="space-y-1 text-xs text-white/60">
          Audience
          <select
            className={INPUT_CLASS}
            value={form.audience}
            onChange={(e) => update("audience", e.target.value as ExplainAudience)}
          >
            <option value="newcomer">Newcomer</option>
            <option value="maintainer">Maintainer</option>
          </select>
        </label>
        <div className="sm:col-span-2">
          <button
            type="submit"
            disabled={!body || loading}
            className="px-4 py-2 text-xs font-mono bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 rounded-lg transition-all disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {loading ? "Explaining..." : "Explain"}
          </button>
        </div>
      </form>

      {error && (
        <div role="alert" className="p-3 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
          {error}
        </div>
      )}

      {result && <ExplainResult result={result} />}
    </div>
  );
}
