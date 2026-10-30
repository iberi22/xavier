import unittest
from pathlib import Path

class TestLedgerPromoteWorkflow(unittest.TestCase):
    def setUp(self):
        self.workflow_path = Path('.github/workflows/ledger-promote.yml')
        self.assertTrue(self.workflow_path.exists())
        self.workflow_content = self.workflow_path.read_text()
        try:
            import yaml
            self.yaml_available = True
            from yaml.loader import SafeLoader
            class StringLoader(SafeLoader):
                pass
            import yaml.resolver
            for ch in "yYnNtTfFoO":
                if ch in StringLoader.yaml_implicit_resolvers:
                    StringLoader.yaml_implicit_resolvers[ch] = [x for x in StringLoader.yaml_implicit_resolvers[ch] if x[0] != 'tag:yaml.org,2002:bool']
            with open(self.workflow_path) as f:
                self.workflow = yaml.load(f, Loader=StringLoader)
        except ImportError:
            self.yaml_available = False

    def test_triggers(self):
        if not self.yaml_available:
            self.skipTest("PyYAML not available")
        triggers = self.workflow.get('on', {})
        self.assertIn('workflow_dispatch', triggers)
        self.assertIn('workflow_run', triggers)
        self.assertIn('schedule', triggers)
        self.assertIn('Xavier CI / CD', triggers['workflow_run']['workflows'])
        self.assertIn('E2E Segmented', triggers['workflow_run']['workflows'])
        self.assertIn('completed', triggers['workflow_run']['types'])
        self.assertEqual(triggers['schedule'][0]['cron'], '17 */6 * * *')

    def test_propose_condition(self):
        if not self.yaml_available:
            self.skipTest("PyYAML not available")
        propose = self.workflow['jobs'].get('propose', {})
        condition = propose.get('if', '')
        self.assertIn("needs.evaluate.outputs.mode == 'apply'", condition)
        self.assertIn("needs.evaluate.outputs.eligible_count", condition)

    def test_apply_demotions_string(self):
        self.assertNotIn("git tag", self.workflow_content)

        # verify apply demotion ONLY under workflow_dispatch
        propose_job_content = ""
        in_propose = False
        for line in self.workflow_content.splitlines():
            if line.startswith("  propose:"):
                in_propose = True
            elif in_propose and line.startswith("  ") and not line.startswith("    "): # next job
                in_propose = False

            if in_propose:
                propose_job_content += line + "\n"

        self.assertTrue("--apply-demotions" in propose_job_content)
        self.assertTrue("github.event_name" in propose_job_content and "workflow_dispatch" in propose_job_content)

if __name__ == '__main__':
    unittest.main()
