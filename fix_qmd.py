import os
import re

filepath = 'src/memory/qmd/mod.rs'
with open(filepath, 'r') as f:
    content = f.read()

new_method = """
    /// Find nearest neighbors for a given vector.
    pub async fn nearest_neighbors_query(
        &self,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<MemoryDocument>> {
        self.vsearch(query_vector, limit).await
    }
"""

# Insert before the last closing brace of QmdMemory impl block
# QmdMemory has multiple impl blocks. The main one ends before "// -- Free functions --" or similar.
# Let's find "pub async fn invalidate_cache(&self) {" and insert after its closing brace.

target = "    pub async fn invalidate_cache(&self) {\n        reader::invalidate_cache(self).await\n    }\n"
if target in content:
    content = content.replace(target, target + new_method)
else:
    # Fallback: find the last method in the first impl block
    print("Could not find target for QmdMemory method insertion, using fallback")

with open(filepath, 'w') as f:
    f.write(content)
