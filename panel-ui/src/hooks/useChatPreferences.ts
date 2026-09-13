import { useCallback, useEffect, useState } from "react";

export type ConversationWidth = "default" | "narrow" | "wide";

export interface ChatPreferences {
  verboseChat: boolean;
  conversationWidth: ConversationWidth;
}

const DEFAULT_PREFERENCES: ChatPreferences = {
  verboseChat: false,
  conversationWidth: "default",
};

export const CHAT_PREFERENCES_STORAGE_KEY = "xavier_chat_preferences";
export const CHAT_PREFERENCES_EVENT = "xavier_chat_preferences_changed";

export function getStoredPreferences(): ChatPreferences {
  if (typeof window === "undefined" || !window.localStorage) {
    return DEFAULT_PREFERENCES;
  }
  try {
    const raw = localStorage.getItem(CHAT_PREFERENCES_STORAGE_KEY);
    if (!raw) return DEFAULT_PREFERENCES;
    const parsed = JSON.parse(raw);
    const validWidths: ConversationWidth[] = ["default", "narrow", "wide"];
    const parsedWidth = typeof parsed.conversationWidth === "string" ? parsed.conversationWidth.toLowerCase() : "";
    const conversationWidth = validWidths.includes(parsedWidth as ConversationWidth)
      ? (parsedWidth as ConversationWidth)
      : DEFAULT_PREFERENCES.conversationWidth;
    const verboseChat =
      typeof parsed.verboseChat === "boolean"
        ? parsed.verboseChat
        : DEFAULT_PREFERENCES.verboseChat;
    return { verboseChat, conversationWidth };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function useChatPreferences() {
  const [preferences, setPreferencesState] = useState<ChatPreferences>(getStoredPreferences);

  useEffect(() => {
    const handleSync = () => {
      setPreferencesState(getStoredPreferences());
    };

    window.addEventListener("storage", handleSync);
    window.addEventListener(CHAT_PREFERENCES_EVENT, handleSync);

    return () => {
      window.removeEventListener("storage", handleSync);
      window.removeEventListener(CHAT_PREFERENCES_EVENT, handleSync);
    };
  }, []);

  const updatePreferences = useCallback((newPrefs: Partial<ChatPreferences>) => {
    setPreferencesState((prev) => {
      const updated: ChatPreferences = {
        ...prev,
        ...newPrefs,
      };
      try {
        localStorage.setItem(CHAT_PREFERENCES_STORAGE_KEY, JSON.stringify(updated));
        window.dispatchEvent(new Event(CHAT_PREFERENCES_EVENT));
      } catch (e) {
        console.warn("Failed to persist chat preferences:", e);
      }
      return updated;
    });
  }, []);

  const setVerboseChat = useCallback(
    (verboseChat: boolean) => {
      updatePreferences({ verboseChat });
    },
    [updatePreferences]
  );

  const setConversationWidth = useCallback(
    (conversationWidth: ConversationWidth) => {
      updatePreferences({ conversationWidth });
    },
    [updatePreferences]
  );

  return {
    preferences,
    verboseChat: preferences.verboseChat,
    conversationWidth: preferences.conversationWidth,
    setVerboseChat,
    setConversationWidth,
    updatePreferences,
  };
}

export default useChatPreferences;
