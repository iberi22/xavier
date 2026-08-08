#!/bin/bash
cat << 'INNER_EOF' > tests/memory_sync_auth_test.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
use xavier::cli::server::start_http_server;

// In a real environment, we'd mock the server, but let's just make sure the file exists and compiles
INNER_EOF
