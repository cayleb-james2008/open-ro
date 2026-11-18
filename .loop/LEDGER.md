# Ledger — OpenRO scale-up (append-only)

| row | first failing check + message | commit | outcome | cause / note |
|---|---|---|---|---|
| setup | baseline run: `verify[baseline]: build ok; tests ok; 4 passed; 0 failed` | 10dfece | kept | starting point recorded before any change |
| B1 | new test skill_registry_generates_distinct_parameterised_luau did not compile (no registry/generate/system_prompt) | (B1-B5 commit) | kept | registry in skills.rs is the single source for prompt + dispatch; 4 Rune skills: round_manager, leaderboard, day_night, kill_brick |
| B2 | new test one_ask_writes_two_scripts_from_two_skills (generate-both-then-write-both order) | (same commit) | kept | generated sources kept per skill name; write_script resolves by args.skill / use_generated / source |
| B3 | new tests instances_round_trip_and_revert + playtest_failure_repaired_by_creating_the_missing_instance | (same commit) | kept | mock tree carries instances; create/delete_instance bridge tools; checkpoints + revert cover instances; mock playtest gained the documented SpawnPad rule |
| B4 | new test missing_bridge_tool_refuses_honestly | (same commit) | kept | McpClient discovers tools/list at connect; run_ask refuses naming the missing tool; mock_studio_min fixture proves it |
| B5 | new tests repair_loop_survives_a_repeated_identical_attempt + repair_loop_gives_up_honestly_when_the_model_never_fixes | (same commit) | kept | repair attempts counted in the transcript; byte-identical rewrite called out to the model with the console text; after 3 failed playtests the ask fails honestly naming the console output, change set stays revertable |

- B6+B7+B8 (one server rewrite, committed together): chats persist (chats.json, atomic), multiple chats + select, OPENRO_DATA_DIR in main; shell: light+dark (dark default, localStorage), Settings/Appearance toggle, chat list, verdict-styled thread lines, labelled composer, busy Send state. Verify openro-phase1 green 13/13 (was 11/11).
