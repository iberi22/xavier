import os
import re

files = [
    "src/agents/rate_limit.rs",
    "src/secrets/audit.rs",
    "src/security/threat_store.rs",
    "src/time/mod.rs"
]

search_pattern = r"fn init_schema\(&self\) -> (?:anyhow::)?Result<[^>]*> \{\s*(?://[^\n]*\s*)?let rt = match tokio::runtime::Handle::try_current\(\) \{\s*Ok\(handle\) => handle,\s*Err\(_\) => \{\s*let runtime = tokio::runtime::Runtime::new\(\)\s*(?:\.map_err\(|(?:\.context\())[^;]*;\s*runtime\.handle\(\)\.clone\(\)\s*\}\s*\};\s*rt\.block_on\(self\.init_schema_async\(\)\)\s*\}"

replacement = """fn init_schema(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => {
                tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .build()
                        .map_err(|e| anyhow::anyhow!("failed to create temporary runtime: {}", e))?;
                    rt.block_on(self.init_schema_async())
                })
            }
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.block_on(self.init_schema_async())
            }
        }
    }"""

for file_path in files:
    if not os.path.exists(file_path):
        continue
    with open(file_path, 'r') as f:
        content = f.read()

    # Try more flexible regex or just direct replacement for now
    new_content = re.sub(r"fn init_schema\(&self\) -> (?:anyhow::)?Result<[^>]*> \{[\s\S]*?rt\.block_on\(self\.init_schema_async\(\)\)\s*\}", replacement, content)

    if new_content != content:
        with open(file_path, 'w') as f:
            f.write(new_content)
        print(f"Fixed {file_path}")
    else:
        print(f"Could not fix {file_path}")
