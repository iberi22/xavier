import { Check, PenSquare, Plus, Share2, X } from "lucide-react";
import React, { useState } from "react";
import type { BookmarkArtifact } from "../types";

interface BookmarkItemProps {
  bookmark: BookmarkArtifact;
  onPinArtifact: (artifact: BookmarkArtifact) => void;
  onUpdateBookmark: (updated: BookmarkArtifact) => void;
}

const BookmarkItem = React.memo(({ bookmark, onPinArtifact, onUpdateBookmark }: BookmarkItemProps) => {
  const [isEditing, setIsEditing] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editCategory, setEditCategory] = useState("");
  const [editType, setEditType] = useState("");

  const startEdit = () => {
    setIsEditing(true);
    setEditTitle(bookmark.title);
    setEditCategory(bookmark.category || "");
    setEditType(bookmark.type);
  };

  const cancelEdit = () => {
    setIsEditing(false);
  };

  const saveEdit = () => {
    onUpdateBookmark({
      ...bookmark,
      title: editTitle,
      category: editCategory,
      type: editType,
    });
    setIsEditing(false);
  };

  return (
    <div
      className={`p-6 rounded-2xl bg-[#090909] border transition-all group flex flex-col ${isEditing ? "border-[#39ff14]/50 shadow-[0_0_20px_rgba(57,255,20,0.1)]" : "border-[#1a1a1a] hover:border-[#39ff14]/30 hover:shadow-[0_0_20px_rgba(57,255,20,0.05)]"}`}
    >
      <div className="flex justify-between items-start mb-4">
        <div>
          {isEditing ? (
            <input
              type="text"
              value={editType}
              onChange={(e) => setEditType(e.target.value)}
              className="text-[10px] uppercase font-mono text-[#39ff14] px-2 py-1 bg-[#39ff14]/10 rounded border border-[#39ff14]/30 outline-none w-24"
            />
          ) : (
            <span className="text-[10px] uppercase font-mono text-[#39ff14] px-2 py-1 bg-[#39ff14]/10 rounded-full">
              {bookmark.type}
            </span>
          )}
        </div>
        <div
          className={`flex gap-2 transition-all ${isEditing ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
        >
          {isEditing ? (
            <>
              <button
                onClick={cancelEdit}
                className="p-1.5 rounded-lg bg-white/5 text-white/30 hover:text-red-400 hover:bg-red-400/10 transition-all"
                title="Cancel"
              >
                <X className="w-4 h-4" />
              </button>
              <button
                onClick={saveEdit}
                className="p-1.5 rounded-lg bg-[#39ff14]/20 text-[#39ff14] hover:bg-[#39ff14]/30 transition-all"
                title="Save Changes"
              >
                <Check className="w-4 h-4" />
              </button>
            </>
          ) : (
            <>
              <button
                onClick={() => onPinArtifact(bookmark)}
                className="p-1.5 rounded-lg bg-white/5 text-white/30 hover:text-[#39ff14] hover:bg-[#39ff14]/10 transition-all"
                title="Pin to Canvas"
              >
                <Plus className="w-4 h-4" />
              </button>
              <button
                onClick={startEdit}
                className="p-1.5 rounded-lg bg-white/5 text-white/30 hover:text-white transition-all"
                title="Edit Properties"
              >
                <PenSquare className="w-4 h-4" />
              </button>
              <button
                className="p-1.5 rounded-lg bg-white/5 text-white/30 hover:text-white transition-all"
                title="Share Artifact"
              >
                <Share2 className="w-4 h-4" />
              </button>
            </>
          )}
        </div>
      </div>

      {isEditing ? (
        <div className="flex flex-col gap-2 mb-4">
          <input
            type="text"
            value={editTitle}
            onChange={(e) => setEditTitle(e.target.value)}
            className="text-lg text-white font-medium bg-black/50 border border-white/10 rounded px-2 py-1 outline-none focus:border-[#39ff14]/50"
          />
          <div className="flex items-center gap-2">
            <span className="text-xs text-white/40 font-mono">
              Category:
            </span>
            <input
              type="text"
              value={editCategory}
              onChange={(e) => setEditCategory(e.target.value)}
              className="text-xs text-white/80 font-mono bg-black/50 border border-white/10 rounded px-2 py-1 outline-none focus:border-[#39ff14]/50 w-32"
            />
          </div>
        </div>
      ) : (
        <>
          <h3 className="text-lg text-white font-medium mb-1">
            {bookmark.title}
          </h3>
          <p className="text-xs text-white/40 mb-6 font-mono">
            Category: {bookmark.category} • Gen: {bookmark.date}
          </p>
        </>
      )}

      <div
        className={`mt-auto w-full h-28 rounded-lg bg-[#050505] inset-shadow-sm border border-white/5 flex items-center justify-center overflow-hidden relative ${isEditing ? "opacity-50 pointer-events-none" : ""}`}
      >
        {bookmark.type === "Table" && (
          <div className="w-full p-4 flex flex-col gap-1.5">
            <div className="h-2 w-full bg-[#39ff14]/60 rounded" />
            <div className="h-2 w-3/4 bg-white/10 rounded" />
            <div className="h-2 w-5/6 bg-white/10 rounded" />
            <div className="h-2 w-2/3 bg-white/10 rounded" />
          </div>
        )}
        {bookmark.type === "Graph" && (
          <svg
            className="w-full h-full opacity-40 text-[#39ff14] px-2 pt-2"
            viewBox="0 0 100 40"
            preserveAspectRatio="none"
          >
            <polyline
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              points="0,40 20,20 40,30 60,10 80,15 100,5"
            />
          </svg>
        )}
        {bookmark.type === "Code Snippet" && (
          <div className="w-full p-4 flex flex-col gap-2 opacity-60">
            <div className="h-1.5 w-1/3 bg-[#39ff14] rounded" />
            <div className="h-1.5 w-1/2 bg-white/40 rounded ml-4" />
            <div className="h-1.5 w-1/4 bg-white/40 rounded ml-4" />
          </div>
        )}
        {bookmark.type === "Data Card" && (
          <div className="flex gap-4 p-4 w-full">
            <div className="w-10 h-10 rounded-full border border-[#39ff14]/50 flex items-center justify-center">
              <div className="w-2 h-2 rounded-full bg-[#39ff14]" />
            </div>
            <div className="flex-1 flex flex-col justify-center gap-2">
              <div className="h-2 w-full bg-white/20 rounded" />
              <div className="h-2 w-1/2 bg-white/10 rounded" />
            </div>
          </div>
        )}
      </div>
    </div>
  );
});

export default BookmarkItem;
