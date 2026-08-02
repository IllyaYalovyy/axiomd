# Testing Strategy

This project values tests that protect user behavior and system invariants over
raw coverage percentages.

## Test Layers

Use the lowest layer that catches the risk clearly:

- **Unit tests** - pure functions, model invariants, parsing, validation,
  reducers, state transitions
- **Integration tests** - storage, API clients, process boundaries, migrations,
  serialization compatibility, concurrency contracts
- **UI / behavioral tests** - user flows, keyboard and pointer behavior,
  accessibility-visible state, layout regressions
- **Property / fuzz tests** - parsers, protocols, tree structures, state
  machines, and untrusted input
- **Manual checks** - hardware, external providers, stores, or other cases where
  automation is not practical

## Regression Rule

Every fixed bug should leave behind a test that fails without the fix.

Every new user-facing behavior should have a test that would fail if the behavior
disappeared.

## Risk Matrix

Maintain a project-specific matrix as the architecture settles:

| Risk / failure mode | User impact | Test layer | Coverage |
|---|---|---|---|
| Example: invalid persisted state | App cannot start | Integration | TODO |

## Local Quality Gate

Run:

```bash
./scripts/quality.sh
```

Add project-specific checks as executable files in `scripts/quality.d/`.

## Test Naming

Prefer names that describe the requirement:

```text
restores_workspace_after_process_restart
rejects_expired_token_without_overwriting_refresh_token
keeps_keyboard_focus_after_item_delete
```

Avoid names that only describe the implementation:

```text
test_update
manager_returns_true
component_renders
```
