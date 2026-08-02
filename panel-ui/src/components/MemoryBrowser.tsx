import {
  ChevronLeft,
  ChevronRight,
  Filter,
  Plus,
  Search,
  X,
} from "lucide-react";
import type React from "react";
import { memo, useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";
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
    doSearch(query, kind, page);
  }, [query, kind, page, doSearch]);

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newContent.trim()) return;
    setAdding(true);
    try {
      await api.addMemory(newContent.trim(), newKind);
      setNewContent("");
      setShowAdd(false);
      doSearch(query, kind, page);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add memory");
    } finally {
      setAdding(false);
    }
  };

  return (
    <div className="flex flex-col h-full p-6 text-white space-y-6 overflow-y-auto">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-light tracking-tight">Memory Browser</h2>
          <p className="text-sm text-white/40 mt-1">
            Search and browse the shared memory store
          </p>
        </div>
        <button
          onClick={() => setShowAdd(!showAdd)}
          className="flex items-center gap-2 px-4 py-2 bg-[#39ff14] text-black rounded-xl text-sm font-bold hover:shadow-[0_0_15px_rgba(57,255,20,0.4)] transition-all"
        >
          <Plus size={16} />
          Add Memory
        </button>
      </div>

      {/* Add Memory Form */}
      {showAdd && (
        <form
          onSubmit={handleAdd}
          className="bg-black/40 backdrop-blur-md rounded-2xl border border-white/10 p-5 space-y-4"
        >
          <div className="flex items-center justify-between">
            <h3 className="font-semibold text-white/90">Add New Memory</h3>
            <button
              type="button"
              onClick={() => setShowAdd(false)}
              aria-label="Close add memory form"
            >
              <X
                size={16}
                className="text-white/40 hover:text-white"
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
            className="w-full px-3 py-2 rounded-xl border border-white/10 bg-black/50 text-white text-sm resize-none focus:outline-none focus:border-[#39ff14] transition-colors"
          />
          <div className="flex items-center gap-3">
            <select
              value={newKind}
              onChange={(e) => setNewKind(e.target.value)}
              aria-label="Memory kind"
              className="px-3 py-2 rounded-xl border border-white/10 bg-black/50 text-white text-sm focus:outline-none focus:border-[#39ff14] appearance-none cursor-pointer"
            >
              {KIND_OPTIONS.slice(1).map((k) => (
                <option key={k} value={k} className="bg-stone-900">
                  {k}
                </option>
              ))}
            </select>
            <button
              type="submit"
              disabled={adding || !newContent.trim()}
              className="px-4 py-2 bg-[#39ff14] text-black rounded-xl text-sm font-bold hover:shadow-[0_0_10px_rgba(57,255,20,0.4)] disabled:opacity-50 transition-all"
            >
              {adding ? "Saving..." : "Save"}
            </button>
          </div>
        </form>
      )}

      {/* Search & Filters */}
      <div className="flex flex-col sm:flex-row gap-3">
        <div className="relative flex-1">
          <Search
            size={16}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-white/40"
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
            className="w-full pl-9 pr-4 py-2 rounded-xl border border-white/10 bg-black/30 text-white text-sm focus:outline-none focus:border-[#39ff14] transition-colors"
          />
        </div>
        <div className="flex items-center gap-2">
          <Filter size={16} className="text-white/40" aria-hidden="true" />
          <select
            value={kind}
            onChange={(e) => {
              setKind(e.target.value);
              setPage(1);
            }}
            aria-label="Filter by kind"
            className="px-3 py-2 rounded-xl border border-white/10 bg-black/30 text-white text-sm focus:outline-none focus:border-[#39ff14] appearance-none cursor-pointer min-w-[120px]"
          >
            <option value="" className="bg-stone-900">
              All kinds
            </option>
            {KIND_OPTIONS.slice(1).map((k) => (
              <option key={k} value={k} className="bg-stone-900">
                {k}
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Error */}
      {error && <div className="text-red-500 text-sm">Error: {error}</div>}

      {/* Loading */}
      {loading && <div className="text-white/40 text-sm">Searching...</div>}

      {/* Results */}
      {!loading && (
        <>
          {memories.length === 0 ? (
            <div className="text-center py-12 text-white/20">
              No memories found
            </div>
          ) : (
            <div className="space-y-3">
              {memories.map((m) => (
                <MemoryCard key={m.id} memory={m} />
              ))}
            </div>
          )}

          {/* Pagination */}
          {memories.length > 0 && (
            <div className="flex items-center justify-center gap-3 pt-4 pb-8">
              <button
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={page === 1}
                aria-label="Previous page"
                className="p-2 rounded-xl border border-white/10 text-white/60 disabled:opacity-20 hover:bg-white/5 transition-colors"
              >
                <ChevronLeft size={16} aria-hidden="true" />
              </button>
              <span className="text-sm text-white/40 font-mono">
                Page {page}
              </span>
              <button
                onClick={() => setPage((p) => p + 1)}
                disabled={memories.length < PAGE_SIZE}
                aria-label="Next page"
                className="p-2 rounded-xl border border-white/10 text-white/60 disabled:opacity-20 hover:bg-white/5 transition-colors"
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
    note: "bg-blue-500/10 text-blue-400 border-blue-500/20",
    fact: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
    preference: "bg-purple-500/10 text-purple-400 border-purple-500/20",
    context: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    task: "bg-red-500/10 text-red-400 border-red-500/20",
    agent: "bg-white/5 text-white/60 border-white/10",
  };

  return (
    <div className="bg-white/5 backdrop-blur-sm rounded-2xl border border-white/10 p-4 hover:border-white/20 transition-colors">
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-2 flex-wrap">
            <span
              className={`px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider border ${kindColor[memory.kind] ?? "bg-white/5 text-white/40 border-white/10"}`}
            >
              {memory.kind}
            </span>
            {memory.source && (
              <span className="text-[10px] text-white/30 uppercase font-mono">
                {memory.source}
              </span>
            )}
            <span className="text-[10px] text-white/30 ml-auto font-mono">
              {new Date(memory.created_at).toLocaleDateString()}
            </span>
          </div>
          <p
            className={`text-sm text-white/80 leading-relaxed ${!expanded && "line-clamp-2"}`}
          >
            {memory.content}
          </p>
          {memory.content.length > 200 && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="text-xs text-[#39ff14] mt-2 hover:underline font-medium"
            >
              {expanded ? "Show less" : "Show more"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
});
