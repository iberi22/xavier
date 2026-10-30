1. **Raise timeout of `ledger-verification` in `.github/workflows/ci.yml`**:
   - Locate the `ledger-verification:` job.
   - Update `timeout-minutes: 45` to `timeout-minutes: 90`.
   - Update the step `Run Full Ledger` to have `timeout-minutes: 80` so that the `Upload Logs` artifact step can always execute on timeout.
2. **Add elapsed seconds output to `scripts/verify-pipeline.sh`**:
   - In `scripts/verify-pipeline.sh`, when it logs the start of each stage `==> [X/5] ...`, we need to change it to include the elapsed time. Since bash sets `$SECONDS` from the start of the script, we can include it at the start or end of each step. The requirements state: "print elapsed seconds at the end of each stage ([1/5] ... [4b/5]) so future slowdowns are visible in ledger.log. No behaviour change otherwise."
   - We will print at the end of each section: `echo "==> [1/5] Validating ledger schema... (done in $SECONDS s)"` is probably not what it means by "at the end of each stage", wait, the requirement: `print elapsed seconds at the end of each stage ([1/5] ... [4b/5])`. It's better to add an echo at the end of the block.
   - Wait, `echo "==> [1/5] ... complete (${SECONDS}s)"` at the end of the block? Or just `echo "  ... stage 1 complete (${SECONDS}s)"`? Let's check how the script is structured.

Let's modify `verify-pipeline.sh` first to see where these can be placed.
