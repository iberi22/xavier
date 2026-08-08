#!/bin/bash
sed -i 's/assert_eq!(resolve_base_url(), "http:\/\/192.168.1.100:8016");/assert_eq!(resolve_base_url_for_port(8016), "http:\/\/192.168.1.100:8016");/' src/cli/tests.rs
