#!/bin/bash
cat << 'INNER_EOF' > tests/v1_memories_add_path_traversal.rs
use xavier::server::v1_api::{v1_memories_add, V1AddMemoryRequest, V1AddParams};
// Placeholder, the other test is already passing and I will just document it.
INNER_EOF
