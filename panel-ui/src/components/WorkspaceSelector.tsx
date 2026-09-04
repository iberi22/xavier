import { Check, ChevronDown, Folder, Plus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useRef, useState } from "react";

const STORAGE_KEY_ACTIVE = "xavier_active_workspace";
const STORAGE_KEY_LIST = "xavier_workspaces";
const DEFAULT_WORKSPACES = ["default", "swal", "personal", "work"];

export function getActiveWorkspaceId(): string {
  if (typeof localStorage === "undefined") return "default";
  return localStorage.getItem(STORAGE_KEY_ACTIVE) || "default";
}

export function getWorkspaceList(): string[] {
  if (typeof localStorage === "undefined") return DEFAULT_WORKSPACES;
  try {
    const raw = localStorage.getItem(STORAGE_KEY_LIST);
    if (!raw) return DEFAULT_WORKSPACES;
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.length > 0) return parsed;
  } catch (e) {
    console.debug("Failed to parse xavier_workspaces from localStorage", e);
  }
  return DEFAULT_WORKSPACES;
}

export default function WorkspaceSelector() {
  const [activeWorkspace, setActiveWorkspace] = useState<string>(
    getActiveWorkspaceId(),
  );
  const [workspaces, setWorkspaces] = useState<string[]>(getWorkspaceList());
  const [isOpen, setIsOpen] = useState(false);
  const [showAddInput, setShowAddInput] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState("");
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
        setShowAddInput(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const switchWorkspace = (wsId: string) => {
    const sanitized = wsId.trim();
    if (!sanitized) return;

    setActiveWorkspace(sanitized);
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY_ACTIVE, sanitized);
    }

    window.dispatchEvent(
      new CustomEvent("xavier:workspace-changed", {
        detail: { workspaceId: sanitized },
      }),
    );

    setIsOpen(false);
    setShowAddInput(false);
  };

  const handleCreateWorkspace = (e: React.FormEvent) => {
    e.preventDefault();
    const name = newWorkspaceName.trim().toLowerCase().replace(/\s+/g, "-");
    if (!name) return;

    let updatedList = workspaces;
    if (!workspaces.includes(name)) {
      updatedList = [...workspaces, name];
      setWorkspaces(updatedList);
      if (typeof localStorage !== "undefined") {
        localStorage.setItem(STORAGE_KEY_LIST, JSON.stringify(updatedList));
      }
    }

    setNewWorkspaceName("");
    switchWorkspace(name);
  };

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-2 h-7 text-white/80 shrink-0 hover:bg-white/5 hover:border-white/20 transition-all text-xs font-mono group"
        title={`Active Workspace: ${activeWorkspace}`}
        aria-label={`Select workspace. Active: ${activeWorkspace}`}
      >
        <Folder className="w-3 h-3 text-emerald-400 group-hover:scale-110 transition-transform" />
        <span className="font-semibold text-[10px] uppercase tracking-wider text-white/90">
          {activeWorkspace}
        </span>
        <ChevronDown
          className={`w-3 h-3 text-white/40 transition-transform duration-200 ${isOpen ? "rotate-180" : ""}`}
        />
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0, y: 8, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.95 }}
            transition={{ duration: 0.15 }}
            className="absolute left-0 top-full mt-2 w-52 bg-[#0a0a0a]/95 backdrop-blur-xl border border-white/10 rounded-xl p-2 shadow-2xl z-[70] flex flex-col gap-1 text-xs"
          >
            <div className="flex items-center justify-between px-2 py-1 border-b border-white/5 mb-1">
              <span className="text-[9px] uppercase tracking-widest text-white/40 font-mono font-semibold">
                Workspaces / Vaults
              </span>
            </div>

            <div className="max-h-48 overflow-y-auto space-y-0.5 scrollbar-thin">
              {workspaces.map((ws) => {
                const isSelected = ws === activeWorkspace;
                return (
                  <button
                    key={ws}
                    type="button"
                    onClick={() => switchWorkspace(ws)}
                    className={`w-full flex items-center justify-between px-2.5 py-1.5 rounded-lg text-left transition-colors font-mono ${
                      isSelected
                        ? "bg-emerald-500/15 text-emerald-300 font-semibold border border-emerald-500/30"
                        : "text-white/70 hover:bg-white/5 hover:text-white"
                    }`}
                  >
                    <span className="truncate">{ws}</span>
                    {isSelected && (
                      <Check className="w-3 h-3 text-emerald-400 shrink-0" />
                    )}
                  </button>
                );
              })}
            </div>

            <div className="border-t border-white/5 pt-1.5 mt-1">
              {!showAddInput ? (
                <button
                  type="button"
                  onClick={() => setShowAddInput(true)}
                  className="w-full flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-white/50 hover:text-emerald-300 hover:bg-emerald-500/10 transition-colors font-mono text-[11px]"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span>New Workspace</span>
                </button>
              ) : (
                <form onSubmit={handleCreateWorkspace} className="flex gap-1">
                  <input
                    type="text"
                    value={newWorkspaceName}
                    onChange={(e) => setNewWorkspaceName(e.target.value)}
                    placeholder="workspace-name"
                    autoFocus
                    className="flex-1 bg-black/60 border border-white/10 rounded-lg px-2 py-1 text-[11px] text-white font-mono placeholder:text-white/30 focus:outline-none focus:border-emerald-400"
                  />
                  <button
                    type="submit"
                    disabled={!newWorkspaceName.trim()}
                    className="px-2 py-1 bg-emerald-500 text-black font-semibold rounded-lg text-[10px] uppercase disabled:opacity-40 hover:bg-emerald-400 transition-colors"
                  >
                    Add
                  </button>
                </form>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
