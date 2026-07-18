import { invoke } from "@tauri-apps/api/core";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAuthStore } from "./auth/AuthProvider";
import { BackupCodesPage } from "./auth/BackupCodesPage";
import { LoginPage } from "./auth/LoginPage";
import { MasterKeyPage } from "./auth/MasterKeyPage";
import { RecoveryPage } from "./auth/RecoveryPage";
import { RegisterPage } from "./auth/RegisterPage";
import { TwoFactorSetup } from "./auth/TwoFactorSetup";
import ChatHistory from "./components/ChatHistory";
import ConfigModal from "./components/ConfigModal";
import DraggableWidget from "./components/DraggableWidget";
import InputArea from "./components/InputArea";
import { OnboardingFlow } from "./components/Onboarding/OnboardingFlow";
import ParticleBackground from "./components/ParticleBackground";
import TopStatusBar from "./components/TopStatusBar";
import { initialBookmarks } from "./data";
import { EMPTY_ROADMAP_GRAPH, normalizeGraphData } from "./utils/roadmapGraph";

import type {
  BackendGraphData,
  Bookmark,
  BookmarkArtifact,
  CanvasWidget,
  GraphData,
  PanelChatResponse,
  PanelMessage,
  ThreadDetail,
  ThreadSummary,
  Widget,
} from "./types";

type RoadmapGraphMeta = {
  id: string;
  name: string;
  created_at: string;
};

const DEFAULT_GRAPH_META: RoadmapGraphMeta = {
  id: "default",
  name: "Workspace roadmap",
  created_at: new Date(0).toISOString(),
};

const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

