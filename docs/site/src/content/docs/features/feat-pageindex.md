---
title: "PageIndex Tree Retrieval"
description: "Index long documents as a table-of-contents tree and let the calling agent navigate it with pageindex_* tools"
---

**Status:** `stable`

## Overview
Documents (PDF, markdown, text, legal) are indexed as a hierarchical table-of-contents tree. The calling agent navigates it with six `pageindex_*` MCP tools (or `/v1/pageindex/*`), reading only the pages it needs. No chunking, no embeddings, no LLM inside Xavier. Inspired by Vectify AI's MIT-licensed [PageIndex](https://github.com/VectifyAI/PageIndex); reimplemented from scratch in Rust.

## Evidence
On the public PageIndex-OSS-Benchmark (62 questions, 34 PDFs) an agent using the tools reached 61/62 reading 11.6 pages per question, and 60/62 reading 1.7 pages per question with in-document search. It complements BM25/vector retrieval and does not replace it.

## Learn more
Full guide, rationale, attribution, evaluation tables and setup: `docs/features/pageindex.md` in the repository, and ADR-036.
