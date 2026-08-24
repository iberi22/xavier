#!/usr/bin/env bash
set -e

# Navigate to repo root directory if running from a subdirectory
cd "$(git rev-parse --show-toplevel)"

# Load environment variables if .env exists
if [ -f .env ]; then
  echo "ℹ️ Loading environment variables from .env file..."
  export $(grep -v '^#' .env | xargs)
fi

# Ensure XAVIER_TOKEN is set
if [ -z "$XAVIER_TOKEN" ]; then
  echo "⚠️ WARNING: XAVIER_TOKEN is not set in environment or .env."
fi

# Ensure we operate relative to the current directory for git-related paths
export XAVIER_WORKSPACE_DIR=.

echo "🛡️ Starting Xavier pre-commit documentation and RAG indexing..."

# 1. Harvest recent git commits & diff metrics
echo "📊 Step 1/5: Harvesting recent commits and git diff metrics..."
CARGO_TARGET_DIR=/build/rust-target/xavier-chronicle /build/rust-target/xavier/release/xavier chronicle harvest --workspace .

# 2. Generate Daily Chronicle release post and ingest it to vector store
echo "📝 Step 2/5: Generating Daily Chronicle release notes and indexing into RAG..."
if ! XAVIER_MODEL_PROVIDER=openrouter /build/rust-target/xavier/release/xavier chronicle generate --ingest; then
  echo "⚠️ WARNING: Failed to generate Daily Chronicle (is your local LLM or Ollama server offline?). Skipping chronicle post generation..."
fi

# 3. Generate Code Auto-Docs (module summaries) and ingest them
echo "🔍 Step 3/5: Generating module understanding auto-docs and indexing into RAG..."
CARGO_TARGET_DIR=/build/rust-target/xavier-chronicle /build/rust-target/xavier/release/xavier chronicle auto-docs --ingest

# 4. Compile the static blog
echo "🌐 Step 4/5: Compiling static HTML/CSS/JS DevLog blog..."
CARGO_TARGET_DIR=/build/rust-target/xavier-chronicle /build/rust-target/xavier/release/xavier chronicle build
# Copy the premium human review interactive code diff dashboard
if [ -f web/chronicle/review.html ]; then
  echo "🖥️ Copying Premium Code Diff & Review dashboard to public output..."
  cp web/chronicle/review.html public/devlog/review.html
fi

# Generate real commit/diff JSON database for the main branch
if [ -f scripts/generate-diff-db.js ]; then
  echo "📊 Extracting real main branch git diffs and commit histories..."
  node scripts/generate-diff-db.js
fi

# Establish the communal Maloca human portal link
echo "🛖 Establishing communal Maloca human documentation portal link..."
ln -sfn devlog public/maloca

# 5. Automatically stage generated documentation and blog assets
echo "💾 Step 5/5: Staging newly generated documentation and blog assets..."
# Stage files if they exist or were modified
git add docs/devlog/ docs/auto-docs/ public/devlog/ public/maloca || true

echo "✅ Xavier pre-commit chronicle pipeline successfully completed!"
