import unittest
import subprocess
import json
import os
from unittest.mock import patch

class TestRustImpact(unittest.TestCase):
    def setUp(self):
        self.mock_metadata = {
            "workspace_members": ["code-graph", "parser-rust", "xavier", "xavier-core-logic", "xavier-wasm"],
            "packages": [
                {
                    "id": "code-graph", "name": "code-graph", "manifest_path": "/app/code-graph/Cargo.toml",
                    "targets": [{"name": "code_graph", "kind": ["lib"], "src_path": "/app/code-graph/src/lib.rs"}]
                },
                {
                    "id": "parser-rust", "name": "parser-rust", "manifest_path": "/app/code-graph/parsers/parser-rust/Cargo.toml",
                    "targets": [{"name": "parser_rust", "kind": ["lib"], "src_path": "/app/code-graph/parsers/parser-rust/src/lib.rs"}]
                },
                {
                    "id": "xavier", "name": "xavier", "manifest_path": "/app/Cargo.toml",
                    "targets": [
                        {"name": "xavier", "kind": ["bin"], "src_path": "/app/src/main.rs"},
                        {"name": "auth_register_test", "kind": ["test"], "src_path": "/app/tests/integration/auth_register_test.rs"}
                    ]
                },
                {
                    "id": "xavier-core-logic", "name": "xavier-core-logic", "manifest_path": "/app/crates/xavier-core-logic/Cargo.toml",
                    "targets": [{"name": "xavier_core_logic", "kind": ["lib"], "src_path": "/app/crates/xavier-core-logic/src/lib.rs"}]
                },
                {
                    "id": "xavier-wasm", "name": "xavier-wasm", "manifest_path": "/app/crates/xavier-wasm/Cargo.toml",
                    "targets": [{"name": "xavier_wasm", "kind": ["lib"], "src_path": "/app/crates/xavier-wasm/src/lib.rs"}]
                }
            ],
            "resolve": {
                "nodes": [
                    {"id": "code-graph", "deps": [{"pkg": "parser-rust"}]},
                    {"id": "xavier", "deps": [{"pkg": "code-graph"}, {"pkg": "xavier-core-logic"}, {"pkg": "xavier-wasm"}]},
                    {"id": "xavier-wasm", "deps": [{"pkg": "xavier-core-logic"}]}
                ]
            }
        }
        with open('mock_meta.json', 'w') as f:
            json.dump(self.mock_metadata, f)

    def tearDown(self):
        if os.path.exists('mock_meta.json'):
            os.remove('mock_meta.json')

    def run_script(self, args, check=True):
        cmd = ["python3", "scripts/rust-impact.py", "--mock-metadata", "mock_meta.json"] + args
        result = subprocess.run(cmd, capture_output=True, text=True)
        if check and result.returncode != 0:
            print(result.stderr)
            result.check_returncode()
        return result

    def test_parser_rust_selects_code_graph_and_xavier(self):
        result = self.run_script(["--paths", "code-graph/parsers/parser-rust/src/lib.rs"])
        out = json.loads(result.stdout)
        self.assertFalse(out["fallback"])
        names = [p["name"] for p in out["packages"]]
        self.assertIn("parser-rust", names)
        self.assertIn("code-graph", names)
        self.assertIn("xavier", names)

    def test_nested_build_rs_cargo_config_and_toolchain_are_full_fallback(self):
        for path in ["crates/xavier-core-logic/build.rs", "rust-toolchain", "rust-toolchain.toml", "Cargo.toml", "Cargo.lock"]:
            result = self.run_script(["--paths", path])
            out = json.loads(result.stdout)
            self.assertTrue(out["fallback"])

    def test_vendor_path_is_full_fallback(self):
        result = self.run_script(["--paths", "vendor/maloca-core/src/lib.rs"])
        out = json.loads(result.stdout)
        self.assertTrue(out["fallback"])

    def test_fallback_package_count_is_every_member(self):
        result = self.run_script(["--paths", "Cargo.toml"])
        out = json.loads(result.stdout)
        self.assertEqual(len(out["packages"]), 5)

    def test_integration_test_names_its_target(self):
        result = self.run_script(["--paths", "tests/integration/auth_register_test.rs"])
        out = json.loads(result.stdout)
        self.assertFalse(out["fallback"])
        xavier_pkg = next(p for p in out["packages"] if p["name"] == "xavier")
        self.assertIn("auth_register_test", xavier_pkg["targets"])

    def test_empty_metadata_exits_nonzero(self):
        with open('mock_meta.json', 'w') as f:
            json.dump({}, f)
        result = self.run_script(["--paths", "tests/integration/auth_register_test.rs"], check=False)
        self.assertNotEqual(result.returncode, 0)

    def test_missing_resolve_exits_nonzero(self):
        del self.mock_metadata["resolve"]
        with open('mock_meta.json', 'w') as f:
            json.dump(self.mock_metadata, f)
        result = self.run_script(["--paths", "tests/integration/auth_register_test.rs"], check=False)
        self.assertNotEqual(result.returncode, 0)

    def test_rename_keeps_old_package(self):
        # Pass both paths for a simulated rename via paths.
        # This will select both xavier-core-logic (old) and xavier-wasm (new).
        # We can also verify that get_git_diff output works correctly in bash manually.
        result = self.run_script(["--paths", "crates/xavier-core-logic/src/a.rs", "crates/xavier-wasm/src/a.rs"])
        out = json.loads(result.stdout)
        self.assertFalse(out["fallback"])
        names = [p["name"] for p in out["packages"]]
        self.assertIn("xavier-core-logic", names)
        self.assertIn("xavier-wasm", names)
        self.assertIn("xavier", names)

    def test_non_rust_file_inside_parser_crate_does_not_select_only_parser(self):
        result = self.run_script(["--paths", "code-graph/parsers/parser-rust/src/data.txt"])
        out = json.loads(result.stdout)
        self.assertTrue(out["fallback"])
        self.assertEqual(len(out["packages"]), 5)

    def test_src_change_does_not_select_core_logic(self):
        result = self.run_script(["--paths", "src/main.rs"])
        out = json.loads(result.stdout)
        names = [p["name"] for p in out["packages"]]
        self.assertIn("xavier", names)
        self.assertNotIn("xavier-core-logic", names)

    def test_core_logic_selects_xavier_and_wasm(self):
        result = self.run_script(["--paths", "crates/xavier-core-logic/src/lib.rs"])
        out = json.loads(result.stdout)
        names = [p["name"] for p in out["packages"]]
        self.assertIn("xavier-core-logic", names)
        self.assertIn("xavier-wasm", names)
        self.assertIn("xavier", names)

    def test_app_tauri(self):
        result = self.run_script(["--paths", "panel-ui/src-tauri/src/main.rs"])
        out = json.loads(result.stdout)
        self.assertFalse(out["fallback"])
        names = [p["name"] for p in out["packages"]]
        self.assertIn("app", names)
        app_pkg = next(p for p in out["packages"] if p["name"] == "app")
        self.assertIn("app", app_pkg["targets"])

    def test_zero_selected_fails_for_package_without_targets(self):
        for p in self.mock_metadata["packages"]:
            if p["name"] == "xavier":
                p["targets"] = []
        with open('mock_meta.json', 'w') as f:
            json.dump(self.mock_metadata, f)
        result = self.run_script(["--paths", "src/main.rs"], check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Error: Selected package xavier has no test target", result.stderr)

    def test_zero_selected_fails(self):
        result = self.run_script(["--paths"], check=False)
        self.assertNotEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()
