# Agent instructions

For repositories instrumented with semlex, so coding agents (e.g. Claude
Code) instrument correctly and reviewers catch what compilers can't.

| File | Use |
|---|---|
| [CLAUDE-section.md](CLAUDE-section.md) | Paste into your repository's `CLAUDE.md` (or equivalent agent instructions). Replace `crate::` and the paths with your registry crate and local registry. |
| [skills/semlex-instrument/](skills/semlex-instrument/SKILL.md) | Skill: add or change instrumentation with semlex, end to end. |
| [skills/semlex-instrumentation-review/](skills/semlex-instrumentation-review/SKILL.md) | Skill: review a change for *missing* telemetry and escape hatches. |

To install the skills for Claude Code, copy the `skills/` directories into
your repository's `.claude/skills/`.

## Weaver's MCP server

The skills look signals up through Weaver's MCP server. For Claude Code, add
it to your repository's `.mcp.json` (adjust the registry path):

```json
{
  "mcpServers": {
    "weaver": {
      "command": "weaver",
      "args": ["registry", "mcp", "--v2", "-r", "registry"]
    }
  }
}
```

Without it, agents can read the local registry's `resolved.yaml` directly.
