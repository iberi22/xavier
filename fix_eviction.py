import re
import os

filepath = 'src/memory/manager/eviction.rs'
with open(filepath, 'r') as f:
    content = f.read()

# find where auto_manage calls decay_memories
# add call to flatten_reorganize after decay_memories

new_call = """
        if self.config.auto_decay_enabled {
            let decay_result = self.decay_memories().await?;
            total_actions += decay_result.documents_affected;

            // Reorganize after decay
            let _ = self.flatten_reorganize().await;
        }
"""

content = re.sub(r'if self\.config\.auto_decay_enabled \{.*?total_actions \+= decay_result\.documents_affected;\s+\}', new_call, content, flags=re.DOTALL)

with open(filepath, 'w') as f:
    f.write(content)
