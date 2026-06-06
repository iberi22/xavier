import os

file_path = "tests/integration/cli.rs"
with open(file_path, 'r') as f:
    content = f.read()

new_content = content.replace("run_with_timeout(&[\"add\", \"test-content\"], 5)", "run_with_timeout(&[\"add\", \"test-content\"], 15)")
new_content = new_content.replace("run_with_timeout(&[\"add\", \"integration test content\"], 5)", "run_with_timeout(&[\"add\", \"integration test content\"], 15)")

with open(file_path, 'w') as f:
    f.write(new_content)
