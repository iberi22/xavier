# FEATURE: Generative UI (OpenUI)

**Status:** `planned` | **Feature ID:** `feat-generative-ui`

## Overview

Generative UI (OpenUI) provides dynamic, structured UI component surfaces for assistant messages in Xavier's panel UI. When the agent produces structured responses or execution traces, the backend formats output into OpenUI pseudo-XML markup (`openui_lang`). The frontend parses this markup into a typed abstract syntax tree (AST) and renders interactive React components directly within chat history, with robust fallback mechanisms for malformed markup or active streaming.

## Pipeline Architecture

1. **Backend Generation (`src/agents/ui_render.rs`):**
   `UiRenderAgent` formats raw text or structured execution traces (`AgentRunTrace`) into typed OpenUI pseudo-XML (`openui_lang`). Attributes are safely escaped (`&`, `"`, `'`, `<`, `>`, `\n`, `\r`). Supported markup structures include `<Card>`, `<SectionBlock>`, `<Accordion>`, `<ListBlock>`, `<InfoCard>`, `<DecisionCard>`, `<QuestionCard>`, and `<TextCallout>`.

2. **Server Panel Handlers (`src/cli/handlers/panel.rs`):**
   `panel_process_chat_inner` invokes `UiRenderAgent::new().render_answer()` on standard LLM responses as well as memory-fallback paths. Output markup and metadata (applied rules, components) are bundled and persisted with the assistant message. `panel_delete_thread` provides cascade deletion of threads and their messages.

3. **Persistence Layer (`src/codebase/conversations_db.rs`):**
   Stores `openui_lang` as an explicit column in SQLite table `conversation_messages`. Content is encrypted/sealed at rest using `sealer.seal_opt("conv.msg.openui_lang", ...)` and authenticated upon read. Supports project scoping, atomic transaction rollback on failure, and complete thread message cascade deletion.

4. **Client-side AST Parser (`panel-ui/src/utils/openuiParser.ts`):**
   The browser-side `parseOpenUi()` function parses `openui_lang` markup into a typed hierarchical `OpenUiNode` AST. Safely handles malformed XML, unclosed tags, entity unescaping, unknown tag degradation, script tag neutralization (inert text rendering), and JSON item field coercion. Returns `null` on unrecoverable syntax errors to trigger UI fallback.

5. **Render Surface & Chat Integration (`panel-ui/src/components/OpenUi/` & `ChatHistory.tsx`):**
   - `RenderSurface.tsx`: Renders the AST into responsive React components matching the Xavier design system tokens.
   - `ChatHistory.tsx`: Detects `message.openui_lang` on assistant messages and mounts `RenderSurface`. Seamlessly falls back to standard markdown/plain-text rendering if `openui_lang` is null, empty, malformed, or while response streaming is actively in progress.

## Existing Test Suites

The feature is verified across four test levels in the codebase:

### 1. Backend Rust Unit Tests (`src/agents/ui_render.rs`)
- `render_answer_without_trace_omits_signals_and_confidence`: Verifies default plain-text markup generation, rule application, and omission of trace telemetry.
- `escape_attr_preserves_quotes_and_line_breaks`: Tests XML entity escaping for quotes, ampersands, angle brackets, and newlines.
- `render_includes_rich_components`: Verifies structured component extraction (Table, Evidence, Confidence) from full `AgentRunTrace`.

### 2. Database Durability & Lifecycle Tests (`src/codebase/conversations_db.rs`)
- `test_thread_and_message_crud`: Verifies storing and retrieving conversation messages with metadata and `openui_lang`.
- `test_delete_thread_existing`: Validates thread deletion and cascading message removal.
- `test_delete_thread_unknown`: Validates error handling for nonexistent thread IDs.
- `test_delete_thread_respects_project_scope`: Verifies cross-project boundary isolation during thread deletion.
- `test_delete_thread_rolls_back_messages_on_failure`: Confirms transaction rollback guarantees.

### 3. Backend Integration Tests (`tests/panel_openui_backend.rs`, included in `tests/suite_08.rs`)
- `production_chat_persists_ui_render_agent_output`: Validates panel chat execution end-to-end, asserting that `UiRenderAgent` output persists to SQLite and verifies thread deletion via HTTP handler.
- `production_chat_fallback_also_renders_actual_answer`: Asserts that when LLM providers fail and the memory fallback triggers, the actual error/fallback response is safely escaped and rendered in OpenUI markup.

### 4. Frontend Vitest Unit & Component Tests (`panel-ui/tests/`)
- `panel-ui/tests/openuiParser.test.ts` (28 tests): Tests parsing for all component tags, XML entity unescaping, XSS script tag neutralization, unknown tag degradation, and syntax error handling.
- `panel-ui/tests/OpenUiRenderSurface.test.tsx` (8 tests): Tests DOM structure, component hierarchy rendering, accordion toggle interactions, and inert script rendering.
- `panel-ui/tests/ChatHistoryOpenUi.test.tsx` (10 tests): Tests assistant message rendering, fallback to plain text on missing/malformed markup, streaming state fallback, and error boundary recovery.

### 5. Playwright E2E Browser Tests (`panel-ui/tests/e2e/generative-ui.spec.ts`)
- `logs in, sends prompt, renders structured SectionBlock/Card/ListBlock and falls back on malformed markup`: Full browser E2E test verifying authentication, message submission, interactive OpenUI component rendering in Chromium, and graceful fallback on malformed markup.
