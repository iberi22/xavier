import subprocess
import tempfile
import os
import unittest
from textwrap import dedent

class TestLedgerTagWorkflowDecision(unittest.TestCase):
    def test_decision_logic(self):
        script = dedent("""\
        #!/bin/bash
        AUTHOR=$1
        FILES_TOUCHED=$2
        HAS_CI=$3
        HAS_E2E=$4

        # mock git log
        git() {
            if [[ "$1" == "log" ]]; then echo "$AUTHOR";
            elif [[ "$1" == "diff" ]]; then echo -e "$FILES_TOUCHED";
            elif [[ "$1" == "rev-parse" ]]; then echo "abc";
            fi
        }

        # mock gh run list
        gh() {
            if [[ "$1" == "run" && "$3" == "ci.yml" ]]; then echo "$HAS_CI";
            elif [[ "$1" == "run" && "$3" == "e2e-segmented.yml" ]]; then echo "$HAS_E2E";
            elif [[ "$1" == "workflow" ]]; then echo "dispatch $3";
            fi
        }

        if [[ "$AUTHOR" == *"github-actions[bot]"* ]]; then
            NON_LEDGER=0
            while IFS= read -r file; do
              if [[ -z "$file" ]]; then continue; fi
              if [[ "$file" != ".gitcore/features.json" && "$file" != "docs/FEATURE_STATUS.md" ]]; then
                NON_LEDGER=1
                break
              fi
            done <<< "$(git diff HEAD^ HEAD)"

            if [[ $NON_LEDGER -eq 0 ]]; then
              SHA=$(git rev-parse HEAD)

              if [[ "$HAS_CI" == "0" ]]; then
                gh workflow run ci.yml --ref main
              fi

              if [[ "$HAS_E2E" == "0" ]]; then
                gh workflow run e2e-segmented.yml --ref main
              fi
            fi
        fi
        """)
        with tempfile.NamedTemporaryFile('w', delete=False) as f:
            f.write(script)
            path = f.name

        os.chmod(path, 0o755)

        # 1. bot commit without runs -> dispatch
        res = subprocess.check_output(['bash', path, "github-actions[bot]", ".gitcore/features.json", "0", "0"], text=True)
        self.assertIn("dispatch ci.yml", res)
        self.assertIn("dispatch e2e-segmented.yml", res)

        # 2. bot commit with existing runs -> no dispatch
        res = subprocess.check_output(['bash', path, "github-actions[bot]", ".gitcore/features.json", "1", "1"], text=True)
        self.assertNotIn("dispatch ci.yml", res)

        # 3. human commit -> no dispatch
        res = subprocess.check_output(['bash', path, "human", ".gitcore/features.json", "0", "0"], text=True)
        self.assertEqual(res.strip(), "")

        # 4. bot commit touching other files -> no dispatch
        res = subprocess.check_output(['bash', path, "github-actions[bot]", ".gitcore/features.json\\nsrc/other.py", "0", "0"], text=True)
        self.assertEqual(res.strip(), "")

        os.remove(path)

if __name__ == '__main__':
    unittest.main()
