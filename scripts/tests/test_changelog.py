import unittest
import sys
import os

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import changelog

class TestChangelog(unittest.TestCase):
    def test_parse_conventional_commit(self):
        # type -> section mapping
        self.assertEqual(changelog.map_type_to_section("feat"), "Added")
        self.assertEqual(changelog.map_type_to_section("fix"), "Fixed")
        self.assertEqual(changelog.map_type_to_section("perf"), "Changed")
        self.assertEqual(changelog.map_type_to_section("docs"), "Documentation")
        self.assertEqual(changelog.map_type_to_section("test"), "Maintenance")
        self.assertEqual(changelog.map_type_to_section("sec"), "Security")
        self.assertEqual(changelog.map_type_to_section("security"), "Security")
        self.assertEqual(changelog.map_type_to_section("fix", "security"), "Security")
        self.assertEqual(changelog.map_type_to_section("other"), "Other")

        # fallback for non-conventional titles to Other
        t, sc, subj = changelog.parse_conventional_commit("Non-conventional commit")
        self.assertEqual(t, None)
        self.assertEqual(sc, None)
        self.assertEqual(subj, "Non-conventional commit")
        self.assertEqual(changelog.map_type_to_section(t), "Other")

    def test_format_prs_dedupe_and_dependabot(self):
        prs = [
            {"number": 1, "title": "feat(api): add v2", "author": {"login": "user1"}},
            {"number": 1, "title": "feat(api): add v2 (duplicate)", "author": {"login": "user1"}},
            {"number": 2, "title": "build(deps): bump x from 1 to 2", "author": {"login": "dependabot"}},
            {"number": 3, "title": "chore: another dep", "author": {"login": "dependabot[bot]"}},
            {"number": 4, "title": "fix: crash", "author": {"login": "user2"}},
            {"number": 5, "title": "sec: oauth state validation", "author": {"login": "user3"}},
            {"number": 6, "title": "fix(security): gate code index", "author": {"login": "user4"}},
        ]

        sections = changelog.format_prs(prs)

        # Deduplication
        self.assertEqual(len(sections["Added"]), 1)
        self.assertEqual(sections["Added"][0], "- **api**: add v2 (#1)")

        # Dependabot collapse
        self.assertTrue("- Dependencies: 2 updates" in sections["Maintenance"])

        self.assertEqual(len(sections["Fixed"]), 1)

        # Security
        self.assertEqual(len(sections["Security"]), 2)
        self.assertEqual(sections["Security"][0], "- oauth state validation (#5)")
        self.assertEqual(sections["Security"][1], "- **security**: gate code index (#6)")

    def test_replace_unreleased_section(self):
        # replace markdown section safely without touching other parts
        content = """# Changelog

## [Unreleased]

Old stuff
that should go away

## [0.2.17] - 2026-09-29
- released item 1
"""
        new_body = "### Added\n- new stuff\n"

        new_content = changelog.replace_unreleased_section(content, new_body)

        expected = """# Changelog

## [Unreleased]

### Added
- new stuff

## [0.2.17] - 2026-09-29
- released item 1
"""
        self.assertEqual(new_content, expected)

    def test_replace_unreleased_section_empty(self):
        content = """# Changelog

## [Unreleased]

## [0.2.17]
"""
        new_body = ""
        new_content = changelog.replace_unreleased_section(content, new_body)
        expected = """# Changelog

## [Unreleased]

## [0.2.17]
"""
        self.assertEqual(new_content, expected)

    def test_run_cmd_mocked(self):
        def mock_runner(cmd, **kwargs):
            class MockResult:
                def __init__(self, stdout):
                    self.stdout = stdout
            return MockResult("2026-09-29T17:20:31-05:00\n")

        res = changelog.get_tag_date("v0.2.17", runner=mock_runner)
        self.assertEqual(res, "2026-09-29T17:20:31-05:00")

if __name__ == "__main__":
    unittest.main()
