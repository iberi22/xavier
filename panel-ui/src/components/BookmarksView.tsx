import { Folder } from "lucide-react";
import { motion } from "motion/react";
import React from "react";
import { useMemo, useState } from "react";
import type { BookmarkArtifact } from "../types";
import BookmarkItem from "./BookmarkItem";

interface BookmarksViewProps {
  key?: React.Key;
  bookmarks: BookmarkArtifact[];
  onPinArtifact: (artifact: BookmarkArtifact) => void;
  onUpdateBookmark: (updated: BookmarkArtifact) => void;
}

export default React.memo(function BookmarksView({
  bookmarks,
  onPinArtifact,
  onUpdateBookmark,
}: BookmarksViewProps) {
  const [activeCategory, setActiveCategory] = useState<string>("All");

  const categories = useMemo(() => {
    const cats = new Set(bookmarks.map((b) => b.category || "General"));
    return ["All", ...Array.from(cats)];
  }, [bookmarks]);

  /**
   * ⚡ Bolt Performance Optimization
   *
   * 💡 What: Wrapped filteredBookmarks in useMemo() and BookmarksView in React.memo()
   * 🎯 Why: filteredBookmarks was re-calculated on every render of BookmarksView, doing an O(N) filter.
   *         Additionally, BookmarksView itself was unmemoized, leading to unnecessary re-renders when parent state changed.
   * 📊 Impact: O(1) filtering on re-renders, preventing expensive filtering and DOM reconciliation on every un-related state change.
   */
  const filteredBookmarks = useMemo(() => {
    return activeCategory === "All"
      ? bookmarks
      : bookmarks.filter((b) => b.category === activeCategory);
  }, [bookmarks, activeCategory]);

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="w-full h-full p-10 flex flex-col overflow-hidden"
    >
      <div className="mb-8 flex items-end justify-between shrink-0">
        <div>
          <h2 className="text-3xl font-light text-white tracking-tight">
            Saved Artifacts
          </h2>
          <p className="text-sm text-white/40 mt-1">
            Generations and visual references bookmarked during conversations.
          </p>
        </div>
      </div>

      <div className="flex gap-2 mb-6 overflow-x-auto shrink-0 pb-2 scrollbar-none">
        {categories.map((cat) => (
          <button
            key={cat}
            type="button"
            onClick={() => setActiveCategory(cat)}
            aria-pressed={activeCategory === cat}
            className={`flex items-center gap-2 px-4 py-2 rounded-full text-xs font-medium transition-all whitespace-nowrap focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 focus-visible:outline-none
              ${activeCategory === cat ? "bg-[#39ff14] text-[#050505] shadow-[0_0_15px_rgba(57,255,20,0.3)]" : "bg-white/5 text-white/60 hover:bg-white/10 hover:text-white"}`}
          >
            {cat !== "All" && <Folder className="w-3.5 h-3.5" aria-hidden="true" />}
            {cat}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto pr-2 grid grid-cols-2 gap-6 pb-10">
        {filteredBookmarks.map((b) => (
          <BookmarkItem
            key={b.id}
            bookmark={b}
            onPinArtifact={onPinArtifact}
            onUpdateBookmark={onUpdateBookmark}
          />
        ))}
      </div>
    </motion.div>
  );
});
