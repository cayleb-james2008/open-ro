--!strict
-- Headless Lune Economy Simulator for Untitled Anime Card Game (UACG)
-- Verifies:
--   1. Probability table integrity (9 rarity tiers, 5 mutations)
--   2. First Legendary < 15 minutes (F2P cohort Monte Carlo)
--   3. First Rebirth (25,000 Gems) within 2-4 hours (F2P vs Paid)
--   4. Robux value-per-product metrics

local RNG_MOD = 2147483647
local RNG_MUL = 48271
local rngState = 133742069

local function nextFloat(): number
	rngState = (rngState * RNG_MUL) % RNG_MOD
	return rngState / RNG_MOD
end

type RaritySpec = {
	name: string,
	oneInN: number,
	cap: number,
	softPity: number?,
	softStep: number?,
	hardPity: number?,
}

local RARITIES_TOP_DOWN: { RaritySpec } = {
	{ name = "Nexus", oneInN = 25000000, cap = 0.05 },
	{ name = "Universal", oneInN = 2500000, cap = 0.10 },
	{ name = "Secret", oneInN = 250000, cap = 0.15 },
	{ name = "Mythic", oneInN = 25000, cap = 0.25, softPity = 8000, softStep = 0.00014, hardPity = 15000 },
	{ name = "Legendary", oneInN = 1000, cap = 0.40, softPity = 450, softStep = 0.0030, hardPity = 750 },
	{ name = "Epic", oneInN = 100, cap = 0.50, hardPity = 100 },
	{ name = "Rare", oneInN = 20, cap = 0.60 },
	{ name = "Uncommon", oneInN = 5, cap = 0.70 },
}

local MUTATIONS = {
	{ name = "Void", weight = 0.001, mult = 7.5 },
	{ name = "Rainbow", weight = 0.006, mult = 4.0 },
	{ name = "Golden", weight = 0.018, mult = 2.25 },
	{ name = "Shiny", weight = 0.055, mult = 1.5 },
	{ name = "Normal", weight = 0.920, mult = 1.0 },
}

local function validateTables()
	local baseSum = 0
	for _, r in ipairs(RARITIES_TOP_DOWN) do
		baseSum += (1 / r.oneInN)
	end
	assert(baseSum > 0.25 and baseSum < 0.30, "Unexpected non-Common rarity probability mass")

	local mutSum = 0
	for _, m in ipairs(MUTATIONS) do
		mutSum += m.weight
	end
	assert(math.abs(mutSum - 1.0) < 1e-6, "Mutation weights must sum to 1.0")
end

local function computeEffectiveLuck(
	flatLuck: number,
	passMult: number,
	platformMult: number,
	potionMult: number,
	weatherMult: number,
	rebirths: number,
	milestoneMult: number,
	ticketMult: number
): number
	local rebirthMult = 1.0 + 0.15 * math.min(rebirths, 250)
	return (1.0 + flatLuck)
		* passMult
		* platformMult
		* potionMult
		* weatherMult
		* rebirthMult
		* milestoneMult
		* ticketMult
end

type PityState = {
	epic: number,
	legendary: number,
	mythic: number,
}

local function rollOnce(effectiveLuck: number, pity: PityState): string
	pity.epic += 1
	pity.legendary += 1
	pity.mythic += 1

	-- Hard pity checks highest tier first
	if pity.mythic >= 15000 then
		pity.mythic = 0
		pity.legendary = 0
		pity.epic = 0
		return "Mythic"
	elseif pity.legendary >= 750 then
		pity.legendary = 0
		pity.epic = 0
		return "Legendary"
	elseif pity.epic >= 100 then
		pity.epic = 0
		return "Epic"
	end

	local u = nextFloat()
	local cum = 0.0
	local chosen = "Common"

	for _, r in ipairs(RARITIES_TOP_DOWN) do
		local p = (1 / r.oneInN) * effectiveLuck
		if r.name == "Mythic" and r.softPity and r.softStep and pity.mythic >= r.softPity then
			p += (pity.mythic - r.softPity + 1) * r.softStep
		elseif r.name == "Legendary" and r.softPity and r.softStep and pity.legendary >= r.softPity then
			p += (pity.legendary - r.softPity + 1) * r.softStep
		end
		p = math.min(r.cap, p)
		local avail = math.max(0.0, 1.0 - cum)
		local slice = math.min(avail, p)
		cum += slice
		if u < cum then
			chosen = r.name
			break
		end
	end

	if chosen == "Nexus" or chosen == "Universal" or chosen == "Secret" or chosen == "Mythic" then
		pity.mythic = 0
		pity.legendary = 0
		pity.epic = 0
	elseif chosen == "Legendary" then
		pity.legendary = 0
		pity.epic = 0
	elseif chosen == "Epic" then
		pity.epic = 0
	end

	return chosen
end

