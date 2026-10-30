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

    def _propose_block(self):
        lines = []
        in_propose = False
        for line in self.workflow_content.splitlines():
            if line.startswith("  propose:"):
                in_propose = True
            elif in_propose and line.startswith("  ") and not line.startswith("    "):
                in_propose = False
            if in_propose:
                lines.append(line)
        self.assertTrue(lines, "propose job not found")
        return lines

    def test_apply_demotions_manual_only(self):
        self.assertNotIn("git tag", self.workflow_content)
        lines = self._propose_block()
        block = "\n".join(lines)
        # The only source of the demotion switch is an explicit manual dispatch.
        self.assertIn(
            "MANUAL_DEMOTIONS: ${{ github.event_name == 'workflow_dispatch' && inputs.apply_demotions == true }}",
            block,
        )
        self.assertEqual(self.workflow_content.count("MANUAL_DEMOTIONS:"), 1)
        flag_lines = [i for i, l in enumerate(lines) if "--apply-demotions" in l]
        self.assertEqual(len(flag_lines), 1, "--apply-demotions must appear exactly once")
        # Walk back from the flag to the enclosing shell `if`: it must be the
        # manual-only guard, with no `else` branch in between.
        i = flag_lines[0]
        guard = None
        for j in range(i, -1, -1):
            stripped = lines[j].strip()
            self.assertFalse(
                stripped.startswith("else") or stripped.startswith("elif"),
                "--apply-demotions reachable from a non-manual branch",
            )
            if stripped.startswith("if ["):
                guard = stripped
                break
        self.assertEqual(guard, 'if [ "$MANUAL_DEMOTIONS" = "true" ]; then')
        # Every demotion-related shell guard uses the manual-only switch.
        for l in lines:
            if "apply_demotions" in l and "MANUAL_DEMOTIONS:" not in l and not l.strip().startswith(("if:", "#")):
                self.fail(f"apply_demotions used outside the manual-only switch: {l.strip()}")

    def test_no_event_fields_interpolated_into_shell(self):
        # Untrusted workflow_run / event fields must reach bash through env:, never
        # through ${{ }} inside a run: script (shell injection via branch names).
        in_run = False
        run_indent = 0
        for n, line in enumerate(self.workflow_content.splitlines(), 1):
            indent = len(line) - len(line.lstrip())
            if in_run and line.strip() and indent <= run_indent:
                in_run = False
            if line.strip().startswith("run: |") or line.strip().startswith("run: >"):
                in_run = True
                run_indent = indent
                continue
            if in_run:
                self.assertNotIn("${{", line, f"expression interpolated into a run script at line {n}: {line.strip()}")
        self.assertIn("WR_BRANCH: ${{ github.event.workflow_run.head_branch }}", self.workflow_content)
        self.assertIn('[ "$WR_BRANCH" = "main" ] || skip', self.workflow_content)
        self.assertIn('[ "$WR_CONCLUSION" = "success" ] || skip', self.workflow_content)

    def test_gap_issue_lookup_not_truncated(self):
        self.assertIn("gh issue list --label ledger-gap --state open --limit 1000", self.workflow_content)

if __name__ == '__main__':
    unittest.main()
