
import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'motion/react';
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
      currentIndex += 5; // Chunk size
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
      <p className="text-sm leading-relaxed font-mono">
        {streamedText}
        {isStreaming && <span className="animate-pulse">_</span>}
      </p>
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
  }, [messages]);

  return (
    <div className="absolute top-0 bottom-[120px] left-1/2 -translate-x-1/2 w-full max-w-4xl p-4 flex flex-col justify-end pointer-events-none z-10">
      <div 
        ref={scrollRef}
        className="w-full overflow-y-auto flex flex-col gap-6 pointer-events-auto pb-4 scroll-smooth"
        style={{ 
          maskImage: 'linear-gradient(to bottom, transparent, black 15%, black 100%)',
          WebkitMaskImage: '-webkit-linear-gradient(top, transparent, black 15%, black 100%)',
          scrollbarWidth: 'none',
          msOverflowStyle: 'none'
        }}
      >
        <AnimatePresence>
          {messages.map((msg) => (
            <motion.div
              key={msg.id}
              initial={{ opacity: 0, y: 15, filter: 'blur(5px)' }}
              animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
              className={`flex flex-col w-full ${msg.role === 'user' ? 'items-end' : 'items-start'}`}
            >
              <div className={`px-5 py-3 rounded-2xl max-w-[90%] backdrop-blur-md ${
                msg.role === 'user' 
                  ? 'bg-white/5 border border-white/10 text-white/90 shadow-lg' 
                  : 'bg-[#39ff14]/5 border border-[#39ff14]/20 text-[#39ff14] neon-glow w-full'
              }`}>
                {msg.role === 'assistant' && (
                  <div className="flex items-center justify-between mb-3 border-b border-[#39ff14]/20 pb-2">
                    <div className="flex items-center gap-2 opacity-80">
                      <div className="w-1.5 h-1.5 rounded-full bg-[#39ff14] animate-pulse"></div>
                      <span className="text-[10px] uppercase tracking-widest font-bold">Xavier Agent</span>
                      <span className="text-[10px] uppercase tracking-widest opacity-60 ml-2">{new Date(msg.created_at).toLocaleTimeString()}</span>
                    </div>
                    {msg.metadata && (
                       <div className="flex gap-4 text-[10px] uppercase tracking-widest font-mono opacity-70">
                         <span>Conf: {formatConfidence(msg.metadata.confidence)}</span>
                         <span>Docs: {msg.metadata.documents ?? 0}</span>
                         <span>Lat: {msg.metadata.timings?.total_ms ?? 0}ms</span>
                       </div>
                    )}
                  </div>
                )}
                
                {msg.role === 'assistant' ? (
                   <StreamingMessageRenderer message={msg} isStreamingActive={msg.id === streamingMessageId} />
                ) : (
                   <p className="text-sm leading-relaxed font-sans">{msg.plain_text}</p>
                )}
              </div>
            </motion.div>
          ))}
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
