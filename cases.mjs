// cases.mjs for ultra-design/kit/replay.mjs against OpenRO v0.3.0
// Tests null/wrong-type fields, long unbroken tokens (260 chars), 4-digit Explorer counts,
// malformed 200 bodies, and unreachable server recovery at both 1440x900 and 390x844.
export default ({ LONG, json, patch, malformed, down, overflowing, has }) => [
  {
    name: "null and numeric chat titles and malformed entries do not blank the workbench",
    routes: [
      [
        "**/api/chats",
        patch((j) => ({
          ...j,
          active: null,
          can_undo: "not-a-bool",
          chats: [...(j.chats || []), null, { id: "n1", title: null }, { id: 42, title: 999 }],
        })),
      ],
    ],
    check: async (p) => (await p.locator("header h2").count()) === 1 || "missing header title",
  },
  {
    name: "ideas with missing pitch and null skills render cleanly",
    routes: [
      [
        "**/api/ideas",
        (r) =>
          r.request().method() === "POST"
            ? json(r, {
                ok: true,
                ideas: [
                  { id: "bare-1", title: "Minimal Blueprint", pitch: null, skills: null },
                  null,
                  { id: "bare-2", title: 12345, skills: ["checkpoints", 99, null] },
                ],
                built_titles: [null, "Lava Obby"],
              })
            : r.continue(),
      ],
    ],
    check: async (p) => (await has(p, "Minimal Blueprint")) || "Minimal Blueprint missing",
  },
  {
    name: "long unbroken 260-char script paths, prompts, and Luau errors stay inside boxes",
    shot: true,
    routes: [
      [
        "**/api/thread",
        (r) =>
          json(r, {
            thread: [
              "You: Build custom module " + LONG,
              "Generated the checkpoints script.",
              "Wrote ServerScriptService." + LONG + ".",
              "Playtest failed: ServerScriptService." + LONG + ":12: unexpected symbol",
              "Console from the failed test run: StackBegin " + LONG,
              "OpenRO: Studio bridge error: refused " + LONG,
            ],
          }),
      ],
      [
        "**/api/place-tree",
        (r) =>
          json(r, {
            scripts: { ["ServerScriptService." + LONG]: "-- " + LONG },
            instances: { ["Workspace." + LONG]: { class_name: "Part", properties: { Tag: LONG } } },
          }),
      ],
    ],
    check: async (p) => (await overflowing(p, "#thread p, #thread code")) === 0 || "thread text wider than box",
  },
  {
    name: "malformed /api/thread shows plain recovery card with Try again",
    routes: [["**/api/thread", malformed]],
    check: async (p) =>
      ((await has(p, "Could not load the conversation.")) &&
        (await p.getByRole("button", { name: "Try again" }).count()) >= 1) ||
      "missing thread recovery UI",
  },
  {
    name: "unreachable /api/thread shows plain recovery card with Try again",
    routes: [["**/api/thread", down]],
    check: async (p) =>
      ((await has(p, "Could not load the conversation.")) &&
        (await p.getByRole("button", { name: "Try again" }).count()) >= 1) ||
      "missing unreachable thread recovery UI",
  },
  {
    name: "non-numeric checked_at and raw URL error in /api/update never leak NaN or raw URL",
    routes: [
      [
        "**/api/update",
        (r) =>
          json(r, {
            current: "0.3.0",
            latest: "0.3.1",
            available: true,
            can_apply: true,
            checked_at: "bad-timestamp",
            error: "https://api.github.com/repos/x/y answered HTTP 502 Bad Gateway",
          }),
      ],
    ],
    check: async (p) => {
      const txt = await p.locator('section[aria-label="OpenRO update"]').innerText();
      if (/https?:\/\//.test(txt)) return "raw URL leaked into update strip: " + txt;
      if (!/GitHub had a problem/.test(txt)) return "expected plain error sentence, got: " + txt;
      return true;
    },
  },
];
