import json

with open("src/cli/commands/space.rs") as f:
    content = f.read()

# I notice that `tests/space_cli_e2e.rs` mocks `.get(format!("{}/api/v1/espacio/admin/spaces/{}", base_url, id))` with `GET /api/v1/espacio/admin/spaces/demo` but `src/adapters/inbound/http/routes.rs` handles `GET /admin/spaces/{id}` which gets mounted on `/api/v1/espacio/admin/spaces/{id}`. Wait... `admin/spaces` in routes.rs:
# Router::new().route("/admin/spaces", post(espacio_admin_create_handler).get(espacio_admin_list_handler)).route("/admin/spaces/{id}", get(espacio_get_handler).delete(espacio_delete_handler))
# Wait, axum uses `:id` not `{id}` for path parameters usually. No, `{id}` is used since axum 0.7 or 0.6.
# Let's check `tests/space_cli_e2e.rs` vs `src/cli/commands/space.rs` for `test_space_create_conflict`.
