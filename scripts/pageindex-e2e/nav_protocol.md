# PageIndex navigator protocol (e2e evaluation)

You answer questions about PDF documents using ONLY Xavier's PageIndex MCP tools, the way an agent would in production.

Tool access: run `./pi.py <tool> '<json args>'` from the scripts/pageindex-e2e directory (with XAVIER_TOKEN or PI_TOKEN_FILE set). Tools:
- pageindex_get_document {"doc_name"}             → page count, builder, hints
- pageindex_get_document_structure {"doc_name", "max_depth"?, "node_id"?} → section tree (titles, page ranges), no text
- pageindex_get_page_content {"doc_name", "pages": "5-7,12"} → page text (max 20 pages/call)
- pageindex_search {"doc_name", "query", "limit"?} → ranked pages/sections with short snippets (no full text)

Before EACH question: `export PI_LOG=nav_logs/q<qid>.jsonl` (exactly this name, nothing appended) (create nav_logs/ if missing) so every call you make for that question is recorded.

Method per question: read the structure first (use pageindex_search when the structure does not point to an obvious section), reason about which section most likely holds the answer, fetch only those pages, and widen only if needed. Be economical: prefer tight page ranges; never dump the whole document unless the structure is useless (then fetch in chunks and stop as soon as you find the answer).

HARD RULES (the evaluation is void otherwise):
- Do NOT open, read, grep, or list the benchmark checkout (it contains the answer key), and do not read the PDFs directly by any other means (no pdftotext, python PDF libs, etc.).
- Do not use web search or prior knowledge to answer; the answer must come from pages you fetched.
- Do not modify pi.py, the server, or other batches' files.

Output: write `nav_answers_<batch>.json` = list of {"qid", "answer" (short, exact fact), "evidence_pages" (list of ints you based the answer on), "confident" (bool)}. If the answer is not found, answer "NOT FOUND" with the pages you checked.
