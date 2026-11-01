cargo run --features ci-safe --bin xavier -- http 0 --mcp-port 0 &
PID=$!
sleep 2
# Hit the endpoint
curl -X POST http://127.0.0.1:0/api/v1/espacio/admin/spaces -H "X-Xavier-Token: wp13o-e2e-root-token" -H "Content-Type: application/json" -d '{"id": "demo", "name": "demo", "description": "a b", "owner_node": "CLI", "is_public": false}'
kill $PID
