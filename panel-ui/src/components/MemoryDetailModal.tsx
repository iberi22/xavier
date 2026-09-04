import { Edit3, ExternalLink, Save, Tag, X } from "lucide-react";
import React, { useState } from "react";
import type { MemoryEntry } from "../types";

interface MemoryDetailModalProps {
  memory: MemoryEntry;
  onClose: () => void;
  onSave?: (updatedContent: string) => Promise<void>;
  onNavigateWikilink?: (target: string) => void;
}

export default function MemoryDetailModal({
  memory,
  onClose,
  onSave,
  onNavigateWikilink,
}: MemoryDetailModalProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [content, setContent] = useState(memory.content);
  const [saving, setSaving] = useState(false);

  // Parse inline wikilinks: [[Target]] or [[Target|Display]]
  const renderFormattedMarkdown = (raw: string) => {
    const parts = raw.split(/(\[\[[^\]]+\]\])/g);
    return parts.map((part, index) => {
      const match = part.match(/^\[\[([^\]\|]+)(?:\|([^\]]+))?\]\]$/);
      if (match) {
        const target = match[1].trim();
        const display = (match[2] || target).trim();
        return (
          <button
            key={index}
            type="button"
            onClick={() => onNavigateWikilink?.(target)}
            className="inline-flex items-center gap-1 text-emerald-600 dark:text-[#39ff14] hover:underline font-mono bg-emerald-500/10 dark:bg-[#39ff14]/10 px-1.5 py-0.5 rounded border border-emerald-500/20 dark:border-[#39ff14]/30 cursor-pointer text-xs align-baseline mx-0.5"
            title={`Go to [[${target}]]`}
          >
            <ExternalLink size={10} aria-hidden="true" />
            <span>{display}</span>
          </button>
        );
      }
      return <span key={index}>{part}</span>;
    });
  };

  const handleSave = async () => {
    if (!onSave) return;
    setSaving(true);
    try {
      await onSave(content);
      setIsEditing(false);
    } catch (err) {
      console.error("Failed to save memory edits:", err);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[120] bg-black/70 backdrop-blur-md flex items-center justify-center p-4 pointer-events-auto font-sans"
      onClick={onClose}
    >
      <div
        className="bg-white dark:bg-[#0a0a0a] border border-slate-200 dark:border-white/10 rounded-2xl w-full max-w-2xl max-h-[85vh] flex flex-col shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-200 dark:border-white/10 bg-slate-50/50 dark:bg-white/[0.02]">
          <div className="flex items-center gap-2.5">
            <span className="px-2.5 py-1 rounded-full text-xs font-bold uppercase tracking-wider bg-emerald-500/10 text-emerald-600 dark:text-[#39ff14] border border-emerald-500/20">
              {memory.kind}
            </span>
            <span className="text-xs font-mono text-slate-500 dark:text-white/40">
              ID: {memory.id.slice(0, 8)}...
            </span>
          </div>
          <div className="flex items-center gap-2">
            {!isEditing ? (
              <button
                type="button"
                onClick={() => setIsEditing(true)}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-white/10 text-xs font-mono text-slate-700 dark:text-white/70 hover:bg-slate-100 dark:hover:bg-white/5 transition-colors cursor-pointer"
              >
                <Edit3 size={13} aria-hidden="true" />
                Edit Note
              </button>
            ) : (
              <button
                type="button"
                disabled={saving}
                onClick={handleSave}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-500 dark:bg-[#39ff14] text-white dark:text-black font-bold text-xs font-mono hover:opacity-90 transition-opacity cursor-pointer disabled:opacity-50"
              >
                <Save size={13} aria-hidden="true" />
                {saving ? "Saving..." : "Save"}
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded-lg text-slate-400 hover:text-slate-900 dark:text-white/40 dark:hover:text-white hover:bg-slate-100 dark:hover:bg-white/5 transition-colors cursor-pointer"
              aria-label="Close note preview"
            >
              <X size={18} aria-hidden="true" />
            </button>
          </div>
        </div>

        {/* Note Metadata Bar */}
        <div className="px-6 py-2.5 bg-slate-100/50 dark:bg-white/[0.01] border-b border-slate-200 dark:border-white/5 flex items-center gap-4 text-xs font-mono text-slate-500 dark:text-white/50 flex-wrap">
          <div className="flex items-center gap-1">
            <Tag size={12} aria-hidden="true" />
            <span>Priority: {memory.priority || "medium"}</span>
          </div>
          <div>Source: {memory.source || "panel-ui"}</div>
          <div className="ml-auto">
            Created: {new Date(memory.created_at).toLocaleString()}
          </div>
        </div>

        {/* Content Body */}
        <div className="p-6 flex-1 overflow-y-auto">
          {isEditing ? (
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              aria-label="Edit memory content"
              rows={14}
              className="w-full h-full p-4 rounded-xl border border-slate-200 dark:border-white/10 bg-slate-50/50 dark:bg-black/50 text-slate-900 dark:text-white text-sm font-mono resize-none focus:outline-none focus:border-emerald-500 dark:focus:border-[#39ff14] transition-colors"
            />
          ) : (
            <div className="prose dark:prose-invert max-w-none text-sm text-slate-800 dark:text-white/90 leading-relaxed font-sans whitespace-pre-wrap">
              {renderFormattedMarkdown(content)}
            </div>
          )}
        </div>

        {/* Footer Obsidian-style tip */}
        <div className="px-6 py-3 border-t border-slate-200 dark:border-white/5 bg-slate-50/50 dark:bg-white/[0.01] flex items-center justify-between text-[11px] font-mono text-slate-400 dark:text-white/30">
          <span>Obsidian-compatible Wikilinks: [[Note Name]]</span>
          <span>Markdown + YAML Frontmatter supported</span>
        </div>
      </div>
    </div>
  );
}
