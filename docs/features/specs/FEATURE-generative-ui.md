# FEATURE: Generative UI (OpenUI)

**Status:** `beta` | **Score:** 100% | **Last Tested:** 2026-10-15

## Overview
Generative UI (OpenUI) provides dynamic visual presentation surfaces for assistant messages in Xavier's panel UI. When the agent produces structured answers or reasoning execution traces, `UiRenderAgent` formats the output into typed OpenUI pseudo-XML markup. The frontend parses this markup into an abstract component tree and renders native React components (Cards, Accordions, ListBlocks, SpecialCards, TextCallouts) within the chat stream.

## Architecture & Design
1. **Backend Generation (`src/agents/ui_render.rs`):** `UiRenderAgent` generates `openui_lang` pseudo-XML from raw text or full agent execution traces (`AgentRunTrace`), handling attribute escaping, rule tagging, and component extraction (Card, Accordion, Table, TextContent, etc.).
2. **Persistence & Panel Routing (`src/codebase/conversations_db.rs`, `src/cli/handlers/panel.rs`):** Chat messages persist `openui_lang` and associated metadata alongside plain text content in `conversation_messages`. Full conversation and thread lifecycle management includes cascaded thread and message deletion.
3. **Client Parser (`panel-ui/src/utils/openuiParser.ts`):** Parses `openui_lang` markup into a typed `OpenUiNode` AST. Safely handles malformed input with graceful fallbacks.
4. **Interactive Component Surface (`panel-ui/src/components/OpenUi/` & `ChatHistory.tsx`):** A collection of lightweight, responsive React presentation components (`RenderSurface`, `Card`, `SectionBlock`, `Accordion`, `ListBlock`, `SpecialCards`, `TextCallout`) seamlessly integrated into `ChatHistory`.

## Implementation Paths
- `src/agents/ui_render.rs` (UiRenderAgent markup generator and test coverage)
- `src/codebase/conversations_db.rs` (conversations database openui_lang schema and thread deletion)
- `src/cli/handlers/panel.rs` (panel chat processing and persistence)
- `tests/panel_openui_backend.rs` (end-to-end panel chat & thread deletion integration tests)
- `panel-ui/src/utils/openuiParser.ts` (client-side OpenUI pseudo-XML parser)
- `panel-ui/src/components/OpenUi/RenderSurface.tsx` (OpenUI React components & RenderSurface)
- `panel-ui/src/components/ChatHistory.tsx` (assistant message rendering integration)
- `panel-ui/tests/openuiParser.test.ts` (Vitest parser unit tests)
- `panel-ui/tests/OpenUiRenderSurface.test.tsx` (Vitest render surface tests)
- `panel-ui/tests/ChatHistoryOpenUi.test.tsx` (Vitest chat history component tests)
- `panel-ui/tests/e2e/generative-ui.spec.ts` (Playwright E2E browser tests)

## Sub-features
- **OpenUI Markup Generator:** Converts answers and system traces to `<Card>`, `<SectionBlock>`, `<Accordion>`, `<ListBlock>`, `<InfoCard>`, `<DecisionCard>`, and `<QuestionCard>` components.
- **Database Durability & Thread Lifecycle:** Stores `openui_lang` in `conversation_messages` with project scoping and atomic rollback on deletion.
- **Client AST Parser:** Safely parses openui_lang strings into typed hierarchical trees.
- **RenderSurface Container:** Renders typed components with dark/light styling consistent with Xavier Design System.
- **Automated Verification:** Verified across Rust unit tests, backend integration tests, Vitest component suites, and Playwright E2E browser tests.

## Test References
- `render_answer_without_trace_omits_signals_and_confidence`, `escape_attr_preserves_quotes_and_line_breaks`, `render_includes_rich_components` in `src/agents/ui_render.rs`.
- `test_delete_thread_existing`, `test_delete_thread_unknown`, `test_delete_thread_respects_project_scope`, `test_delete_thread_rolls_back_messages_on_failure` in `src/codebase/conversations_db.rs`.
- `production_chat_persists_ui_render_agent_output`, `production_chat_fallback_also_renders_actual_answer` in `tests/panel_openui_backend.rs`.
- `panel-ui/tests/openuiParser.test.ts` (Vitest parser suite).
- `panel-ui/tests/OpenUiRenderSurface.test.tsx` (Vitest surface component suite).
- `panel-ui/tests/ChatHistoryOpenUi.test.tsx` (Vitest chat history integration suite).
- `panel-ui/tests/e2e/generative-ui.spec.ts` (Playwright E2E test suite running in CI).

## Known Issues & Notes
- Merged via PRs #3164 (frontend surface and parser), #3166 (backend persistence and thread deletion), and #3167 (Playwright E2E CI integration).
- Status is `beta` pending production runtime telemetry and validation across diverse agent workloads.
