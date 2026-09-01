import { AnimatePresence, motion } from "motion/react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { PanelMessage } from "../types";
import LoadingSpinner from "./ui/LoadingSpinner";

interface ChatHistoryProps {
  messages: PanelMessage[];
  streamingMessageId: string | null;
  isLoading?: boolean;
}

function formatConfidence(value?: number) {
  if (typeof value !== "number") {
    return "n/a";
  }
  return `${Math.round(value * 100)}%`;
}

function StreamingMessageRenderer({
  message,
  isStreamingActive,
}: {
  message: PanelMessage;
  isStreamingActive: boolean;
}) {
  const [streamedText, setStreamedText] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);

  useEffect(() => {
    if (!isStreamingActive) {
      setStreamedText(message.plain_text || "");
      setIsStreaming(false);
      return;
    }

    const fullText = message.plain_text || "";
    let currentIndex = 0;
    setIsStreaming(true);
    setStreamedText("");

    const intervalId = setInterval(() => {
      currentIndex += 5;
      setStreamedText(fullText.slice(0, currentIndex));

      if (currentIndex >= fullText.length) {
        setIsStreaming(false);
        clearInterval(intervalId);
      }
    }, 20);

    return () => clearInterval(intervalId);
  }, [message, isStreamingActive]);

  return (
    <div className="flex flex-col gap-4 w-full">
      <p className="text-sm leading-relaxed font-mono whitespace-pre-wrap">
        {streamedText}
        {isStreaming && (
          <span className="inline-flex items-center gap-1.5 ml-1">
            <span className="animate-pulse opacity-70">▊</span>
            <LoadingSpinner size={12} className="text-[#39ff14]" />
          </span>
        )}
      </p>
    </div>
  );
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted message row into ChatMessageItem and wrapped in React.memo()
 * 🎯 Why: When appending new messages to the chat (or streaming text), ChatHistory
 *         was re-rendering the entire list. This caused O(N) string parsing because
 *         JSON.parse(msg.metadata) ran for every message on every render.
 * 📊 Impact: Eliminates O(N) JSON.parse overhead and DOM reconciliation for old messages
 *         when new messages stream in. Appending/streaming is now O(1) in the list context.
 */
