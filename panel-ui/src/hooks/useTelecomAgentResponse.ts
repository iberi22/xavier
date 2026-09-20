import { useCallback, useEffect, useRef, useState } from "react";
import { getApiUrl } from "../api/client";
import { getApiTokenSync } from "./useApiToken";

export interface TelecomAgentChunk {
  delta?: string;
  thought?: string;
  type?: "token" | "thought" | "start" | "end" | "error";
  responseId?: string;
  agentId?: string;
  roomId?: string;
  tokenUsage?: number;
  error?: string;
}

export interface UseTelecomAgentResponseOptions {
  roomId?: string;
  agentId?: string;
  apiEndpoint?: string;
  smoothTyping?: boolean;
  typingIntervalMs?: number;
  onChunk?: (chunk: TelecomAgentChunk) => void;
  onFinish?: (fullText: string, thoughtText: string) => void;
  onError?: (error: Error) => void;
}

export interface UseTelecomAgentResponseReturn {
  streamingText: string;
  thoughtText: string;
  isThinking: boolean;
  isStreaming: boolean;
  isLoading: boolean;
  tokensReceived: number;
  responseId: string | null;
  error: Error | null;
  sendQuery: (
    prompt: string,
    overrideOptions?: { roomId?: string; agentId?: string }
  ) => Promise<void>;
  abortStream: () => void;
  resetStream: () => void;
}

/**
 * Custom hook for streaming token-by-token automated agent replies over the telecom P2P channel.
 * Manages real-time token buffering, agent thought indicator states, and AbortController lifecycle.
 */
