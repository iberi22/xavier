import unittest
import tempfile
import importlib.util
from pathlib import Path
import os

# Dynamically load the module since the filename contains a hyphen
module_name = "delivery_dag_check"
file_path = Path(__file__).parent.parent / "delivery-dag-check.py"
spec = importlib.util.spec_from_file_location(module_name, file_path)
check_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_module)

class TestDeliveryDagCheck(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.repo_root = Path(self.temp_dir.name)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_valid_dag(self):
        dag = [
            {"id": "A", "files": ["a.txt"], "depends_on": []},
            {"id": "B", "files": ["b.txt"], "depends_on": ["A"]}
        ]
        reasons = check_module.validate(dag, self.repo_root)
        self.assertEqual(reasons, [])

    def test_cycle(self):
        dag = [
            {"id": "A", "files": ["a.txt"], "depends_on": ["B"]},
            {"id": "B", "files": ["b.txt"], "depends_on": ["A"]}
        ]
        reasons = check_module.validate(dag, self.repo_root)
        self.assertTrue(any("Cycle detected" in r for r in reasons))

    def test_orphan_dependency(self):
        dag = [
            {"id": "A", "files": ["a.txt"], "depends_on": ["C"]}
        ]
        reasons = check_module.validate(dag, self.repo_root)
        self.assertTrue(any("Unknown dependency C" in r for r in reasons))

    def test_path_escapes(self):
        # Absolute path
        dag1 = [
            {"id": "A", "files": ["/etc/passwd"], "depends_on": []}
        ]
        reasons1 = check_module.validate(dag1, self.repo_root)
        self.assertTrue(any("Absolute path not allowed" in r for r in reasons1))

        # .. segment
        dag2 = [
            {"id": "A", "files": ["../outside.rs"], "depends_on": []}
        ]
        reasons2 = check_module.validate(dag2, self.repo_root)
        self.assertTrue(any("Path contains .. segment" in r for r in reasons2))

        # Symlink resolving outside
        outside_target = Path(self.temp_dir.name) / ".." / "outside.txt"
        symlink_path = Path(self.temp_dir.name) / "symlink.txt"
        try:
            os.symlink(outside_target, symlink_path)
            dag3 = [
                {"id": "A", "files": ["symlink.txt"], "depends_on": []}
            ]
            reasons3 = check_module.validate(dag3, self.repo_root)
            self.assertTrue(any("Path escapes repo root" in r for r in reasons3))
        except OSError:
            pass # Ignore if symlinks aren't supported on this OS/setup

    def test_oversize_task(self):
        # Too many files
        dag1 = [
            {"id": "A", "files": ["1", "2", "3", "4", "5"], "depends_on": []}
        ]
        reasons1 = check_module.validate(dag1, self.repo_root)
        self.assertTrue(any("More than 4 files declared" in r for r in reasons1))

        # Too many lines
        dag2 = [
            {"id": "A", "files": ["1"], "depends_on": [], "changed_lines": 401}
        ]
        reasons2 = check_module.validate(dag2, self.repo_root)
        self.assertTrue(any("declared changed_lines above 400" in r for r in reasons2))

    def test_pre_completed_task(self):
        dag = [
            {"id": "A", "files": ["a.txt"], "depends_on": [], "status": "complete"}
        ]
        reasons = check_module.validate(dag, self.repo_root)
        self.assertTrue(any("Task status is complete" in r for r in reasons))

if __name__ == '__main__':
    unittest.main()
