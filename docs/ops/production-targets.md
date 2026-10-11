# Production Targets & Credentials Guide

This guide details the least-privilege credentials required for each production target kind in the Xavier repository, how to store them, and how to validate your setup before activation (R11 requirement).

## cf_pages
To deploy to Cloudflare Pages, you need an API Token and your Account ID.

**Cloudflare API Token scopes required:**
* Cloudflare Pages: Edit
* Cloudflare Pages: Read

**Storage:**
Store the API token in your CI/CD environment secrets as `CLOUDFLARE_API_TOKEN` and your Account ID as `CLOUDFLARE_ACCOUNT_ID`.

## cf_worker
To deploy Cloudflare Workers, you need an API Token and your Account ID.

**Cloudflare API Token scopes required:**
* Workers Scripts: Edit
* Workers Routes: Edit

**Storage:**
Store the API token in your CI/CD environment secrets as `CLOUDFLARE_API_TOKEN` and your Account ID as `CLOUDFLARE_ACCOUNT_ID`.

## gh_pages
To deploy to GitHub Pages from a GitHub Actions workflow or locally, you need a fine-grained Personal Access Token (PAT).

**GitHub Fine-Grained Token permissions required:**
* Contents: Write
* Pages: Write

**Storage:**
Store the fine-grained token in your CI/CD environment secrets as `GH_TOKEN`.

## release_tag
To create a GitHub Release, you need a fine-grained Personal Access Token (PAT).

**GitHub Fine-Grained Token permissions required:**
* Contents: Write
* Repository releases: Write

**Storage:**
Store the fine-grained token in your CI/CD environment secrets as `GH_TOKEN`.

## local_binary
Local binary targets are handled locally without the need for external platform credentials. No credentials required.

---

## Preflight Execution & Activation Drill (R11)

Before you can activate Guardian automation for your targets, you must run the preflight check to ensure the required variables are correctly set in the environment.

1. **Run the preflight script:**
   Execute the preflight script with your inventory JSON to validate the targets and credentials.
   ```bash
   bash scripts/check-production-targets.sh <inventory.json>
   ```
   The script will verify the target schemas and check the presence of required environment variables without printing their values.

2. **Activation Drill (R11):**
   Guardian automation mandates that the operator acknowledges the risk of activation. You must set `XAVIER_GUARDIAN_ACCEPTED=1` in your environment before the preflight will pass and automation can begin.
   ```bash
   export XAVIER_GUARDIAN_ACCEPTED=1
   ```
