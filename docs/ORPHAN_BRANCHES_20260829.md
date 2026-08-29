# Merged & Orphan Remote Branches Cleanup (2026-08-29)

The following remote branches on `origin` correspond to merged PRs or abandoned task branches that should be purged post-merge.

## Branches to Delete

- `docs/changelog-v0.14.0-320543887835049903`
- `docs/update-readme-public-release-16407591286045378163`
- `feat-issue-context-packager-stable-17571648071610165333`
- `feat-store-path-hierarchy-tests-10565640631588122383`
- `feat/supabase-unification-2026-08-24`
- `fix-centralize-sqlite-pragmas-9451278719973610105`
- `fix/empty-memory-symbol-links-bloat-14861935359604781725`
- `jules-10750920045185825449-3b17be92`
- `jules-11174658088277341807-e55b3c71`
- `jules-16418130804909919494-d909a574`
- `jules-3534589577834093985-d9697f2c`
- `jules-494945635976515320-ac591c90`
- `refactor-doctor-subsystem-checks-3787558386034701749`
- `refactor/decompose-sqlite-vec-store-put-5832788325316412402`
- `refactor/unify-memory-search-query-engine-8310044969747454011`
- `sentinel/fix-insecure-random-13433511878432449804`
- `sentinel/fix-insecure-webauthn-device-key-10502038064197515721`

## Manual Deletion Command

Run the following command after merging PR #1614:

```bash
git push origin --delete \
  docs/changelog-v0.14.0-320543887835049903 \
  docs/update-readme-public-release-16407591286045378163 \
  feat-issue-context-packager-stable-17571648071610165333 \
  feat-store-path-hierarchy-tests-10565640631588122383 \
  feat/supabase-unification-2026-08-24 \
  fix-centralize-sqlite-pragmas-9451278719973610105 \
  fix/empty-memory-symbol-links-bloat-14861935359604781725 \
  jules-10750920045185825449-3b17be92 \
  jules-11174658088277341807-e55b3c71 \
  jules-16418130804909919494-d909a574 \
  jules-3534589577834093985-d9697f2c \
  jules-494945635976515320-ac591c90 \
  refactor-doctor-subsystem-checks-3787558386034701749 \
  refactor/decompose-sqlite-vec-store-put-5832788325316412402 \
  refactor/unify-memory-search-query-engine-8310044969747454011 \
  sentinel/fix-insecure-random-13433511878432449804 \
  sentinel/fix-insecure-webauthn-device-key-10502038064197515721
```
