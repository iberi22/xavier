import type { TelecomFrame } from "../../hooks/useTelecomSocket";

export interface TransferProgressPayload {
  session_id: string;
  received_chunks: number;
  total_chunks: number;
  bytes_transferred: number;
  is_completed: boolean;
}

export interface TransferFailedPayload {
  session_id: string;
  reason: string;
}

export interface FileTransferDrawerStateUpdate {
  status?: "transferring" | "verifying" | "completed" | "failed";
  receivedChunks?: number;
  totalChunks?: number;
  chunkProgress?: number;
}

export function parseFileTransferFrame(frame: TelecomFrame): FileTransferDrawerStateUpdate | null {
  if (frame.event_type === "file_transfer_progress" && frame.payload) {
    const payload = frame.payload as TransferProgressPayload;
    if (typeof payload.received_chunks === "number" && typeof payload.total_chunks === "number") {
      let progress = 0;
      if (payload.total_chunks > 0) {
        progress = Math.floor((payload.received_chunks / payload.total_chunks) * 100);
      }

      let status: "transferring" | "verifying" | "completed" | "failed" = "transferring";
      if (payload.is_completed) {
        status = "completed";
      }

      return {
        status,
        receivedChunks: payload.received_chunks,
        totalChunks: payload.total_chunks,
        chunkProgress: progress,
      };
    }
  } else if (frame.event_type === "file_transfer_failed" && frame.payload) {
    return {
      status: "failed",
    };
  }

  return null;
}