export default function App() {
  const { token, isAuthenticated } = useAuthStore();
  const [hash, setHash] = useState(window.location.hash);
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [messages, setMessages] = useState<PanelMessage[]>([]);
  const [health, setHealth] = useState("checking");
  const [_isLoading, setIsLoading] = useState(false);
  const [_error, setError] = useState<string | null>(null);
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(
    null,
  );

  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [widgets, setWidgets] = useState<CanvasWidget[]>([]);
  const [graphData, setGraphData] = useState<GraphData>(EMPTY_ROADMAP_GRAPH);
  const [graphMeta, setGraphMeta] =
    useState<RoadmapGraphMeta>(DEFAULT_GRAPH_META);
  const graphMetaRef = useRef(graphMeta);
  const graphSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [hasConfig, setHasConfig] = useState(true);

  const [isConfigOpen, setIsConfigOpen] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(
    () =>
      typeof window !== "undefined" &&
      !localStorage.getItem("xavier_onboarding_completed"),
  );

  const api = useCallback(
    async <T,>(path: string, options?: RequestInit): Promise<T> => {
      const response = await fetch(getApiUrl(path), {
        ...options,
        headers: {
          "Content-Type": "application/json",
          "X-Xavier-Token": token ?? "",
          ...(options?.headers ?? {}),
        },
      });

      if (!response.ok) {
        throw new Error(await response.text());
      }
      return (await response.json()) as T;
    },
    [token],
  );

  useEffect(() => {
    graphMetaRef.current = graphMeta;
  }, [graphMeta]);

  useEffect(() => {
    return () => {
      if (graphSaveTimerRef.current) {
        clearTimeout(graphSaveTimerRef.current);
      }
    };
  }, []);

  const loadPanelData = useCallback(
    async (currentToken: string) => {
      try {
        const [bookmarksData, _widgetsData, graphDataResult] =
          await Promise.all([
            api<Bookmark[]>("/panel/api/bookmarks", {
              headers: { "X-Xavier-Token": currentToken },
            }),
            api<Widget[]>("/panel/api/widgets", {
              headers: { "X-Xavier-Token": currentToken },
            }).catch(() => []),
            api<BackendGraphData>("/panel/api/graph", {
              headers: { "X-Xavier-Token": currentToken },
            }).catch(() => null as unknown as BackendGraphData),
          ]);
        setBookmarks(bookmarksData);
        if (graphDataResult) {
          const payload = graphDataResult.data as GraphData | undefined;
          const nodes = Array.isArray(payload?.nodes) ? payload.nodes : [];
          const links = Array.isArray(payload?.links) ? payload.links : [];
          setGraphData(normalizeGraphData({ nodes, links }));
          const meta: RoadmapGraphMeta = {
            id: graphDataResult.id || DEFAULT_GRAPH_META.id,
            name: graphDataResult.name || DEFAULT_GRAPH_META.name,
            created_at:
              graphDataResult.created_at || new Date().toISOString(),
          };
          setGraphMeta(meta);
          graphMetaRef.current = meta;
        } else {
          setGraphData(EMPTY_ROADMAP_GRAPH);
          setGraphMeta(DEFAULT_GRAPH_META);
          graphMetaRef.current = DEFAULT_GRAPH_META;
        }
      } catch (e) {
        console.warn("Failed to load panel state:", e);
      }
    },
    [api],
  );

  const _activeThread = useMemo(
    () => threads.find((item) => item.id === selectedThreadId) ?? null,
    [selectedThreadId, threads],
  );

  useEffect(() => {
    fetch(getApiUrl("/health"))
      .then((response) => response.json())
      .then((data) => setHealth(data.status ?? "unknown"))
      .catch(() => setHealth("offline"));
  }, []);

  useEffect(() => {
    const checkNativeConfig = async () => {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          const configState = (await invoke("get_current_config_state")) as {
            has_openai: boolean;
            has_gemini: boolean;
          };
          setHasConfig(configState.has_openai || configState.has_gemini);
        } catch (e) {
          console.warn("Could not retrieve Xavier config", e);
        }
      }
    };
    void checkNativeConfig();
  }, []);

  useEffect(() => {
    const handleHashChange = () => setHash(window.location.hash);
    window.addEventListener("hashchange", handleHashChange);
    return () => window.removeEventListener("hashchange", handleHashChange);
  }, []);

  const openThread = useCallback(
    async (threadId: string, currentToken = token) => {
      if (!currentToken) return;
      try {
        setError(null);
        const response = await fetch(
          getApiUrl(`/panel/api/threads/${threadId}`),
          {
            headers: { "X-Xavier-Token": currentToken },
          },
        );
        if (!response.ok) throw new Error("Failed to load thread");
        const detail = (await response.json()) as ThreadDetail;
        setSelectedThreadId(threadId);
        setMessages(detail.messages);
      } catch (cause) {
        setError(
          cause instanceof Error ? cause.message : "Failed to open thread",
        );
      }
    },
    [token],
  );

  const loadThreads = useCallback(
    async (currentToken: string) => {
      try {
        setError(null);
        const response = await fetch(getApiUrl("/panel/api/threads"), {
          headers: { "X-Xavier-Token": currentToken },
        });
        if (!response.ok) throw new Error("Token rejected by Xavier");
        const data = (await response.json()) as ThreadSummary[];
        setThreads(data);
        if (!selectedThreadId && data[0]) {
          void openThread(data[0].id, currentToken);
        }
      } catch (cause) {
        setError(
          cause instanceof Error ? cause.message : "Failed to load threads",
        );
      }
    },
    [openThread, selectedThreadId],
  );

  useEffect(() => {
    if (!token) return;
    void loadThreads(token);
    void loadPanelData(token);
  }, [token, loadThreads, loadPanelData]);

  async function _createThread() {
    try {
      const thread = await api<ThreadSummary>("/panel/api/threads", {
        method: "POST",
        body: JSON.stringify({ title: "New Session" }),
      });
      setThreads((current) => [thread, ...current]);
      setSelectedThreadId(thread.id);
      setMessages([]);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Failed to create thread",
      );
    }
  }

  async function sendMessage(draft: string) {
    if (!draft.trim()) return;

    const tempId = Date.now().toString();
    const newUserMsg: PanelMessage = {
      id: tempId,
      role: "user",
      plain_text: draft,
      created_at: new Date().toISOString(),
    };

    if (!hasConfig) {
      setMessages((prev) => [
        ...prev,
        newUserMsg,
        {
          id: `${tempId}_sys`,
          role: "assistant",
          plain_text:
            "⚠️ Sistema no configurado: No se detectaron proveedores de IA. Por favor, abre los ajustes y configura tu API Key de OpenAI o Gemini.",
          created_at: new Date().toISOString(),
        },
      ]);
      return;
    }

    // Optimistic UI updates
    setMessages((prev) => [...prev, newUserMsg]);

    try {
      setIsLoading(true);
      setError(null);

      // Create thread if none exists
      let targetThreadId = selectedThreadId;
      if (!targetThreadId) {
        const thread = await api<ThreadSummary>("/panel/api/threads", {
          method: "POST",
          body: JSON.stringify({ title: draft.slice(0, 30) }),
        });
        setThreads((current) => [thread, ...current]);
        targetThreadId = thread.id;
        setSelectedThreadId(thread.id);
      }

      const payload = await api<PanelChatResponse>("/panel/api/chat", {
        method: "POST",
        body: JSON.stringify({
          thread_id: targetThreadId,
          message: draft,
        }),
      });

      setSelectedThreadId(payload.thread.id);
      setMessages(payload.messages);

      const lastMessage = payload.messages[payload.messages.length - 1];
      if (lastMessage?.role === "assistant") {
        setStreamingMessageId(lastMessage.id);
      }

      setThreads((current) => {
        const next = [
          payload.thread,
          ...current.filter((item) => item.id !== payload.thread.id),
        ];
        return next;
      });
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : "Failed to send message",
      );
    } finally {
      setIsLoading(false);
    }
  }

  const handleSystemMessage = (text: string) => {
    setMessages((prev) => [
      ...prev,
      {
        id: `${Date.now().toString()}_sys`,
        role: "assistant",
        plain_text: text,
        created_at: new Date().toISOString(),
      },
    ]);
  };

  const handleUpdateBookmark = (_updated: BookmarkArtifact) => {
    // For demo purposes, sync local state
  };

  const handleUpdateGraphData = useCallback(
    (data: GraphData) => {
      const normalized = normalizeGraphData(data);
      setGraphData(normalized);

      if (graphSaveTimerRef.current) {
        clearTimeout(graphSaveTimerRef.current);
      }

      graphSaveTimerRef.current = setTimeout(async () => {
        const meta = graphMetaRef.current;
        try {
          await api("/panel/api/graph", {
            method: "POST",
            body: JSON.stringify({
              id: meta.id,
              name: meta.name,
              data: normalized,
              created_at: meta.created_at || new Date().toISOString(),
            }),
          });
        } catch (e) {
          console.warn("Failed to save roadmap graph:", e);
        }
      }, 400);
    },
    [api],
  );

  const handlePinArtifact = (artifact: BookmarkArtifact) => {
    const newWidget: CanvasWidget = {
      id: `w_${Date.now()}`,
      artifact,
      position: { x: 50 + widgets.length * 30, y: 50 + widgets.length * 30 },
    };
    setWidgets((prev) => [...prev, newWidget]);
    setIsConfigOpen(false);
  };

  const handleRemoveWidget = (id: string) => {
    setWidgets((prev) => prev.filter((w) => w.id !== id));
  };

  const handleUpdateWidgetPosition = (id: string, x: number, y: number) => {
    setWidgets((prev) =>
      prev.map((w) => (w.id === id ? { ...w, position: { x, y } } : w)),
    );
  };

  if (health === "offline") {
    return (
      <div className="w-full h-screen bg-black flex items-center justify-center text-[#39ff14] font-mono">
        <div className="text-center">
          <h1 className="text-2xl mb-4 uppercase tracking-widest border-b border-[#39ff14]/30 pb-2">
            Xavier Offline
          </h1>
          <p className="opacity-70 text-sm">
            Cannot reach local backend at 127.0.0.1:8006.
          </p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="mt-8 px-6 py-2 border border-[#39ff14] hover:bg-[#39ff14]/10 transition-colors uppercase text-xs tracking-widest rounded-lg"
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  if (!isAuthenticated) {
    if (hash === "#/register") return <RegisterPage />;
    if (hash === "#/recovery") return <RecoveryPage />;
    return <LoginPage />;
  }

  // Auth-only routes
  if (hash === "#/2fa/setup") return <TwoFactorSetup />;
  if (hash === "#/2fa/backup") return <BackupCodesPage />;
  if (hash === "#/master-key") return <MasterKeyPage />;

  if (showOnboarding) {
    return <OnboardingFlow onComplete={() => setShowOnboarding(false)} />;
  }

  return (
    <div className="relative w-full h-screen font-sans bg-[#050505] flex flex-col overflow-hidden text-white">
      <ParticleBackground />
      <TopStatusBar isModalOpen={isConfigOpen} />

      {/* Pinned Widgets Layer */}
      <div className="absolute inset-0 pointer-events-none z-20">
        <AnimatePresence>
          {widgets.map((widget) => (
            <DraggableWidget
              key={widget.id}
              widget={widget}
              onRemove={handleRemoveWidget}
              onUpdatePosition={handleUpdateWidgetPosition}
            />
          ))}
        </AnimatePresence>
      </div>

      {/* Main Content Area */}
      <main className="flex-1 w-full flex items-center justify-center relative pb-24 z-10">
        <AnimatePresence mode="wait">
          {isConfigOpen && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
            >
              <ConfigModal
                key="modal"
                onClose={() => setIsConfigOpen(false)}
                graphData={graphData}
                onUpdateGraphData={handleUpdateGraphData}
                bookmarks={
                  bookmarks.length > 0
                    ? (bookmarks as unknown as BookmarkArtifact[])
                    : initialBookmarks
                }
                onPinArtifact={handlePinArtifact}
                onUpdateBookmark={handleUpdateBookmark}
                token={token || undefined}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </main>

      <ChatHistory
        messages={messages}
        streamingMessageId={streamingMessageId}
      />

      <AnimatePresence>
        {!isConfigOpen && (
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
          >
            <InputArea
              onSendMessage={sendMessage}
              onOpenConfig={() => setIsConfigOpen(true)}
              onSystemMessage={handleSystemMessage}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
