# FEATURE: Starlight Documentation Site

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-06-13

## Overview
A comprehensive public-facing documentation platform built with Astro and Starlight. This site serves as the user-facing and developer-facing hub for learning Xavier, hosting quickstarts, deployment strategies, and complete API/architecture documentation.

## Architecture & Design
The documentation project is maintained as an Astro site under the `docs/site/` directory. Starlight's theme provides accessible, high-performance navigation, search capabilities, and multi-language potential. The site is built and deployed automatically via GitHub Actions workflows.

## Implementation Paths
- `docs/site/` (Astro configuration, Astro markdown pages, and Starlight settings)
- `.github/workflows/docs.yml` (automated compilation and publication workflows)

## Sub-features
- **Astro + Starlight Configuration:** Setup layout, dark-mode styling, and plugin integration.
- **Quickstart & User Guides:** Walkthroughs for local installation, Docker Compose setups, and CLI operations.
- **API Reference Documentation:** Endpoints, schema layouts, and token configurations.
- **CI/CD Integration:** Automated deployment to GitHub Pages upon merges into the main branch.

## Test References
- AST validity and link-checking checks run inside the CI/CD pipeline.

## Known Issues & Notes
- Built and published artifacts are completely decoupled from the core Rust package to prevent bloating compiled binaries.
