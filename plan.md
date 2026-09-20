1. Edit `panel-ui/src/components/NotificationCenter.tsx` to wrap `markRead` and `dismissNotification` in `useCallback` with empty dependency arrays, and add the Bolt performance optimization documentation.
2. Run `cd panel-ui && pnpm lint` and `pnpm test` to verify the changes.
3. Complete pre commit steps to ensure proper testing, verification, review, and reflection are done.
4. Create a PR using the submit tool with standard formatting.
