# FEATURE: Cloudflare Turnstile Frictionless CAPTCHA

**Status:** `planned` | **Category:** `security`

## Overview
Invisible and frictionless Cloudflare Turnstile CAPTCHA protection for authentication endpoints (`/auth/magic-link`), preventing automated spam and credential stuffing at edge.

## Architecture & Design
- Client-side: Dynamic Turnstile script loader with dark theme container in `LoginPage.tsx`.
- Edge validation: Cloudflare Pages Functions (`panel-ui/functions/[[path]].ts`) intercepting auth requests and validating tokens with `https://challenges.cloudflare.com/turnstile/v0/siteverify`.
- Fail-closed security: Missing or invalid CAPTCHA tokens fail with HTTP 403 Forbidden.

## Implementation Paths
- `panel-ui/src/auth/LoginPage.tsx` (Turnstile widget container and token delivery)
- `panel-ui/functions/[[path]].ts` (Cloudflare Pages edge function gate)

## Test References
- `panel-ui/tests/e2e/auth-adversarial-edge-cases.spec.ts` adversarial test rejecting invalid CAPTCHA tokens and allowing valid tokens.
