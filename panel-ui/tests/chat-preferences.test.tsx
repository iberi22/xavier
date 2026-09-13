import { act, render, renderHook, screen } from "@testing-library/react";
import React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import ChatHistory, { parseThinkingContent } from "../src/components/ChatHistory";
import InputArea from "../src/components/InputArea";
import {
  CHAT_PREFERENCES_STORAGE_KEY,
  useChatPreferences,
} from "../src/hooks/useChatPreferences";
import { ThemeProvider } from "../src/lib/theme/theme-provider";
import AppearancePage from "../src/pages/Settings/Appearance";
import type { PanelMessage } from "../src/types";

describe("Chat Preferences Hook and UI Integration", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.className = "";
    document.documentElement.removeAttribute("data-theme");
  });

  it("provides default chat preferences and persists updates to localStorage", () => {
    const { result } = renderHook(() => useChatPreferences());

    expect(result.current.verboseChat).toBe(true);
    expect(result.current.conversationWidth).toBe("default");

    act(() => {
      result.current.setVerboseChat(false);
    });

    expect(result.current.verboseChat).toBe(false);
    const stored = JSON.parse(localStorage.getItem(CHAT_PREFERENCES_STORAGE_KEY) || "{}");
    expect(stored.verboseChat).toBe(false);

    act(() => {
      result.current.setConversationWidth("wide");
    });

    expect(result.current.conversationWidth).toBe("wide");
    const storedWide = JSON.parse(localStorage.getItem(CHAT_PREFERENCES_STORAGE_KEY) || "{}");
    expect(storedWide.conversationWidth).toBe("wide");
  });

  it("renders Chat Preferences controls in AppearancePage and updates hook state", () => {
    render(
      <ThemeProvider defaultTheme="studio-dark" storageKey="test-theme">
        <AppearancePage />
      </ThemeProvider>
    );

    expect(screen.getByText("Chat Settings")).toBeDefined();
    expect(screen.getByText("Verbose Agent Chat")).toBeDefined();
    expect(screen.getByText("Conversation Width")).toBeDefined();

    const switchBtn = screen.getByRole("switch", { name: "Verbose Agent Chat" });
    expect(switchBtn.getAttribute("aria-checked")).toBe("true");

    act(() => {
      switchBtn.click();
    });

    expect(switchBtn.getAttribute("aria-checked")).toBe("false");
    const stored = JSON.parse(localStorage.getItem(CHAT_PREFERENCES_STORAGE_KEY) || "{}");
    expect(stored.verboseChat).toBe(false);
  });

  it("correctly parses thinking tags and metadata thoughts", () => {
    const tagged = parseThinkingContent("<think>Analyzing input...</think>Hello world");
    expect(tagged.thinkingText).toBe("Analyzing input...");
    expect(tagged.mainText).toBe("Hello world");

    const meta = parseThinkingContent("Response text", { thinking: "Metadata reasoning" });
    expect(meta.thinkingText).toBe("Metadata reasoning");
    expect(meta.mainText).toBe("Response text");
  });

  it("applies width class to InputArea and ChatHistory and expands thoughts when verboseChat is true", () => {
    localStorage.setItem(
      CHAT_PREFERENCES_STORAGE_KEY,
      JSON.stringify({ verboseChat: true, conversationWidth: "wide" })
    );

    const testMessages: PanelMessage[] = [
      {
        id: "1",
        role: "assistant",
        plain_text: "<think>Deep thoughts here</think>Final answer.",
        created_at: new Date().toISOString(),
      },
    ];

    const { container: chatContainer } = render(
      <ChatHistory messages={testMessages} streamingMessageId={null} />
    );

    // Dynamic width class max-w-5xl for wide
    const outerChatDiv = chatContainer.firstElementChild;
    expect(outerChatDiv?.className).toContain("max-w-5xl");

    // Agent thoughts expanded when verboseChat is true
    expect(screen.getByTestId("agent-thought-process").textContent).toContain("Deep thoughts here");

    const { container: inputContainer } = render(
      <InputArea onSendMessage={() => {}} onOpenConfig={() => {}} />
    );

    const outerInputDiv = inputContainer.firstElementChild;
    expect(outerInputDiv?.className).toContain("max-w-5xl");
  });
});
