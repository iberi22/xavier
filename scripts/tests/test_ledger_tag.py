import unittest
import json
import os
import tempfile
import sys
from pathlib import Path

# Add scripts directory to path to import ledger-tag
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
ledger_tag = __import__("ledger-tag")

class FakeRunner:
    def __init__(self, existing_tags=None, file_contents_by_tag=None):
        self.existing_tags = existing_tags or []
        self.file_contents_by_tag = file_contents_by_tag or {}
        self.created_tags = []
        self.pushed_tags = []

    def fetch_tags(self):
        pass

    def list_tags(self):
        return self.existing_tags

    def create_tag(self, tag, sha, message):
        self.created_tags.append((tag, sha, message))
        self.existing_tags.append(tag)

    def push_tag(self, tag):
        self.pushed_tags.append(tag)

    def show_file_at_tag(self, tag, filepath):
        if tag in self.file_contents_by_tag:
            return self.file_contents_by_tag[tag]
        raise Exception(f"File not found at tag {tag}")

class TestLedgerTag(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.features_path = os.path.join(self.temp_dir.name, 'features.json')

    def tearDown(self):
        self.temp_dir.cleanup()

    def write_features(self, data):
        with open(self.features_path, 'w') as f:
            json.dump(data, f)

    def run_tagger(self, runner, dry_run=False):
        class Args:
            def __init__(self, features, sha, dry_run):
                self.features = features
                self.sha = sha
                self.dry_run = dry_run
        args = Args(self.features_path, 'dummy_sha', dry_run)
        ledger_tag.run(args, runner=runner)

    def test_first_tag(self):
        self.write_features({
            "features": {
                "f1": {"status": "stable"},
                "f2": {"status": "beta"}
            }
        })
        runner = FakeRunner()
        self.run_tagger(runner)
        self.assertEqual(len(runner.created_tags), 2)
        tag, sha, msg = runner.created_tags[0]
        self.assertEqual(tag, 'ledger-stable-1')
        self.assertIn("1/2 stable (50.0%)", msg)
        self.assertIn("Newly stable: f1", msg)

    def test_no_tag_when_s_unchanged(self):
        self.write_features({
            "features": {
                "f1": {"status": "stable"},
                "f2": {"status": "beta"}
            }
        })
        runner = FakeRunner(existing_tags=['ledger-stable-1'])
        self.run_tagger(runner)
        self.assertEqual(len(runner.created_tags), 0)

    def test_milestone_crossing_60_to_70(self):
        # 10 features total. Start with 6 stable (60%)
        # Now we have 7 stable (70%)
        features = {}
        for i in range(1, 8):
            features[f"f{i}"] = {"status": "stable"}
        for i in range(8, 11):
            features[f"f{i}"] = {"status": "beta"}

        self.write_features({"features": features})

        prev_features = {}
        for i in range(1, 7):
            prev_features[f"f{i}"] = {"status": "stable"}
        for i in range(7, 11):
            prev_features[f"f{i}"] = {"status": "beta"}

        runner = FakeRunner(
            existing_tags=['ledger-stable-6', 'ledger-milestone-60'],
            file_contents_by_tag={'ledger-stable-6': json.dumps({"features": prev_features})}
        )
        self.run_tagger(runner)
        self.assertEqual(len(runner.created_tags), 2)

        tags = [t[0] for t in runner.created_tags]
        self.assertIn('ledger-stable-7', tags)
        self.assertIn('ledger-milestone-70', tags)

        for t, sha, msg in runner.created_tags:
            if t == 'ledger-stable-7':
                self.assertIn("Newly stable: f7", msg)

    def test_message_lists_newly_stable_ids(self):
        self.write_features({
            "features": {
                "f1": {"status": "stable"},
                "f2": {"status": "stable"},
                "f3": {"status": "stable"},
                "f4": {"status": "beta"}
            }
        })
        prev_features = {
            "features": {
                "f1": {"status": "stable"},
                "f2": {"status": "beta"},
                "f3": {"status": "beta"},
                "f4": {"status": "beta"}
            }
        }
        runner = FakeRunner(
            existing_tags=['ledger-stable-1'],
            file_contents_by_tag={'ledger-stable-1': json.dumps(prev_features)}
        )
        self.run_tagger(runner)
        tag, sha, msg = runner.created_tags[0]
        self.assertIn("Newly stable: f2, f3", msg)

    def test_v_prefixed_name_refused(self):
        with self.assertRaises(SystemExit) as cm:
            ledger_tag.check_tag_name("v1.0.0")
        self.assertEqual(cm.exception.code, 2)

        with self.assertRaises(SystemExit) as cm:
            ledger_tag.check_tag_name("ledger-stable-1a")
        self.assertEqual(cm.exception.code, 2)

        # should not raise
        ledger_tag.check_tag_name("ledger-stable-1")
        ledger_tag.check_tag_name("ledger-milestone-10")

if __name__ == '__main__':
    unittest.main()