const ChatMessageItem = memo(function ChatMessageItem({
  msg,
  isStreamingActive,
}: {
  msg: PanelMessage;
  isStreamingActive: boolean;
}) {
  const parsedMetadata = useMemo(() => {
    if (!msg.metadata) return null;
    if (typeof msg.metadata === "string") {
      try {
        return JSON.parse(msg.metadata);
      } catch {
        return null;
      }
    }
    return msg.metadata;
  }, [msg.metadata]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 12, filter: "blur(4px)" }}
      animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
      transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
      className={`flex w-full gap-3 ${msg.role === "user" ? "flex-row-reverse" : "flex-row"}`}
    >
      {/* Avatar */}
      {msg.role === "assistant" ? (
        <div className="flex-shrink-0 w-7 h-7 rounded-full bg-[#39ff14]/10 border border-[#39ff14]/20 flex items-center justify-center mt-1">
          <div className="w-1.5 h-1.5 rounded-full bg-[#39ff14] msg-active-dot" />
        </div>
      ) : (
        <div className="flex-shrink-0 w-7 h-7 rounded-full bg-white/10 border border-white/[0.12] flex items-center justify-center mt-1 text-[9px] font-bold text-white/50 uppercase">
          U
        </div>
      )}

      {/* Bubble */}
      <div
        className={`flex flex-col max-w-[85%] ${msg.role === "user" ? "items-end" : "items-start"}`}
      >
        <div
          className={`px-4 py-3 rounded-2xl backdrop-blur-md ${
            msg.role === "user"
              ? "bg-white/[0.05] border border-white/[0.07] text-white/80 shadow-sm rounded-tr-sm"
              : "bg-[#39ff14]/[0.025] border border-[#39ff14]/[0.09] text-[#39ff14] neon-glow-subtle w-full rounded-tl-sm"
          }`}
        >
          {msg.role === "assistant" && (
            <div className="flex items-center justify-between mb-2.5 border-b border-[#39ff14]/[0.08] pb-2">
              <div className="flex items-center gap-2 opacity-50">
                <span className="text-[9px] uppercase tracking-[0.15em] font-bold">
                  Xavier Agent
                </span>
                <span className="text-[9px] opacity-60">
                  {new Date(msg.created_at).toLocaleTimeString()}
                </span>
              </div>
              {parsedMetadata && (
                <div className="flex gap-3 text-[9px] uppercase tracking-widest font-mono opacity-40">
                  <span>
                    Conf: {formatConfidence(parsedMetadata.confidence)}
                  </span>
                  <span>Docs: {parsedMetadata.documents ?? 0}</span>
                  <span>Lat: {parsedMetadata.timings?.total_ms ?? 0}ms</span>
                </div>
              )}
            </div>
          )}

          {msg.role === "assistant" &&
            parsedMetadata?.provider === "memory-fallback" && (
              <div
                role="note"
                aria-label="Respondiendo desde memoria (LLM no disponible)"
                className="flex items-center gap-2 px-3 py-1.5 mb-3 text-xs rounded-lg bg-amber-500/10 border border-amber-500/20 text-amber-400 select-none"
              >
                <span aria-hidden="true">💾</span>
                <span className="font-medium">
                  Respondiendo desde memoria (LLM no disponible)
                </span>
              </div>
            )}

          {msg.role === "assistant" ? (
            <StreamingMessageRenderer
              message={msg}
              isStreamingActive={isStreamingActive}
            />
          ) : (
            <p className="text-sm leading-relaxed font-sans">
              {msg.plain_text}
            </p>
          )}
        </div>

        {/* Timestamp below bubble for user messages */}
        {msg.role === "user" && (
          <span className="text-[9px] text-white/20 mt-1 mr-1">
            {new Date(msg.created_at).toLocaleTimeString()}
          </span>
        )}
      </div>
    </motion.div>
  );
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped ChatHistory in React.memo()
 * 🎯 Why: ChatHistory is a large complex component re-rendering dynamic lists of ChatMessageItem. When the parent App.tsx updates due to typing inputs or hash changes, ChatHistory needlessly re-rendered.
 * 📊 Impact: O(1) rendering for the ChatHistory container when un-related parent state updates, preventing expensive DOM reconciliation of the message list.
 */
export default memo(function ChatHistory({
  messages,
  streamingMessageId,
  isLoading = false,
}: ChatHistoryProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      setTimeout(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      }, 50);
    }
  }, []);

  return (
    <div className="absolute top-0 bottom-[120px] left-1/2 -translate-x-1/2 w-full max-w-4xl p-4 flex flex-col justify-end pointer-events-none z-10">
      <div
        ref={scrollRef}
        className="w-full overflow-y-auto flex flex-col gap-5 pointer-events-auto pb-4 scroll-smooth"
        style={{
          maskImage:
            "linear-gradient(to bottom, transparent, black 15%, black 100%)",
          WebkitMaskImage:
            "-webkit-linear-gradient(top, transparent, black 15%, black 100%)",
          scrollbarWidth: "none",
          msOverflowStyle: "none",
        }}
      >
        <AnimatePresence>
          {messages.map((msg) => (
            <ChatMessageItem
              key={msg.id}
              msg={msg}
              isStreamingActive={msg.id === streamingMessageId}
            />
          ))}
          {isLoading && !streamingMessageId && (
            <motion.div
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -12 }}
              className="flex items-center gap-3 p-3 rounded-xl bg-[#39ff14]/[0.025] border border-[#39ff14]/[0.09] text-[#39ff14] text-xs font-mono"
              data-testid="chat-loading-indicator"
            >
              <LoadingSpinner size={16} className="text-[#39ff14]" />
              <span>Xavier is thinking...</span>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <style>{`
        .scroll-smooth::-webkit-scrollbar {
          display: none;
        }
      `}</style>
    </div>
  );
});
