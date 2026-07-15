# Xavier Project - Fix auth2

Apply ALL fixes from CLAUDECODE_TASK.md in order:

1. Fix Cargo.toml - add `hkdf = "0.13"` to dependencies
2. Implement missing endpoints in `src/auth2/mod.rs` (2fa/setup, 2fa/verify, /recovery)
3. Register new routes in auth_routes()
4. Implement real TOTP verification in login_handler
5. Fix register_handler to return seed_phrase
6. Add auth step to OnboardingFlow.tsx
7. Clean up unused imports warnings

Then verify: `cargo check` and `cd panel-ui && npx vite build`
Fix ALL errors until both pass.
