---
title: "TASK: Fix workspace and storage isolation for memory runtime"
labels:
  - ai-plan
  - release
  - storage
assignees: []
---

## Description

Usage testing showed memory search results leaking existing workspace content while the server was started with temporary database paths intended to isolate the test run. That makes the current storage contract too ambiguous for a `1.0` release.

## Acceptance Criteria

- [x] define the canonical environment variables for all memory and workspace storage paths
- [x] verify those variables fully isolate a fresh runtime from previous local data
- [x] document which paths belong to code graph, vector memory, workspace state, and panel threads
- [x] add an automated test proving an isolated temporary workspace stays isolated
- [x] remove or explain any implicit seeding that makes empty-state testing ambiguous

## Evidence

- direct HTTP search returned older default-workspace memories during a temporary isolated test run
