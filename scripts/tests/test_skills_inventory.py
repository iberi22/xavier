import importlib.util
from io import StringIO
import os
import sys
import tempfile
import unittest
from unittest.mock import patch
import json

class TestSkillsInventory(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "skills-inventory.py")
        spec = importlib.util.spec_from_file_location("skills_inventory", path)
        cls.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.mod)

    def run_main(self, args_list):
        with patch("sys.stdout", new_callable=StringIO) as out_buf:
            self.mod.main(args_list)
            return out_buf.getvalue()

    def test_same_hash_duplicate(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")
            root2 = os.path.join(td, "root2")
            os.makedirs(os.path.join(root1, "pkgA"))
            os.makedirs(os.path.join(root2, "pkgA"))

            skill_content = "---\nname: my_skill\n---\nbody"
            with open(os.path.join(root1, "pkgA", "SKILL.md"), "w") as f:
                f.write(skill_content)
            with open(os.path.join(root2, "pkgA", "SKILL.md"), "w") as f:
                f.write(skill_content)

            out = self.run_main(["--root", root1, root2])
            self.assertIn("INVENTORY: 2 packages, 0 conflicts", out)

    def test_divergent_duplicate(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")
            root2 = os.path.join(td, "root2")
            os.makedirs(os.path.join(root1, "pkgB"))
            os.makedirs(os.path.join(root2, "pkgB"))

            with open(os.path.join(root1, "pkgB", "SKILL.md"), "w") as f:
                f.write("---\nname: my_skill_b\n---\nbody 1")
            with open(os.path.join(root2, "pkgB", "SKILL.md"), "w") as f:
                f.write("---\nname: my_skill_b\n---\nbody 2")

            out = self.run_main(["--root", root1, root2])
            self.assertIn("INVENTORY: 2 packages, 1 conflicts", out)

    def test_symlinked_package_and_broken_link(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")
            os.makedirs(root1)

            # Real package outside root
            real_pkg = os.path.join(td, "real_pkg")
            os.makedirs(real_pkg)
            with open(os.path.join(real_pkg, "SKILL.md"), "w") as f:
                f.write("---\nname: sym_skill\n---\n")

            # Valid symlink package
            os.symlink(real_pkg, os.path.join(root1, "pkg_sym"))

            # Broken link
            os.symlink(os.path.join(td, "does_not_exist"), os.path.join(root1, "pkg_broken"))

            # Broken file link inside package
            os.symlink(os.path.join(td, "no_file"), os.path.join(real_pkg, "bad_file"))

            out = self.run_main(["--root", root1])
            self.assertIn("link ->", out)
            self.assertIn("sym_skill", out)
            # 1 broken package dir link + 1 broken file link = 2 broken links
            self.assertIn("2 broken links", out)

    def test_fake_secret(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")
            os.makedirs(os.path.join(root1, "pkgC"))

            secret_value = "ghp_abcdefghijklmnopqrstuvwxyz123456"
            with open(os.path.join(root1, "pkgC", "SKILL.md"), "w") as f:
                f.write(f"---\nname: secret_skill\n---\nMy token is {secret_value}\n")

            out = self.run_main(["--root", root1])
            self.assertIn("1 secret findings", out)
            self.assertIn("github_pat", out)
            self.assertIn("SKILL.md:4", out)
            self.assertNotIn(secret_value, out)

    def test_bounded_walk(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")

            # Very deep package
            deep_dir = root1
            for i in range(10):
                deep_dir = os.path.join(deep_dir, f"dir_{i}")
            os.makedirs(deep_dir)

            # This SKILL.md is deeper than 4 levels from root, so it won't be found
            with open(os.path.join(deep_dir, "SKILL.md"), "w") as f:
                f.write("---\nname: deep_skill\n---\n")

            out = self.run_main(["--root", root1])
            self.assertIn("INVENTORY: 0 packages", out)

    def test_json_output(self):
        with tempfile.TemporaryDirectory() as td:
            root1 = os.path.join(td, "root1")
            os.makedirs(os.path.join(root1, "pkgD"))

            with open(os.path.join(root1, "pkgD", "SKILL.md"), "w") as f:
                f.write("---\nname: json_skill\n---\n")

            out_file = os.path.join(td, "out.json")
            out = self.run_main(["--root", root1, "--json", "--out", out_file])

            with open(out_file, "r") as f:
                data = json.load(f)
                self.assertEqual(len(data), 1)
                self.assertEqual(data[0]['name'], 'json_skill')

if __name__ == "__main__":
    unittest.main()
