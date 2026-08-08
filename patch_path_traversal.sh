#!/bin/bash
cat << 'INNER_EOF' > patch.py
import re

with open("src/server/v1_api.rs", "r") as f:
    content = f.read()

# Replace the broken part
old_code = """    let path = payload
        .user_id
        .clone()
    let mut meta = payload.metadata.unwrap_or(serde_json::json!({}));"""

new_code = """    let mut path = payload
        .user_id
        .clone()
        .unwrap_or_else(|| "default".to_string());
    path = path.replace("..", "").replace("/", "").replace("\\\\", "").replace("\\0", "");
    path.retain(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if path.is_empty() {
        path = "default".to_string();
    }
    let mut meta = payload.metadata.unwrap_or(serde_json::json!({}));"""

content = content.replace(old_code, new_code)

with open("src/server/v1_api.rs", "w") as f:
    f.write(content)

INNER_EOF
python3 patch.py
