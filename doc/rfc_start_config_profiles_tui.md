# RFC: `start` onboarding, profile-based config, and richer TUI

Status: Draft (awaiting approval before implementation)
Author: Rai engineering
Date: 2026-02-28

## 1) Background

Current behavior:
- `rai config` is a single linear flow (provider -> api key -> default model).
- Runtime config is effectively single-profile.
- Prompt UX uses `dialoguer`, which is functional but visually basic.

Requested outcomes:
1. Add `rai start` for first-time setup.
2. Redesign `rai config` into a "what do you want to configure?" menu.
3. Add profile support (`profile` command + global `--profile` flag).
4. Make terminal UI significantly better-looking (Bubble Tea style quality target).

This document proposes UX, config schema, command surface, migration, and phased implementation.

---

## 2) Goals and non-goals

## Goals
- Make first-run setup fast and obvious (`rai start`).
- Support multiple named working setups (profiles) for different workloads/models/providers.
- Keep backward compatibility with existing config files.
- Improve UX quality without breaking CI/non-interactive behavior.

## Non-goals (initial implementation)
- Implementing additional providers beyond currently supported provider runtime.
- Full plugin marketplace / remote profile sync.
- Breaking changes to existing `rai run` / task execution semantics.

---

## 3) Proposed command UX

## 3.1 `rai start` (new)

Purpose: opinionated onboarding wizard for first setup (or quick reset/setup of a profile).

### Interactive flow (proposed)
1. Welcome + quick explanation.
2. Profile choice:
   - Create/select profile (default suggestion: `default`).
3. Provider selection.
4. API key setup (keyring preferred).
5. Model selection:
   - Popular presets + custom entry.
6. Tool behavior selection:
   - `ask` / `ask_once` / `allow` / `deny` presets.
7. Confirm summary.
8. Save and print "next commands".

### Example
- `rai start`
- `rai start --profile hard-task`

If profile exists and user targets it, prompt for overwrite/edit.

---

## 3.2 `rai config` redesign

Purpose: central configuration hub (not only provider selection).

`rai config` opens a menu:
- Provider & API key
- Model defaults
- Tool behavior / permissions
- Profiles (manage/switch/default)
- Billing/preferences (future extension point)
- Exit

### Subcommands for scriptability (optional but recommended)
- `rai config show [--profile NAME]`
- `rai config edit [section] [--profile NAME]`

CI mode remains non-interactive; interactive UI is disabled as today.

---

## 3.3 `rai profile` (new)

Purpose: explicit profile lifecycle management.

Proposed subcommands:
- `rai profile list`
- `rai profile show [NAME]`
- `rai profile create NAME [--copy-from NAME]`
- `rai profile delete NAME`
- `rai profile rename OLD NEW`
- `rai profile switch NAME` (sets active profile)
- `rai profile default NAME` (sets default profile)

Global flag:
- `--profile <NAME>` available on all execution commands (`run`, `plan`, shorthand task mode, etc.).

Resolution order:
1. CLI `--profile`
2. `RAI_PROFILE` env var (new)
3. `active_profile` in config
4. `default_profile` in config

If none resolves, fail with actionable guidance (`rai start`).

---

## 4) Config model proposal (profiles)

## 4.1 New schema (v2)

```toml
config_version = 2
default_profile = "default"
active_profile = "default"

[profiles.default]
providers = ["poe"]
default_provider = "poe"
default_model = "gpt-4o"
tool_mode = "ask"      # ask | ask_once | allow | deny
no_tools = false
auto_approve = false
```

Profile fields are extensible for future settings.

## 4.2 Backward compatibility

Legacy fields currently used:
- `provider`
- `providers`
- `default_provider`
- `default_model`

Migration plan:
1. On load, detect absence of `profiles`.
2. Create `profiles.default` using current effective values.
3. Set `default_profile = "default"` and `active_profile = "default"`.
4. Preserve legacy read support for a transition window.
5. Write back v2 shape on save.

## 4.3 API key scoping with profiles

Recommended keyring account format:
- `<profile>:<provider>`

Lookup order:
1. `RAI_API_KEY`
2. provider-specific env vars
3. keyring `<profile>:<provider>`
4. keyring legacy `<provider>` (compat fallback)

---

## 5) Detailed behavior changes

## 5.1 Execution commands

Commands that resolve profile:
- shorthand `rai <task...>`
- `rai run ...`
- `rai plan ...` (for preview defaults)
- future interactive commands.

Model precedence:
1. CLI `--model`
2. task frontmatter model
3. profile `default_model`

Provider precedence:
1. explicit runtime override (future)
2. profile resolved provider selection

## 5.2 First-run handling

If no usable config/profile exists:
- `rai start` gives best UX.
- Other commands should fail with:
  - clear error
  - one-line fix: `Run 'rai start'`.

---

## 6) TUI modernization plan (item 4)

Bubble Tea is Go-native; for Rust, recommended stack is:
- `ratatui` + `crossterm` + optional `tui-input`.

## 6.1 UX target
- Full-screen panels with:
  - title/header
  - step indicator
  - key hints (`↑/↓`, `Enter`, `Esc`)
  - highlighted selections
  - consistent color theme
- Shared component system for `start`, `config`, and `profile` flows.

## 6.2 Architecture (proposed)
- New module `src/tui/`:
  - `app.rs` (state machine)
  - `theme.rs` (colors/styles)
  - `screens/` (provider, model, tools, profile)
  - `widgets/` (menu list, footer hints, summary cards)
- Graceful fallback:
  - non-TTY/CI -> existing non-interactive errors (unchanged).

## 6.3 Scope split
- Phase A: improve onboarding/config/profile flows first.
- Phase B: extend style system across other interactive flows (`create`, `plan` interactive path).

---

## 7) Implementation phases

## Phase 1: data model + profile resolution
- Add profile schema structs and migration.
- Add profile resolver + `--profile` + `RAI_PROFILE`.
- Maintain compatibility.

## Phase 2: command surface
- Add `start` command.
- Redesign `config` menu.
- Add `profile` command group.

## Phase 3: TUI upgrade
- Introduce `ratatui` stack.
- Replace `dialoguer`-only menus in `start/config/profile`.
- Keep fallback behavior for CI/non-interactive mode.

---

## 8) Acceptance criteria

1. `rai start` configures a new user in <60s for default profile.
2. `rai config` allows selecting which area to configure.
3. `rai profile` supports create/list/switch/default/delete.
4. `--profile` reliably overrides active/default profile for run/plan.
5. Existing users with legacy config continue working after migration.
6. Interactive screens are visibly richer and consistent.

---

## 9) Test plan (for implementation phase)

- Unit tests:
  - config migration old->v2
  - profile resolution precedence
  - keyring lookup precedence with profile/legacy fallback
- Integration tests:
  - `--profile` on `run` and shorthand mode
  - `profile switch/default` behavior
  - `start` non-interactive guard in CI
- Manual TUI tests:
  - keyboard navigation, cancel path, save path, redraw correctness.

---

## 10) Open decisions for approval

1. Should `rai start` auto-run when user executes `rai` without config (or just show guidance)?
2. Should new profile creation default to cloning current profile settings?
3. Should deleting active/default profile be blocked unless reassigned first? (recommended: yes)
4. Should profile-specific tool policy be enforced at run time now, or staged in phase 2?
5. TUI library decision: approve `ratatui + crossterm` as the Bubble Tea-style Rust equivalent?

