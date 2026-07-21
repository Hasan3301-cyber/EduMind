# Local Agent Sandbox

EduMind’s installed desktop app includes a local agent registry and a bounded
sandbox for each configured profile. It gives a learner control over agent
identity, instructions, model overrides, read-only study context, and four
local policy documents without exposing the gateway or filesystem to a browser.

## Use it

1. Open **Administration** in the installed EduMind desktop app.
2. Create or edit a profile, choose only the read-only context it needs, then
   save the registry.
3. Edit that profile’s `AGENTS.md`, `SOUL.md`, `IDENTITY.md`, and `USER.md`
   from the same panel.
4. Select an active non-Master profile in **Chat** for a focused conversation.
   Automatic study workspaces remain coordinated by the Master Agent.

## Safety model

- The Master Agent always remains present and is the default coordinator.
- Desktop-managed profiles cannot create subagent trees and cannot gain write,
  shell, scheduling, messaging, or external-submission tools.
- Every workspace is fixed under the app’s private `Sandbox/agents/<agent-id>`
  directory. The runtime reads only the four named control files, verifies they
  remain inside that sandbox, and caps each file at 32 KiB.
- Local policy files may shape a role or learner preference, but they cannot
  override tool policy, consent requirements, or EduMind’s runtime safety
  boundary.
- The editor refuses likely credentials and private keys. Configure provider
  keys only through the Administration keychain-backed provider form.
- Removing a profile unregisters it; it deliberately preserves its local
  sandbox files rather than silently deleting learner-authored policy.

Generated study artifacts still belong beneath `EDUMIND_OUTPUT_DIR` (default
`OUTPUT`), not in the sandbox control-file directory.
