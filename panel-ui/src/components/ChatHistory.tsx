import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
import { Renderer } from "@openuidev/react-lang";
import { combinedLibrary } from '../registry';
import { PanelMessage } from '../types';

interface ChatHistoryProps {
  messages: PanelMessage[];
  streamingMessageId: string | null;
}

function formatConfidence(value?: number) {
  if (typeof value !== "number") {
    return "n/a";
  }
  return `${Math.round(value * 100)}%`;
}

function StreamingMessageRenderer({ message, isStreamingActive }: { message: PanelMessage; isStreamingActive: boolean }) {
  const [streamedResponse, setStreamedResponse] = useState("");
  const [streamedText, setStreamedText] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);

  useEffect(() => {
    if (!isStreamingActive) {
      setStreamedResponse(message.openui_lang || "");
      setStreamedText(message.plain_text || "");
      setIsStreaming(false);
      return;
    }

    const fullResponse = message.openui_lang || "";
    const fullText = message.plain_text || "";
    let currentIndex = 0;
    setIsStreaming(true);
    setStreamedResponse("");
    setStreamedText("");

    const intervalId = setInterval(() => {
      currentIndex += 5; // Chunk size
      setStreamedResponse(fullResponse.slice(0, currentIndex));
      setStreamedText(fullText.slice(0, currentIndex));
      
      if (currentIndex >= Math.max(fullResponse.length, fullText.length)) {
        setIsStreaming(false);
        clearInterval(intervalId);
      }
    }, 20); // Faster speed for smooth blur diffusion

    return () => clearInterval(intervalId);
  }, [message, isStreamingActive]);

  return (
    <div className="flex flex-col gap-3">
      {message.openui_lang ? (
        <div className="w-full bg-[#0a0a0a]/80 rounded-xl overflow-hidden border border-white/5">
          <div className="px-4 py-2 bg-white/5 border-b border-white/5 flex items-center justify-between">
            <span className="text-[10px] uppercase text-white/40 tracking-widest">
              Render Surface
            </span>
            <span className="text-[10px] uppercase text-[#39ff14]/70 tracking-widest">
              {isStreaming ? "Streaming..." : "Structured"}
            </span>
          </div>
          <div className="p-4 bg-white">
            <Renderer
              response={streamedResponse}
              library={combinedLibrary}
              isStreaming={isStreaming}
            />
          </div>
        </div>
      ) : null}
      {streamedText && (
         <p className="text-sm leading-relaxed font-sans text-white/90">
            {streamedText}
         </p>
      )}
    </div>
  );
}

export default function ChatHistory({ messages, streamingMessageId }: ChatHistoryProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      setTimeout(() => {
        scrollRef.current!.scrollTop = scrollRef.current!.scrollHeight;
      }, 50);
    }
  }, [messages, streamingMessageId]);

  return (
    <div className="absolute top-0 bottom-[120px] left-1/2 -translate-x-1/2 w-full max-w-3xl p-4 flex flex-col justify-end pointer-events-none z-10">
      <div 
        ref={scrollRef}
        className="w-full overflow-y-auto flex flex-col gap-6 pointer-events-auto pb-4 scroll-smooth px-4"
        style={{ 
          maskImage: 'linear-gradient(to bottom, transparent, black 15%, black 100%)',
          WebkitMaskImage: '-webkit-linear-gradient(top, transparent, black 15%, black 100%)',
          scrollbarWidth: 'none',
          msOverflowStyle: 'none'
        }}
      >
        <AnimatePresence>
          {messages.map((msg) => {
            const isSystem = msg.role === 'assistant';
            
            return (
              <motion.div
                key={msg.id}
                initial={{ opacity: 0, y: 15, filter: 'blur(5px)' }}
                animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                className={`flex flex-col w-full ${!isSystem ? 'items-end' : 'items-start'}`}
              >
                <div className={`px-5 py-4 rounded-2xl max-w-[90%] backdrop-blur-md ${
                  !isSystem 
                    ? 'bg-white/5 border border-white/10 text-white/90 shadow-lg' 
                    : 'bg-[#39ff14]/5 border border-[#39ff14]/20 text-[#39ff14] neon-glow w-full'
                }`}>
                  {isSystem && (
                    <div className="flex items-center gap-2 mb-3 opacity-80 border-b border-[#39ff14]/10 pb-2">
                      <div className="w-1.5 h-1.5 rounded-full bg-[#39ff14] animate-pulse"></div>
                      <span className="text-[10px] uppercase tracking-widest font-bold">Xavier System</span>
                      
                      {/* Meta stats for system message */}
                      <div className="ml-auto flex gap-3 text-[9px] font-mono text-[#39ff14]/60">
                         <span>CONF: {formatConfidence(msg.metadata?.confidence)}</span>
                         <span>DOCS: {msg.metadata?.documents ?? 0}</span>
                         <span>EVID: {msg.metadata?.evidence ?? 0}</span>
                         <span>LATENCY: {msg.metadata?.timings?.total_ms ?? 0}ms</span>
                      </div>
                    </div>
                  )}
                  
                  {isSystem && (msg.metadata?.rules?.length || msg.metadata?.components?.length) ? (
                    <div className="mb-4 flex flex-wrap gap-2">
                       {msg.metadata.rules?.map(r => (
                          <span key={r} className="text-[9px] px-2 py-0.5 rounded-full border border-[#39ff14]/30 bg-[#39ff14]/10 text-[#39ff14]">
                            {r}
                          </span>
                       ))}
                       {msg.metadata.components?.map(c => (
                          <span key={c} className="text-[9px] px-2 py-0.5 rounded-full border border-white/30 bg-white/10 text-white">
                            {c}
                          </span>
                       ))}
                    </div>
                  ) : null}

                  {isSystem ? (
                    <StreamingMessageRenderer message={msg} isStreamingActive={msg.id === streamingMessageId} />
                  ) : (
                    <p className="text-sm leading-relaxed font-sans">
                      {msg.plain_text}
                    </p>
                  )}
                </div>
              </motion.div>
            );
          })}
        </AnimatePresence>
      </div>
      <style>{`
        .scroll-smooth::-webkit-scrollbar {
          display: none;
        }
      `}</style>
    </div>
  );
}
