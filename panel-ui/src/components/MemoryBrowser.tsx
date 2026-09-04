import {
  ChevronLeft,
  ChevronRight,
  Download,
  Filter,
  Plus,
  Search,
  X,
} from "lucide-react";
import type React from "react";
import { memo, useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";
import { useDebounce } from "../hooks/useDebounce";
import type { MemoryEntry } from "../types";

const PAGE_SIZE = 20;

const KIND_OPTIONS = [
  "",
  "note",
  "fact",
  "preference",
  "context",
  "task",
  "agent",
];

interface MemoryBrowserProps {
  token: string;
}

export default function MemoryBrowser({ token }: MemoryBrowserProps) {
  const api = new ApiClient(token);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("");
  const [memories, setMemories] = useState<MemoryEntry[]>([]);
  const [page, setPage] = useState(1);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Add memory form
  const [showAdd, setShowAdd] = useState(false);
  const [newContent, setNewContent] = useState("");
  const [newKind, setNewKind] = useState("note");
  const [adding, setAdding] = useState(false);
  const [exporting, setExporting] = useState(false);
  const debouncedQuery = useDebounce(query, 300);

  const doSearch = useCallback(
    (q: string, k: string, p: number) => {
      setLoading(true);
      setError(null);
      api
        .searchMemories(q, k || undefined, PAGE_SIZE * p)
        .then((results) => {
          const start = (p - 1) * PAGE_SIZE;
          setMemories(results.slice(start, start + PAGE_SIZE));
        })
        .catch((e: Error) => setError(e.message))
        .finally(() => setLoading(false));
    },
    [token],
  );

  useEffect(() => {
    doSearch(debouncedQuery, kind, page);

    const handleWorkspaceChange = () => {
      doSearch(debouncedQuery, kind, page);
    };

    window.addEventListener("xavier:workspace-changed", handleWorkspaceChange);
    return () => {
      window.removeEventListener("xavier:workspace-changed", handleWorkspaceChange);
    };
  }, [debouncedQuery, kind, page, doSearch]);

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newContent.trim()) return;
    setAdding(true);
    try {
      await api.addMemory(newContent.trim(), newKind);
      setNewContent("");
      setShowAdd(false);
      doSearch(debouncedQuery, kind, page);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add memory");
    } finally {
      setAdding(false);
    }
  };

  const handleExportMarkdown = async () => {
    setExporting(true);
    try {
      const data = await api.exportMarkdown();
      const content =
        typeof data === "string"
          ? data
          : JSON.stringify(data, null, 2);
      const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "memories-export.md";
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);

      if (typeof window !== "undefined" && window.dispatchEvent) {
        window.dispatchEvent(
          new CustomEvent("xavier-error-toast", {
            detail: { message: "Markdown export downloaded successfully!" },
          }),
        );
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to export markdown vault",
      );
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="flex flex-col h-full p-4 sm:p-6 text-slate-900 dark:text-white space-y-4 sm:space-y-6 overflow-y-auto">
      <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
        <div>
          <h2 className="text-2xl sm:text-3xl font-light tracking-tight">Memory Browser</h2>
          <p className="text-xs sm:text-sm text-slate-600 dark:text-white/40 mt-1">
            Search and browse the shared memory store
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={handleExportMarkdown}
            disabled={exporting}
            type="button"
            aria-label="Export Markdown"
            className="flex items-center gap-2 px-3 sm:px-4 py-2 border border-slate-200 dark:border-white/10 bg-white/80 dark:bg-black/40 text-slate-800 dark:text-white rounded-xl text-xs sm:text-sm font-semibold hover:bg-slate-100 dark:hover:bg-white/10 disabled:opacity-50 transition-all"
          >
            <Download size={16} aria-hidden="true" />
            {exporting ? "Exporting..." : "Export Markdown"}
          </button>
          <button
            onClick={() => setShowAdd(!showAdd)}
            type="button"
            aria-label="Add Memory"
            className="flex items-center gap-2 px-3 sm:px-4 py-2 bg-emerald-500 dark:bg-[#39ff14] text-white dark:text-black rounded-xl text-xs sm:text-sm font-bold hover:shadow-[0_0_15px_rgba(57,255,20,0.4)] transition-all"
          >
            <Plus size={16} aria-hidden="true" />
            Add Memory
          </button>
        </div>
      </div>

      {/* Add Memory Form */}
      {showAdd && (
        <form
          onSubmit={handleAdd}
          className="bg-white/80 dark:bg-black/40 backdrop-blur-md rounded-2xl border border-slate-200 dark:border-white/10 p-4 sm:p-5 space-y-4 shadow-sm"
        >
          <div className="flex items-center justify-between">
            <h3 className="font-semibold text-slate-900 dark:text-white/90">Add New Memory</h3>
            <button
              type="button"
              onClick={() => setShowAdd(false)}
              aria-label="Close add memory form"
            >
              <X
                size={16}
                className="text-slate-400 dark:text-white/40 hover:text-slate-900 dark:hover:text-white"
                aria-hidden="true"
              />
            </button>
          </div>
          <textarea
            value={newContent}
            onChange={(e) => setNewContent(e.target.value)}
            placeholder="Memory content..."
            aria-label="Memory content"
            rows={3}
            className="w-full px-3 py-2 rounded-xl border border-slate-200 dark:border-white/10 bg-white/90 dark:bg-black/50 text-slate-900 dark:text-white text-sm resize-none focus:outline-none focus:border-emerald-500 dark:focus:border-[#39ff14] transition-colors"
          />
          <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-3">
            <select
              value={newKind}
              onChange={(e) => setNewKind(e.target.value)}
              aria-label="Memory kind"
              className="px-3 py-2 rounded-xl border border-slate-200 dark:border-white/10 bg-white dark:bg-black/50 text-slate-900 dark:text-white text-sm focus:outline-none focus:border-emerald-500 dark:focus:border-[#39ff14] appearance-none cursor-pointer"
            >
              {KIND_OPTIONS.slice(1).map((k) => (
                <option key={k} value={k} className="bg-white dark:bg-stone-900 text-slate-900 dark:text-white">
                  {k}
                </option>
              ))}
            </select>
            <button
              type="submit"
              disabled={adding || !newContent.trim()}
              className="px-4 py-2 bg-emerald-500 dark:bg-[#39ff14] text-white dark:text-black rounded-xl text-sm font-bold hover:shadow-[0_0_10px_rgba(57,255,20,0.4)] disabled:opacity-50 transition-all"
            >
              {adding ? "Saving..." : "Save"}
            </button>
          </div>
        </form>
      )}

      {/* Search & Filters */}
      <div className="flex flex-col sm:flex-row gap-3 md:gap-4 lg:gap-6">
        <div className="relative flex-1">
          <Search
            size={16}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400 dark:text-white/40"
            aria-hidden="true"
          />
          <input
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setPage(1);
            }}
            placeholder="Search memories..."
            aria-label="Search memories"
            className="w-full pl-9 pr-4 py-2 rounded-xl border border-slate-200 dark:border-white/10 bg-white/80 dark:bg-black/30 text-slate-900 dark:text-white text-sm focus:outline-none focus:border-emerald-500 dark:focus:border-[#39ff14] transition-colors"
          />
        </div>
        <div className="flex items-center gap-2">
          <Filter size={16} className="text-slate-400 dark:text-white/40" aria-hidden="true" />
          <select
            value={kind}
            onChange={(e) => {
              setKind(e.target.value);
              setPage(1);
            }}
            aria-label="Filter by kind"
            className="px-3 py-2 rounded-xl border border-slate-200 dark:border-white/10 bg-white/80 dark:bg-black/30 text-slate-900 dark:text-white text-sm focus:outline-none focus:border-emerald-500 dark:focus:border-[#39ff14] appearance-none cursor-pointer min-w-[120px]"
          >
            <option value="" className="bg-white dark:bg-stone-900 text-slate-900 dark:text-white">
              All kinds
            </option>
            {KIND_OPTIONS.slice(1).map((k) => (
              <option key={k} value={k} className="bg-white dark:bg-stone-900 text-slate-900 dark:text-white">
                {k}
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Error */}
      {error && <div className="text-red-500 text-sm">Error: {error}</div>}

      {/* Loading */}
      {loading && <div className="text-slate-500 dark:text-white/40 text-sm">Searching...</div>}

      {/* Results */}
      {!loading && (
        <>
          {memories.length === 0 ? (
            <div className="text-center py-12 text-slate-400 dark:text-white/20">
              No memories found
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 sm:gap-4 lg:gap-6">
              {memories.map((m) => (
                <MemoryCard key={m.id} memory={m} />
              ))}
            </div>
          )}

          {/* Pagination */}
          {memories.length > 0 && (
            <div className="flex items-center justify-center gap-3 pt-4 pb-8">
              <button
                type="button"
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={page === 1}
                aria-label="Previous page"
                className="p-2 rounded-xl border border-slate-200 dark:border-white/10 text-slate-600 dark:text-white/60 disabled:opacity-20 hover:bg-slate-100 dark:hover:bg-white/5 transition-colors"
              >
                <ChevronLeft size={16} aria-hidden="true" />
              </button>
              <span className="text-sm text-slate-500 dark:text-white/40 font-mono">
                Page {page}
              </span>
              <button
                type="button"
                onClick={() => setPage((p) => p + 1)}
                disabled={memories.length < PAGE_SIZE}
                aria-label="Next page"
                className="p-2 rounded-xl border border-slate-200 dark:border-white/10 text-slate-600 dark:text-white/60 disabled:opacity-20 hover:bg-slate-100 dark:hover:bg-white/5 transition-colors"
              >
                <ChevronRight size={16} aria-hidden="true" />
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped MemoryCard in memo()
 * 🎯 Why: MemoryBrowser contains a list of memories and a search/filter state.
 *         Typing in the search input or adding a new memory would re-render the entire list
 *         of MemoryCards, causing unnecessary DOM reconciliation and string parsing.
 * 📊 Impact: O(1) appends and prevents O(N) re-renders when parent state changes.
 */
const MemoryCard = memo(function MemoryCard({ memory }: { memory: MemoryEntry }) {
  const [expanded, setExpanded] = useState(false);

  const kindColor: Record<string, string> = {
    note: "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
    fact: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20",
    preference: "bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20",
    context: "bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20",
    task: "bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/20",
    agent: "bg-slate-100 dark:bg-white/5 text-slate-600 dark:text-white/60 border-slate-200 dark:border-white/10",
  };

  return (
    <div className="bg-white dark:bg-white/5 backdrop-blur-sm rounded-2xl border border-slate-200 dark:border-white/10 p-4 hover:border-slate-300 dark:hover:border-white/20 transition-colors shadow-sm">
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-2 flex-wrap">
            <span
              className={`px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider border ${kindColor[memory.kind] ?? "bg-slate-100 dark:bg-white/5 text-slate-500 dark:text-white/40 border-slate-200 dark:border-white/10"}`}
            >
              {memory.kind}
            </span>
            {memory.source && (
              <span className="text-[10px] text-slate-400 dark:text-white/30 uppercase font-mono">
                {memory.source}
              </span>
            )}
            <span className="text-[10px] text-slate-400 dark:text-white/30 ml-auto font-mono">
              {new Date(memory.created_at).toLocaleDateString()}
            </span>
          </div>
          <p
            className={`text-sm text-slate-800 dark:text-white/80 leading-relaxed ${!expanded && "line-clamp-2"}`}
          >
            {memory.content}
          </p>
          {memory.content.length > 200 && (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className="text-xs text-emerald-600 dark:text-[#39ff14] mt-2 hover:underline font-medium"
            >
              {expanded ? "Show less" : "Show more"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
});
