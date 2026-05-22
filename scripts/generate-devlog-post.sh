#!/bin/bash
# DevLog generator script
# Usage: ./scripts/generate-devlog-post.sh <title> <body> <issue_number>
set -euo pipefail

DATE=$(date +%Y-%m-%d)
TITLE="$1"
BODY="$2"
NUM="$3"
SLUG=$(echo "$TITLE" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/--*/-/g' | sed 's/^-//' | sed 's/-$//')
FILENAME="docs/devlog/${DATE}-${SLUG}.md"

cat > "$FILENAME" << EOF
# ${TITLE}

**Date**: ${DATE}
**Author**: Community Contribution
**Tags**: [generated, devlog]
**Source**: Issue #${NUM}

${BODY}

---

_This DevLog post was auto-generated from issue #${NUM}._
EOF

echo "Post created: ${FILENAME}"
echo "filename=${FILENAME}"
