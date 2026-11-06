import { describe, it, expect } from "vitest";
import { parseFileTransferFrame } from "./fileTransferUtils";
import type { TelecomFrame } from "../../hooks/useTelecomSocket";

describe("parseFileTransferFrame", () => {
  it("should parse file_transfer_progress correctly when not completed", () => {
    const frame: TelecomFrame = {
      event_type: "file_transfer_progress",
      payload: {
        session_id: "123",
        received_chunks: 50,
        total_chunks: 100,
        bytes_transferred: 500,
        is_completed: false,
      },
    };

    const update = parseFileTransferFrame(frame);
    expect(update).toEqual({
      status: "transferring",
      receivedChunks: 50,
      totalChunks: 100,
      chunkProgress: 50,
    });
  });

  it("should parse file_transfer_progress correctly when completed", () => {
    const frame: TelecomFrame = {
      event_type: "file_transfer_progress",
      payload: {
        session_id: "123",
        received_chunks: 100,
        total_chunks: 100,
        bytes_transferred: 1000,
        is_completed: true,
      },
    };

    const update = parseFileTransferFrame(frame);
    expect(update).toEqual({
      status: "completed",
      receivedChunks: 100,
      totalChunks: 100,
      chunkProgress: 100,
    });
  });

  it("should parse file_transfer_failed correctly", () => {
    const frame: TelecomFrame = {
      event_type: "file_transfer_failed",
      payload: {
        session_id: "123",
        reason: "Connection lost",
      },
    };

    const update = parseFileTransferFrame(frame);
    expect(update).toEqual({
      status: "failed",
    });
  });

  it("should return null for unknown event types", () => {
    const frame: TelecomFrame = {
      event_type: "other_event",
      payload: {},
    };

    const update = parseFileTransferFrame(frame);
    expect(update).toBeNull();
  });
});
