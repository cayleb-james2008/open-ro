--!strict
local NumberFormat = {}

local SUFFIXES = { "", "K", "M", "B", "T", "Qa", "Qi", "Sx", "Sp", "Oc" }

local function trimZeros(text: string): string
	local trimmed = string.gsub(text, "%.0+$", "")
	trimmed = string.gsub(trimmed, "(%.%d-)0+$", "%1")
	return trimmed
end

function NumberFormat.commas(value: number): string
	local n = math.floor(value + 0.5)
	local sign = ""
	if n < 0 then
		sign = "-"
		n = -n
	end
	local s = tostring(n)
	while true do
		local updated, count = string.gsub(s, "^(%d+)(%d%d%d)", "%1,%2")
		s = updated
		if count == 0 then
			break
		end
	end
	return sign .. s
end

function NumberFormat.compact(value: number): string
	local absVal = math.abs(value)
	if absVal < 1000 then
		return tostring(math.floor(value + 0.5))
	end

	local sign = if value < 0 then "-" else ""
	local tier = 1
	local scaled = absVal
	while scaled >= 1000 and tier < #SUFFIXES do
		scaled /= 1000
		tier += 1
	end

	local formatted: string
	if scaled >= 100 then
		formatted = string.format("%.0f", scaled)
	elseif scaled >= 10 then
		formatted = trimZeros(string.format("%.1f", scaled))
	else
		formatted = trimZeros(string.format("%.2f", scaled))
	end

	return sign .. formatted .. SUFFIXES[tier]
end

function NumberFormat.odds(baseN: number, effectiveLuck: number?): string
	local luck = math.max(1, effectiveLuck or 1)
	local denom = math.max(1, math.floor(baseN / luck + 0.5))
	if denom >= 1000000 then
		return "1-in-" .. NumberFormat.compact(denom)
	end
	return "1-in-" .. NumberFormat.commas(denom)
end

function NumberFormat.multiplier(mult: number): string
	return trimZeros(string.format("%.2f", mult)) .. "x"
end

function NumberFormat.duration(seconds: number): string
	local total = math.max(0, math.floor(seconds + 0.5))
	local hours = math.floor(total / 3600)
	local mins = math.floor((total % 3600) / 60)
	local secs = total % 60
	if hours > 0 then
		return string.format("%dh %02dm", hours, mins)
	elseif mins > 0 then
		return string.format("%dm %02ds", mins, secs)
	end
	return string.format("%ds", secs)
end

return NumberFormat
