# Backlog — OpenRO scale-up

| id | what is wrong | how to reproduce | severity | status |
|---|---|---|---|---|
| B1 | Exactly one skill (round_manager) and the system prompt is a hard-coded string naming only that scenario (breaks C2) | read src/agent.rs SYSTEM_PROMPT and src/skills.rs | high | open |
| B2 | `last_generated` holds one source: a second skill generation overwrites the first before it is written, so multi-script asks write the wrong content (breaks C4) | read src/agent.rs run_skill/write_script; run an ask that generates two skills then writes both | high | open |
| B3 | The place tree is scripts-only: the bridge has no instance create/delete, so most of a game (parts, spawns, folders) cannot be built at all (breaks C3) | read src/bin/mock_studio.rs tool list and src/agent.rs tools | high | open |
| B4 | No tools/list discovery: the agent assumes the baseline tool set exists on whatever server OPENRO_MCP_COMMAND names (breaks C6) | read src/mcp.rs: initialize is the only non-tool request | medium | open |
| B5 | Repair loop has no attempt accounting and no repeat detection: a model repeating the same failing step burns all 16 steps and fails with an opaque "did not finish" (breaks C5) | read src/agent.rs run_ask/playtest arm | medium | open |
| B6 | Chats and thread lines live in server memory only; there is one anonymous chat; a restart loses everything (breaks C7) | read src/server.rs Shared: thread is a Vec in memory; /api/new-chat just clears it | medium | open |
| B7 | No themes and no settings: the page is dark-only with hard-coded colours (breaks C9) | read src/server.rs render_page | medium | open |
| B8 | UI craft gaps: composer input has no label, no :focus-visible styling, Send never disables while the agent works, thread lines are unstyled by role, no empty state | read src/server.rs render_page | low | open |
| B9 | Replacement staging does not exist (breaks C12) | ls ~/workspace/restart-20261117/replacement/openro/ fails | medium | open |

## Leads

- McpClient::spawn splits OPENRO_MCP_COMMAND on whitespace — a command with quoted paths would break; check whether the real Studio server command needs quoting.
- Agent's change set (last_changes) is global, not per chat — revisit when chats land (B6).
