--!strict
export type QuestDef = {
	id: string,
	kind: "Daily" | "Weekly",
	title: string,
	stat: string,
	target: number,
	yenReward: number,
	gemReward: number,
	ticketReward: number,
}

export type StreakReward = {
	day: number,
	yen: number,
	gems: number,
	tickets: number,
	label: string,
}

export type PlaytimeMilestone = {
	id: string,
	seconds: number,
	label: string,
	yen: number,
	gems: number,
	tickets: number,
}

local Quests = {}

Quests.Daily = {
	{
		id = "daily_roll_10",
		kind = "Daily",
		title = "Summon 10 Cards",
		stat = "rolls",
		target = 10,
		yenReward = 500,
		gemReward = 60,
		ticketReward = 1,
	},
	{
		id = "daily_battle_5",
		kind = "Daily",
		title = "Win 5 Stage Battles",
		stat = "battles",
		target = 5,
		yenReward = 750,
		gemReward = 80,
		ticketReward = 1,
	},
	{
		id = "daily_boss_1",
		kind = "Daily",
		title = "Defeat 1 World Boss",
		stat = "bosses",
		target = 1,
		yenReward = 1200,
		gemReward = 120,
		ticketReward = 2,
	},
	{
		id = "daily_tower_3",
		kind = "Daily",
		title = "Clear 3 Tower Floors",
		stat = "tower",
		target = 3,
		yenReward = 900,
		gemReward = 100,
		ticketReward = 1,
	},
} :: { QuestDef }

Quests.Weekly = {
	{
		id = "weekly_roll_75",
		kind = "Weekly",
		title = "Summon 75 Cards",
		stat = "rolls",
		target = 75,
		yenReward = 3500,
		gemReward = 350,
		ticketReward = 5,
	},
	{
		id = "weekly_battle_30",
		kind = "Weekly",
		title = "Win 30 Stage Battles",
		stat = "battles",
		target = 30,
		yenReward = 5000,
		gemReward = 450,
		ticketReward = 6,
	},
	{
		id = "weekly_raid_3",
		kind = "Weekly",
		title = "Clear 3 Raids",
		stat = "raids",
		target = 3,
		yenReward = 6000,
		gemReward = 500,
		ticketReward = 8,
	},
	{
		id = "weekly_ranked_5",
		kind = "Weekly",
		title = "Play 5 Ranked Matches",
		stat = "ranked",
		target = 5,
		yenReward = 4000,
		gemReward = 400,
		ticketReward = 5,
	},
} :: { QuestDef }

Quests.LoginStreak = {
	{ day = 1, yen = 300, gems = 50, tickets = 1, label = "Day 1" },
	{ day = 2, yen = 500, gems = 75, tickets = 1, label = "Day 2" },
	{ day = 3, yen = 800, gems = 100, tickets = 2, label = "Day 3" },
	{ day = 4, yen = 1200, gems = 140, tickets = 2, label = "Day 4" },
	{ day = 5, yen = 1800, gems = 180, tickets = 3, label = "Day 5" },
	{ day = 6, yen = 2500, gems = 240, tickets = 4, label = "Day 6" },
	{ day = 7, yen = 5000, gems = 500, tickets = 8, label = "Day 7 Jackpot" },
} :: { StreakReward }

Quests.PlaytimeMilestones = {
	{ id = "pt_5m", seconds = 300, label = "5 Min", yen = 250, gems = 30, tickets = 0 },
	{ id = "pt_15m", seconds = 900, label = "15 Min", yen = 600, gems = 60, tickets = 1 },
	{ id = "pt_30m", seconds = 1800, label = "30 Min", yen = 1200, gems = 120, tickets = 2 },
	{ id = "pt_60m", seconds = 3600, label = "1 Hour", yen = 2500, gems = 250, tickets = 3 },
	{ id = "pt_120m", seconds = 7200, label = "2 Hours", yen = 5000, gems = 450, tickets = 5 },
} :: { PlaytimeMilestone }

Quests.ById = {} :: { [string]: QuestDef }
for _, q in ipairs(Quests.Daily) do
	Quests.ById[q.id] = q
end
for _, q in ipairs(Quests.Weekly) do
	Quests.ById[q.id] = q
end

function Quests.get(id: string): QuestDef?
	return Quests.ById[id]
end

return Quests