local function percentile(sorted: { number }, p: number): number
	local idx = math.max(1, math.min(#sorted, math.ceil(#sorted * p)))
	return sorted[idx]
end

-- 1. Simulate 500 F2P players over their first 20 minutes to measure First Legendary timing
local function runFirstLegendarySim(trials: number, isPaid: boolean): (number, number, number, number)
	local timesMin: { number } = {}
	local sumMin = 0.0
	local hitsUnder15 = 0

	local rollCooldown = if isPaid then 0.6 else 1.6
	local passMult = if isPaid then 2.0 else 1.0

	for _ = 1, trials do
		local pity: PityState = { epic = 0, legendary = 0, mythic = 0 }
		local tSec = 0.0
		local rollCount = 0
		local foundAtMin: number? = nil

		-- Phase A (0..120s): Join + Group (+1.0 flat) + Tutorial (15 Tickets) + Code (10 Tickets)
		tSec = 60.0
		local flatLuck = 1.0 -- Group join
		local noviceMult = 1.5 -- 20m Novice Blessing
		local potionMult = 1.5 -- Tutorial Minor Luck Potion
		for _ = 1, 25 do
			tSec += 0.12 -- 5-pack Ticket pull cadence
			rollCount += 1
			local luck = computeEffectiveLuck(flatLuck, passMult, 1.0, potionMult, 1.0, 0, 1.0, 2.0) * noviceMult
			local tier = rollOnce(luck, pity)
			if
				(tier == "Legendary" or tier == "Mythic" or tier == "Secret" or tier == "Universal" or tier == "Nexus")
				and not foundAtMin
			then
				foundAtMin = tSec / 60.0
				break
			end
		end

		-- Phase B (2..15 min): Alternate short World 1 Acts (60s battle) & rolling batches
		-- Grants 60 Roll Tickets + ~36,000 Yen across Acts 1-4, Playtime 5m/10m, Daily Quest 1, and dupe quick-sells.
		local queuedTickets = 60
		local queuedYen = 36000
		local nextActAt = 120.0
		local battleEndAt = 0.0

		while not foundAtMin and tSec < 1200.0 do
			if tSec >= nextActAt and nextActAt <= 360.0 then
				battleEndAt = tSec + 45.0
				tSec = battleEndAt
				nextActAt += 110.0
				if tSec >= 300.0 then
					potionMult = 2.5 -- 5m/10m Playtime Major Luck Potion
				end
			elseif queuedTickets > 0 then
				queuedTickets -= 1
				tSec += 0.12
				rollCount += 1
				local weather = if math.floor(tSec / 600) == 0 and tSec >= 300 then 2.0 else 1.0
				local luck = computeEffectiveLuck(flatLuck, passMult, 1.0, potionMult, weather, 0, 1.0, 2.0)
					* noviceMult
				local tier = rollOnce(luck, pity)
				if
					tier == "Legendary"
					or tier == "Mythic"
					or tier == "Secret"
					or tier == "Universal"
					or tier == "Nexus"
				then
					foundAtMin = tSec / 60.0
					break
				end
			else
				if queuedYen < 100 then
					-- Quick Infinite/Act farm burst + dupe sell
					tSec += 20.0
					queuedYen += 2500
				end
				queuedYen -= 100
				tSec += rollCooldown
				rollCount += 1
				if tSec >= 300.0 then
					potionMult = 2.5
				end
				local weather = if tSec >= 300 and tSec <= 540 then 2.0 else 1.0
				local luck = computeEffectiveLuck(flatLuck, passMult, 1.0, potionMult, weather, 0, 1.0, 1.0)
					* noviceMult
				local tier = rollOnce(luck, pity)
				if tier == "Common" then
					queuedYen += 25
				elseif tier == "Uncommon" then
					queuedYen += 75
				end
				if
					tier == "Legendary"
					or tier == "Mythic"
					or tier == "Secret"
					or tier == "Universal"
					or tier == "Nexus"
				then
					foundAtMin = tSec / 60.0
					break
				end
			end
		end

		local resolved = foundAtMin or 20.0
		if resolved <= 15.0 then
			hitsUnder15 += 1
		end
		sumMin += resolved
		table.insert(timesMin, resolved)
	end

	table.sort(timesMin)
	return sumMin / trials, percentile(timesMin, 0.50), percentile(timesMin, 0.90), (hitsUnder15 / trials) * 100.0
end

-- 2. Simulate Gem curve to First Rebirth (25,000 Gems) in 5-minute steps
local function runFirstRebirthSim(isPaid: boolean): (number, number)
	-- Paid player represents Starter Bundle (+2,500 Gems) + VIP (+25% battle/quest Gems)
	local gemMult = if isPaid then 1.25 else 1.0
	local gems = if isPaid then 2500 else 0
	local spentSkillTree = 0
	local skillBudget = 1800

	-- Scheduled milestone Gem bursts (minutes -> base gems)
	local milestones: { [number]: number } = {
		[5] = 1500, -- Tutorial + Codes
		[15] = 1200, -- W1 Acts 1-3 + Index 10%
		[25] = 1600, -- W1 Acts 4-5 + Boss + Playtime 25m
		[45] = 2200, -- Tower 1-10 + W2 Early Acts
		[60] = 1800, -- Playtime 60m + Daily Quests batch
		[75] = 2400, -- W2 Clear + Boss
		[95] = 2800, -- Tower 11-20 + W1/W2 Infinite Waves
		[120] = 2600, -- Playtime 120m + Index 30% + Ranked placement
		[145] = 3200, -- Tower 21-30 + Weekly Quest milestone 1
		[170] = 3600, -- W3 Early Acts + Infinite Wave 40 + Ranked wins
		[195] = 3400, -- W3 Acts 4-5 + Index 40%
		[220] = 3600, -- Tower 31-40 + Daily reset / Raid prep
	}

	for min = 5, 240, 5 do
		-- Steady passive Gem trickle (repeat clears, quests, minor milestones): ~115 Gems / 5 min
		gems += math.floor(115 * gemMult)
		if milestones[min] then
			gems += math.floor(milestones[min] * gemMult)
		end
		if spentSkillTree < skillBudget and gems >= 600 then
			local invest = math.min(300, skillBudget - spentSkillTree)
			spentSkillTree += invest
			gems -= invest
		end
		if gems >= 25000 then
			return min, gems
		end
	end

	return 999, gems
end

-- 3. Audit Robux Value-per-Product
local function runProductAudit()
	local gemPacks = {
		{ name = "Pouch of Gems", robux = 99, gems = 1200 },
		{ name = "Chest of Gems", robux = 249, gems = 3500 },
		{ name = "Vault of Gems", robux = 499, gems = 8000 },
		{ name = "Treasury of Gems", robux = 999, gems = 20000 },
	}

	local prevRate = 0.0
	for _, pack in ipairs(gemPacks) do
		local rate = pack.gems / pack.robux
		assert(rate > prevRate, "Bulk Gem packs must strictly improve Gems/Robux")
		prevRate = rate
	end

	-- Starter bundle: 2,500 Gems + 25 Tickets (100 Gems eq/ticket = 2,500) + 2 Major Potions (450 eq each = 900) = 5,900 Gem-eq
	local starterRate = 5900 / 149
	assert(starterRate > prevRate * 1.5, "Starter Bundle must outperform highest bulk Gem pack as onboarding anchor")
	return gemPacks, starterRate
end

-- Execute verification suite
validateTables()

local f2pMean, f2pP50, f2pP90, f2pPct15 = runFirstLegendarySim(500, false)
local paidMean, paidP50, paidP90, paidPct15 = runFirstLegendarySim(500, true)
local f2pRebirthMin, f2pRebirthGems = runFirstRebirthSim(false)
local paidRebirthMin, paidRebirthGems = runFirstRebirthSim(true)
local gemPacks, starterRate = runProductAudit()

print("=== UACG Headless Economy Simulation (Lune) ===")
print(
	string.format(
		"First Legendary (F2P, n=500):  mean=%.2fm | P50=%.2fm | P90=%.2fm | <=15m=%.1f%%",
		f2pMean,
		f2pP50,
		f2pP90,
		f2pPct15
	)
)
print(
	string.format(
		"First Legendary (Paid, n=500): mean=%.2fm | P50=%.2fm | P90=%.2fm | <=15m=%.1f%%",
		paidMean,
		paidP50,
		paidP90,
		paidPct15
	)
)
print(
	string.format(
		"First Rebirth (F2P):           %d min (%.2f h) at %d Gems",
		f2pRebirthMin,
		f2pRebirthMin / 60.0,
		f2pRebirthGems
	)
)
print(
	string.format(
		"First Rebirth (Paid):          %d min (%.2f h) at %d Gems",
		paidRebirthMin,
		paidRebirthMin / 60.0,
		paidRebirthGems
	)
)
print("--- Robux Value Anchor ---")
for _, p in ipairs(gemPacks) do
	print(string.format("  %-18s %4d R$ -> %6d Gems (%.2f Gems/R$)", p.name, p.robux, p.gems, p.gems / p.robux))
end
print(string.format("  %-18s %4d R$ -> %6d Gem-eq (%.2f Gem-eq/R$)", "Starter Bundle", 149, 5900, starterRate))

-- Hard milestone assertions
assert(f2pP50 < 15.0, "FAIL: F2P median time to First Legendary must be < 15 min")
assert(f2pP90 <= 15.0, "FAIL: F2P P90 time to First Legendary must be <= 15 min")
assert(paidP50 < f2pP50, "FAIL: Paid accelerator must beat F2P median time")
assert(f2pRebirthMin >= 120 and f2pRebirthMin <= 240, "FAIL: F2P First Rebirth must be within 2-4 hours (120-240 min)")
assert(
	paidRebirthMin < f2pRebirthMin and paidRebirthMin >= 60,
	"FAIL: Paid First Rebirth must accelerate fairly without sub-hour collapse"
)

print("ALL ECONOMY MILESTONE CHECKS PASSED (exit 0)")