export function useTelecomAgentResponse(
  options: UseTelecomAgentResponseOptions = {}
): UseTelecomAgentResponseReturn {
  const {
    roomId: defaultRoomId = "room-general",
    agentId: defaultAgentId = "xavier-agent",
    apiEndpoint = "/telecom/agent/stream",
    smoothTyping = true,
    typingIntervalMs = 15,
    onChunk,
    onFinish,
    onError,
  } = options;

  const [streamingText, setStreamingText] = useState<string>("");
  const [thoughtText, setThoughtText] = useState<string>("");
  const [isThinking, setIsThinking] = useState<boolean>(false);
  const [isStreaming, setIsStreaming] = useState<boolean>(false);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [tokensReceived, setTokensReceived] = useState<number>(0);
  const [responseId, setResponseId] = useState<string | null>(null);
  const [error, setError] = useState<Error | null>(null);

  const abortControllerRef = useRef<AbortController | null>(null);
  const bufferRef = useRef<string[]>([]);
  const thoughtBufferRef = useRef<string[]>([]);
  const drainTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const isMountedRef = useRef<boolean>(true);
  const activePromptRef = useRef<string>("");

  // Store options in ref to avoid re-triggering callbacks on option updates
  const callbacksRef = useRef({ onChunk, onFinish, onError });
  useEffect(() => {
    callbacksRef.current = { onChunk, onFinish, onError };
  }, [onChunk, onFinish, onError]);

  const clearDrainTimer = useCallback(() => {
    if (drainTimerRef.current !== null) {
      clearInterval(drainTimerRef.current);
      drainTimerRef.current = null;
    }
  }, []);

  const resetStream = useCallback(() => {
    clearDrainTimer();
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    bufferRef.current = [];
    thoughtBufferRef.current = [];
    setStreamingText("");
    setThoughtText("");
    setIsThinking(false);
    setIsStreaming(false);
    setIsLoading(false);
    setTokensReceived(0);
    setResponseId(null);
    setError(null);
  }, [clearDrainTimer]);

  const abortStream = useCallback(() => {
    clearDrainTimer();
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }

    // Immediately flush remaining buffer into streamingText on manual abort
    if (bufferRef.current.length > 0) {
      const remaining = bufferRef.current.join("");
      bufferRef.current = [];
      setStreamingText((prev) => prev + remaining);
    }
    if (thoughtBufferRef.current.length > 0) {
      const remainingThoughts = thoughtBufferRef.current.join("");
      thoughtBufferRef.current = [];
      setThoughtText((prev) => prev + remainingThoughts);
    }

    if (isMountedRef.current) {
      setIsStreaming(false);
      setIsLoading(false);
      setIsThinking(false);
    }
  }, [clearDrainTimer]);

  // Buffer drain loop for smooth typing effect
  const startDrainLoop = useCallback(() => {
    if (drainTimerRef.current !== null) return;

    drainTimerRef.current = setInterval(() => {
      if (!isMountedRef.current) {
        clearDrainTimer();
        return;
      }

      let textUpdated = false;
      let thoughtUpdated = false;

      if (bufferRef.current.length > 0) {
        const nextChar = bufferRef.current.shift();
        if (nextChar !== undefined) {
          setStreamingText((prev) => prev + nextChar);
          textUpdated = true;
        }
      }

      if (thoughtBufferRef.current.length > 0) {
        const nextThoughtChar = thoughtBufferRef.current.shift();
        if (nextThoughtChar !== undefined) {
          setThoughtText((prev) => prev + nextThoughtChar);
          thoughtUpdated = true;
        }
      }

      // If no text/thought remains in buffer and stream isn't actively reading new chunks, stop drain timer
      if (
        !textUpdated &&
        !thoughtUpdated &&
        bufferRef.current.length === 0 &&
        thoughtBufferRef.current.length === 0
      ) {
        clearDrainTimer();
      }
    }, typingIntervalMs);
  }, [clearDrainTimer, typingIntervalMs]);

  const processChunkData = useCallback(
    (chunkData: TelecomAgentChunk) => {
      if (!isMountedRef.current) return;

      if (chunkData.responseId && !responseId) {
        setResponseId(chunkData.responseId);
      }

      if (chunkData.type === "error" || chunkData.error) {
        const errMsg = chunkData.error || "Telecom agent streaming error";
        const err = new Error(errMsg);
        setError(err);
        callbacksRef.current.onError?.(err);
        setIsStreaming(false);
        setIsLoading(false);
        setIsThinking(false);
        return;
      }

      // Handle thought indicators
      if (chunkData.thought || chunkData.type === "thought") {
        const thoughtContent = chunkData.thought || chunkData.delta || "";
        if (thoughtContent) {
          setIsThinking(true);
          if (smoothTyping) {
            thoughtBufferRef.current.push(...thoughtContent.split(""));
            startDrainLoop();
          } else {
            setThoughtText((prev) => prev + thoughtContent);
          }
        }
      }

      // Handle token deltas
      if (chunkData.delta && chunkData.type !== "thought") {
        const delta = chunkData.delta;

        // Parse embedded <thought>...</thought> tags if present in plain token streams
        if (delta.includes("<thought>")) {
          setIsThinking(true);
        }

        if (smoothTyping) {
          bufferRef.current.push(...delta.split(""));
          startDrainLoop();
        } else {
          setStreamingText((prev) => prev + delta);
        }

        if (delta.includes("</thought>")) {
          setIsThinking(false);
        }

        setTokensReceived((prev) => prev + 1);
      }

      callbacksRef.current.onChunk?.(chunkData);
    },
    [responseId, smoothTyping, startDrainLoop]
  );

  const sendQuery = useCallback(
    async (
      prompt: string,
      overrideOptions?: { roomId?: string; agentId?: string }
    ) => {
      if (!prompt.trim()) return;

      // Abort any existing stream
      abortStream();

      // Reset state for new query stream
      bufferRef.current = [];
      thoughtBufferRef.current = [];
      setStreamingText("");
      setThoughtText("");
      setIsThinking(false);
      setIsStreaming(true);
      setIsLoading(true);
      setTokensReceived(0);
      setResponseId(null);
      setError(null);

      activePromptRef.current = prompt;

      const controller = new AbortController();
      abortControllerRef.current = controller;

      const targetRoomId = overrideOptions?.roomId || defaultRoomId;
      const targetAgentId = overrideOptions?.agentId || defaultAgentId;

      try {
        const token = getApiTokenSync();
        const response = await fetch(getApiUrl(apiEndpoint), {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "X-Xavier-Token": token,
          },
          body: JSON.stringify({
            prompt,
            room_id: targetRoomId,
            agent_id: targetAgentId,
            stream: true,
          }),
          signal: controller.signal,
        });

        if (!response.ok) {
          throw new Error(`Agent response failed with status ${response.status}`);
        }

        if (!response.body) {
          throw new Error("ReadableStream not supported by server response");
        }

        if (isMountedRef.current) {
          setIsLoading(false);
        }

        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let partialChunk = "";

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;

          const textChunk = decoder.decode(value, { stream: true });
          const lines = (partialChunk + textChunk).split("\n");
          partialChunk = lines.pop() || "";

          for (const line of lines) {
            const trimmed = line.trim();
            if (!trimmed || trimmed.startsWith(":")) continue; // Skip comments/keepalives

            let payloadStr = trimmed;
            if (trimmed.startsWith("data:")) {
              payloadStr = trimmed.replace(/^data:\s*/, "");
            }

            if (payloadStr === "[DONE]") {
              break;
            }

            try {
              const parsed: TelecomAgentChunk = JSON.parse(payloadStr);
              processChunkData(parsed);
            } catch {
              // Plain text token fallback
              processChunkData({ delta: payloadStr, type: "token" });
            }
          }
        }

        // Process any remaining trailing buffer line
        if (partialChunk.trim() && partialChunk.trim() !== "[DONE]") {
          let payloadStr = partialChunk.trim();
          if (payloadStr.startsWith("data:")) {
            payloadStr = payloadStr.replace(/^data:\s*/, "");
          }
          try {
            const parsed: TelecomAgentChunk = JSON.parse(payloadStr);
            processChunkData(parsed);
          } catch {
            processChunkData({ delta: payloadStr, type: "token" });
          }
        }

        // Wait briefly for smooth typing buffer to drain if smoothTyping is enabled
        if (smoothTyping) {
          await new Promise<void>((resolve) => {
            const checkDrain = setInterval(() => {
              if (
                bufferRef.current.length === 0 &&
                thoughtBufferRef.current.length === 0
              ) {
                clearInterval(checkDrain);
                resolve();
              }
            }, 20);
          });
        }

        if (isMountedRef.current) {
          setIsStreaming(false);
          setIsThinking(false);
          setStreamingText((finalText) => {
            setThoughtText((finalThought) => {
              callbacksRef.current.onFinish?.(finalText, finalThought);
              return finalThought;
            });
            return finalText;
          });
        }
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") {
          // Stream was manually cancelled
          return;
        }

        const streamErr =
          err instanceof Error
            ? err
            : new Error("Unknown error during agent response stream");

        if (isMountedRef.current) {
          setError(streamErr);
          setIsStreaming(false);
          setIsLoading(false);
          setIsThinking(false);
          callbacksRef.current.onError?.(streamErr);
        }
      } finally {
        if (abortControllerRef.current === controller) {
          abortControllerRef.current = null;
        }
      }
    },
    [
      abortStream,
      apiEndpoint,
      defaultAgentId,
      defaultRoomId,
      processChunkData,
      smoothTyping,
    ]
  );

  // Clean up on component unmount
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      clearDrainTimer();
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
        abortControllerRef.current = null;
      }
    };
  }, [clearDrainTimer]);

  return {
    streamingText,
    thoughtText,
    isThinking,
    isStreaming,
    isLoading,
    tokensReceived,
    responseId,
    error,
    sendQuery,
    abortStream,
    resetStream,
  };
}

export default useTelecomAgentResponse;
