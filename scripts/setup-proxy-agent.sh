#!/bin/bash
# scripts/setup-proxy-agent.sh
# Sets up OpenCode and Claude Code to use Xavier as a proxy with a leased API key.

set -e

# Config
XAVIER_BIN=${XAVIER_BIN_PATH:-"xavier"}
TTL=86400 # 24 hours
PROXY_URL="http://localhost:8006/v1"
MODEL="glm-5.2"

echo "--- Xavier Proxy Setup for OpenCode & Claude Code ---"

# 1. Lend secret
echo ">> Requesting secret lease from Xavier..."
# We use --ttl $TTL and capture output.
# We assume ZAI_API_KEY is already in the vault.
OUTPUT=$($XAVIER_BIN secrets lend ZAI_API_KEY opencode --ttl $TTL 2>/dev/null || true)

# If it failed, maybe it's because it's not in PATH, try cargo run if we are in the repo
if [ -z "$OUTPUT" ] && [ -f "Cargo.toml" ]; then
    echo ">> Xavier not in PATH, trying cargo run..."
    OUTPUT=$(cargo run --quiet --bin xavier -- secrets lend ZAI_API_KEY opencode --ttl $TTL 2>/dev/null || true)
fi

TOKEN=$(echo "$OUTPUT" | grep "Lease Token:" | awk -F': ' '{print $2}' | tr -d '"' | tr -d '[:space:]')

if [ -z "$TOKEN" ]; then
    echo "Error: Failed to obtain Lease Token from Xavier."
    echo "Make sure Xavier is running and ZAI_API_KEY is set in the vault."
    echo "Command output: $OUTPUT"
    exit 1
fi

echo ">> Obtained Lease Token: $TOKEN"

# 2. Configure OpenCode
OPENCODE_CONFIG="$HOME/.config/opencode/config.json"
mkdir -p "$(dirname "$OPENCODE_CONFIG")"
cat > "$OPENCODE_CONFIG" <<EOF
{
  "base_url": "$PROXY_URL",
  "api_key": "$TOKEN",
  "model": "$MODEL"
}
EOF
echo ">> Configured OpenCode at $OPENCODE_CONFIG"

# 3. Configure Claude Code
CLAUDE_CONFIG="$HOME/.claude/settings.json"
mkdir -p "$(dirname "$CLAUDE_CONFIG")"
cat > "$CLAUDE_CONFIG" <<EOF
{
  "apiBaseUrl": "$PROXY_URL",
  "apiKey": "$TOKEN",
  "model": "$MODEL"
}
EOF
echo ">> Configured Claude Code at $CLAUDE_CONFIG"

echo ""
echo "--- Setup Complete ---"
echo "The agents are now configured to use Xavier proxy."
echo "The lease will expire in 24 hours."
echo ""
echo "To revoke this lease manually, run:"
echo "  xavier secrets revoke $TOKEN"
echo ""
