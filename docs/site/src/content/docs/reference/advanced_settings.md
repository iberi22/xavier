---
title: Advanced Settings
description: Detailed configuration for Xavier
---

# Advanced Settings Reference

This document provides detailed information about advanced configuration options in Xavier. These settings are primarily managed via `config/xavier.config.json` or environment variables.

## Overview of Advanced Sections

Xavier's configuration is divided into several logical sections:

- **PgHeart**: Integration with the PgHeart persistence and heartbeat service.
- **Chronicle**: Automated documentation and project activity harvesting.
- **Agents**: Global agentic behavior and budgeting.
- **Retrieval**: Tuning the vector search and RAG pipeline.
- **Memory Layers**: Configuration for working and episodic memory.

---

## PgHeart Settings

PgHeart is a plugin that provides external persistence via Supabase and periodic heartbeat monitoring.

| Key | Environment Variable | Default | Description |
|---|---|---|---|
| `url` | `XAVIER_PGHEART_URL` | `null` | URL of the PgHeart service or Supabase REST endpoint. |
| `token` | `XAVIER_PGHEART_TOKEN` | `null` | Authentication token for the service. |
| `instance_id` | `XAVIER_PGHEART_INSTANCE_ID` | `null` | Unique identifier for this Xavier instance in PgHeart. |
| `sync_interval_ms` | `XAVIER_PGHEART_SYNC_INTERVAL_MS` | `60000` | Frequency of heartbeats and synchronization in milliseconds. |
| `auto_heartbeat` | `XAVIER_PGHEART_AUTO_HEARTBEAT` | `true` | Whether to automatically start the heartbeat loop on startup. |

---

## Chronicle Settings

Chronicle automates the process of harvesting technical decisions and activity to generate documentation.

| Key | Environment Variable | Default | Description |
|---|---|---|---|
| `model` | `XAVIER_CHRONICLE_MODEL` | (empty) | The LLM model used for analyzing activity and generating posts. |

---

## Agent Settings

Global settings for AI agents spawned or managed by Xavier.

| Key | Environment Variable | Default | Description |
|---|---|---|---|
| `weekly_budget` | `XAVIER_AGENTS_WEEKLY_BUDGET` | `null` | Maximum allowed spend (in internal units/tokens) per week. |

---

## Advanced General Settings

Found under the `advanced` section in the config.

| Key | Environment Variable | Default | Description |
|---|---|---|---|
| `qjl_threshold` | `XAVIER_QJL_THRESHOLD` | `500` | Threshold for Quick Journaling Logic (QJL) processing. |
| `entity_extraction_enabled` | `XAVIER_ENTITY_EXTRACTION_ENABLED` | `true` | Enable automatic NER (Named Entity Extraction) during memory ingestion. |
| `audit_chain_enabled` | `XAVIER_AUDIT_CHAIN_ENABLED` | `true` | Enable cryptographic chaining of audit logs. |
| `panel_store_dir` | `XAVIER_PANEL_STORE_DIR` | (empty) | Directory where Panel UI conversation threads are stored. |

---

## Retrieval & Search Tuning

Tuning the hybrid search (Vector + BM25) and RAG pipeline.

| Key | Environment Variable | Default | Description |
|---|---|---|---|
| `disable_hyde` | `XAVIER_DISABLE_HYDE` | `true` | Disable Hypothetical Document Embeddings (HyDE). |
| `rrf_k` | `XAVIER_RRF_K` | `60` | The `k` parameter for Reciprocal Rank Fusion. |
| `zone_boost_multiplier` | `XAVIER_ZONE_BOOST_MULTIPLIER` | `1.5` | Multiplier for hits in "priority" zones/paths. |
| `zone_penalty_multiplier` | `XAVIER_ZONE_PENALTY_MULTIPLIER` | `0.5` | Multiplier for hits in "ignored" or "noisy" zones. |

---

## Memory Layer Configuration

Xavier uses a layered memory architecture: **Working** (fast, LRU) and **Episodic** (long-term, summarized).

### Working Memory
| Key | Default | Description |
|---|---|---|
| `capacity` | `100` | Number of documents kept in high-speed cache. |
| `bm25_k1` | `1.5` | BM25 ranking parameter `k1`. |
| `bm25_b` | `0.75` | BM25 ranking parameter `b`. |

### Episodic Memory
| Key | Default | Description |
|---|---|---|
| `summary_window` | `10` | Number of events before generating a summary. |
| `max_sessions` | `50` | Maximum number of concurrent active sessions. |
| `min_event_importance` | `0.5` | Minimum importance score (0.0-1.0) to keep an event without summarization. |
