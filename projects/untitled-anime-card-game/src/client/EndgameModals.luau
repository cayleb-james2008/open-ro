--!strict

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type EndgameController = {
	openTower: () -> (),
	openRaid: () -> (),
	openRanked: () -> (),
	openRebirth: () -> (),
	openCharms: () -> (),
	openWorldBoss: () -> (),
	closeAll: () -> (),
	applyTheme: () -> (),
	updateState: (state: { [string]: any }) -> (),
}

local EndgameModals = {}

local function clearChildren(container: Instance)
	for _, child in ipairs(container:GetChildren()) do
		if not child:IsA("UIListLayout") and not child:IsA("UIPadding") then
			child:Destroy()
		end
	end
end

function EndgameModals.build(parent: Instance, api: any, _theme: any): EndgameController
	local palette = Theme.getPalette()
	local cachedState: { [string]: any } = {}
	local activeMode: string? = nil
	local lastData: { [string]: any } = {}

	local backdrop = Instance.new("Frame")
	backdrop.Name = "EndgameModalBackdrop"
	backdrop.Size = UDim2.fromScale(1, 1)
	backdrop.BackgroundColor3 = Color3.fromRGB(8, 10, 16)
	backdrop.BackgroundTransparency = 0.35
	backdrop.BorderSizePixel = 0
	backdrop.Active = true
	backdrop.Visible = false
	backdrop.ZIndex = 30
	backdrop.Parent = parent

	local panel =
		Components.makePanel(backdrop, "EndgamePanel", UDim2.fromScale(0.94, 0.84), UDim2.fromScale(0.5, 0.5), 14)
	panel.AnchorPoint = Vector2.new(0.5, 0.5)
	panel.ZIndex = 31

	local sizeConstraint = Instance.new("UISizeConstraint")
	sizeConstraint.MinSize = Vector2.new(300, 380)
	sizeConstraint.MaxSize = Vector2.new(540, 660)
	sizeConstraint.Parent = panel

	local header = Instance.new("Frame")
	header.Name = "Header"
	header.Size = UDim2.new(1, -24, 0, 48)
	header.Position = UDim2.fromOffset(12, 10)
	header.BackgroundTransparency = 1
	header.ZIndex = 32
	header.Parent = panel

	local titleLabel = Components.makeLabel(
		header,
		"Title",
		"Endgame Mode",
		UDim2.new(1, -96, 1, 0),
		UDim2.fromOffset(0, 0),
		Theme.FONT_DISPLAY,
		18,
		palette.textPrimary,
		Enum.TextXAlignment.Left
	)
	titleLabel.ZIndex = 33

	local closeBtn = Components.makeButton(
		header,
		"CloseBtn",
		"Close",
		UDim2.fromOffset(84, 44),
		UDim2.new(1, -84, 0.5, -22),
		palette.accentRed or Color3.fromRGB(205, 65, 65),
		Color3.fromRGB(255, 255, 255)
	)
	closeBtn.ZIndex = 33
	closeBtn.TextSize = 13

	local statusLabel = Components.makeLabel(
		panel,
		"StatusBanner",
		"Ready",
		UDim2.new(1, -24, 0, 26),
		UDim2.fromOffset(12, 62),
		Theme.FONT_BODY,
		12,
		palette.textSecondary or palette.textPrimary,
		Enum.TextXAlignment.Left
	)
	statusLabel.ZIndex = 32

	local scroll = Instance.new("ScrollingFrame")
	scroll.Name = "BodyScroll"
	scroll.Size = UDim2.new(1, -24, 1, -104)
	scroll.Position = UDim2.fromOffset(12, 92)
	scroll.BackgroundTransparency = 1
	scroll.BorderSizePixel = 0
	scroll.ScrollBarThickness = 5
	scroll.CanvasSize = UDim2.new(0, 0, 0, 0)
	scroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
	scroll.ZIndex = 32
	scroll.Parent = panel

	local listLayout = Instance.new("UIListLayout")
	listLayout.SortOrder = Enum.SortOrder.LayoutOrder
	listLayout.Padding = UDim.new(0, 8)
	listLayout.Parent = scroll

	local pad = Instance.new("UIPadding")
	pad.PaddingBottom = UDim.new(0, 10)
	pad.Parent = scroll

	local function callServer(payload: { [string]: any }): { [string]: any }
		if type(api) == "function" then
			return api(payload)
		elseif type(api) == "table" and type(api.request) == "function" then
			return api.request(payload)
		end
		return { ok = false, error = "No API handler" }
	end

	local function setStatus(msg: string, isError: boolean?)
		local pal = Theme.getPalette()
		statusLabel.Text = msg
		statusLabel.TextColor3 = if isError then (pal.accentRed or Color3.fromRGB(235, 85, 85)) else pal.accentGreen
	end

	local function addInfoCard(order: number, heading: string, detail: string): Frame
		local pal = Theme.getPalette()
		local card = Instance.new("Frame")
		card.Name = "InfoCard_" .. tostring(order)
		card.LayoutOrder = order
		card.Size = UDim2.new(1, -4, 0, 66)
		card.BackgroundColor3 = pal.bgCard or pal.bgPanel
		card.BorderSizePixel = 0
		card.ZIndex = 33
		card.Parent = scroll
		Components.addCorner(card, 10)
		Components.addStroke(card, pal.border, 1, 0.25)

		local h = Components.makeLabel(
			card,
			"Heading",
			heading,
			UDim2.new(1, -16, 0, 22),
			UDim2.fromOffset(8, 6),
			Theme.FONT_DISPLAY,
			14,
			pal.textPrimary,
			Enum.TextXAlignment.Left
		)
		h.ZIndex = 34
		local d = Components.makeLabel(
			card,
			"Detail",
			detail,
			UDim2.new(1, -16, 0, 32),
			UDim2.fromOffset(8, 28),
			Theme.FONT_BODY,
			12,
			pal.textSecondary or pal.textPrimary,
			Enum.TextXAlignment.Left
		)
		d.TextWrapped = true
		d.ZIndex = 34
		return card
	end

	local function addActionRow(
		order: number,
		heading: string,
		detail: string,
		btnText: string,
		btnColor: Color3,
		onClick: () -> ()
	)
		local pal = Theme.getPalette()
		local row = Instance.new("Frame")
		row.Name = "ActionRow_" .. tostring(order)
		row.LayoutOrder = order
		row.Size = UDim2.new(1, -4, 0, 68)
		row.BackgroundColor3 = pal.bgCard or pal.bgPanel
		row.BorderSizePixel = 0
		row.ZIndex = 33
		row.Parent = scroll
		Components.addCorner(row, 10)
		Components.addStroke(row, pal.border, 1, 0.25)

		local h = Components.makeLabel(
			row,
			"Heading",
			heading,
			UDim2.new(1, -132, 0, 22),
			UDim2.fromOffset(10, 6),
			Theme.FONT_DISPLAY,
			13,
			pal.textPrimary,
			Enum.TextXAlignment.Left
		)
		h.ZIndex = 34
		local d = Components.makeLabel(
			row,
			"Detail",
			detail,
			UDim2.new(1, -132, 0, 34),
			UDim2.fromOffset(10, 28),
			Theme.FONT_BODY,
			11,
			pal.textSecondary or pal.textPrimary,
			Enum.TextXAlignment.Left
		)
		d.TextWrapped = true
		d.ZIndex = 34

		local btn = Components.makeButton(
			row,
			"ActionBtn",
			btnText,
			UDim2.fromOffset(112, 44),
			UDim2.new(1, -120, 0.5, -22),
			btnColor,
			Color3.fromRGB(255, 255, 255)
		)
		btn.ZIndex = 34
		btn.TextSize = 12
		btn.Activated:Connect(onClick)
	end

	local renderCurrent: (mode: string, data: { [string]: any }) -> ()

	local function refreshMode(mode: string)
		activeMode = mode
		local actionMap: { [string]: string } = {
			Tower = "GetTowerStatus",
			Raid = "GetRaidStatus",
			WorldBoss = "GetWorldBossStatus",
			Ranked = "GetRankedStatus",
			Rebirth = "GetRebirthStatus",
			Charms = "GetCharmStatus",
		}
		local reqAction = actionMap[mode]
		if not reqAction then
			return
		end
		setStatus("Loading " .. mode .. "...", false)
		local res = callServer({ action = reqAction })
		if res.ok then
			local data = if type(res.data) == "table" then res.data else {}
			lastData = data
			setStatus(mode .. " ready", false)
			renderCurrent(mode, data)
		else
			setStatus(tostring(res.error or "Failed to load"), true)
			renderCurrent(mode, lastData)
		end
	end

	renderCurrent = function(mode: string, data: { [string]: any })
		local pal = Theme.getPalette()
		clearChildren(scroll)
		local order = 1

		if mode == "Tower" then
			titleLabel.Text = "Heavens Arena Tower"
			local clearedFloor = tonumber(data.clearedFloor or data.currentFloor or data.highestFloor) or 0
			local nextFloor = tonumber(data.nextFloor) or (clearedFloor + 1)
			local maxFloor = tonumber(data.maxFloor) or 100
			local nextCfg = if type(data.nextConfig) == "table" then data.nextConfig else nil
			local enemyInfo = "Uses your active 5-card deck."
			if nextCfg then
				local eName = tostring(nextCfg.bossName or nextCfg.enemyName or nextCfg.name or "Guardian")
				local ePow = tonumber(nextCfg.recommendedPower or nextCfg.power)
				enemyInfo = if ePow
					then string.format("Enemy: %s (Rec. Power: %d)", eName, ePow)
					else string.format("Enemy: %s — Uses your active 5-card deck.", eName)
			end
			addInfoCard(
				order,
				string.format("Cleared: Floor %d / %d", clearedFloor, maxFloor),
				"Climb 100 floors for Gems, Tickets, and the Floor 100 Exclusive Card."
			)
			order += 1
			addActionRow(
				order,
				"Next Challenge: Floor " .. tostring(nextFloor),
				enemyInfo,
				"Challenge",
				pal.accentBlue,
				function()
					local res = callServer({ action = "ChallengeTower", floor = nextFloor })
					if res.ok then
						local won = type(res.data) == "table" and res.data.won == true
						setStatus(if won then "Floor cleared!" else "Defeated — power up your deck!", not won)
						refreshMode("Tower")
					else
						setStatus(tostring(res.error or "Challenge failed"), true)
					end
				end
			)
		elseif mode == "Raid" then
			titleLabel.Text = "Dimensional Raids"
			local rawRaids = if type(data.raids) == "table"
				then data.raids
				else {
					{ id = "raid_skull_island", name = "Skull Island Breach", recommendedPower = 1500, worldReq = 3 },
					{ id = "raid_shibuya_breach", name = "Shibuya Breach", recommendedPower = 3500, worldReq = 4 },
				}
			local raidList: { { [string]: any } } = {}
			for k, r in pairs(rawRaids) do
				if type(r) == "table" then
					local copy = table.clone(r)
					if not copy.id and type(k) == "string" then
						copy.id = k
					end
					table.insert(raidList, copy)
				end
			end
			table.sort(raidList, function(a, b)
				local wa = tonumber(a.worldReq) or 0
				local wb = tonumber(b.worldReq) or 0
				if wa ~= wb then
					return wa < wb
				end
				return tostring(a.id or "") < tostring(b.id or "")
			end)
			for _, raid in ipairs(raidList) do
				local rId = tostring(raid.id or "raid_skull_island")
				local rName = tostring(raid.displayName or raid.name or rId)
				local rPow = tonumber(raid.recommendedPower) or 1000
				addActionRow(
					order,
					rName,
					string.format("Rec. Power: %d • High-tier Charm & Gem drops", rPow),
					"Start Raid",
					pal.accentPurple,
					function()
						local res = callServer({ action = "StartRaid", raidId = rId })
						if res.ok then
							setStatus("Raid completed!", false)
							refreshMode("Raid")
						else
							setStatus(tostring(res.error or "Raid failed"), true)
						end
					end
				)
				order += 1
			end
		elseif mode == "WorldBoss" then
			titleLabel.Text = "World Bosses"
			local rawBosses = if type(data.bosses) == "table" then data.bosses else {}
			local bossList: { { [string]: any } } = {}
			for k, b in pairs(rawBosses) do
				if type(b) == "table" then
					local copy = table.clone(b)
					if not copy.worldId then
						copy.worldId = tonumber(k)
					end
					table.insert(bossList, copy)
				end
			end
			table.sort(bossList, function(a, b)
				return (tonumber(a.worldId) or 0) < (tonumber(b.worldId) or 0)
			end)
			if #bossList == 0 then
				addInfoCard(order, "World Bosses", "Unlock worlds in Battle mode to challenge each World Boss.")
			else
				for _, boss in ipairs(bossList) do
					local bId = tostring(boss.id or boss.bossId or "")
					local bName = tostring(boss.displayName or boss.name or bId)
					local bWorld = tonumber(boss.worldId) or 1
					addActionRow(
						order,
						string.format("W%d: %s", bWorld, bName),
						string.format("Rec. Power: %d", tonumber(boss.recommendedPower) or 500),
						"Challenge",
						pal.accentGold,
						function()
							local res = callServer({ action = "ChallengeWorldBoss", worldId = bWorld, bossId = bId })
							if res.ok then
								setStatus("World Boss battle finished!", false)
								refreshMode("WorldBoss")
							else
								setStatus(tostring(res.error or "Battle failed"), true)
							end
						end
					)
					order += 1
				end
			end
		elseif mode == "Ranked" then
			titleLabel.Text = "Ranked Arena (Async PvP)"
			local elo = tonumber(data.elo or data.rating) or 1000
			local tier = tostring(data.tier or "Bronze")
			addInfoCard(
				order,
				string.format("Rating: %d Elo (%s)", elo, tier),
				"Set your Defense Deck in the Deck menu. Reach Grandmaster for an Exclusive Card!"
			)
			order += 1
			addActionRow(
				order,
				"Season Rewards",
				"Claim your current tier milestone rewards.",
				"Claim",
				pal.accentGold,
				function()
					local res = callServer({ action = "ClaimRankedReward" })
					if res.ok then
						setStatus("Ranked reward claimed!", false)
						refreshMode("Ranked")
					else
						setStatus(tostring(res.error or "Nothing to claim yet"), true)
					end
				end
			)
			order += 1
			local opps = if type(data.opponents) == "table" then data.opponents else {}
			if #opps == 0 then
				addActionRow(
					order,
					"Quick Ranked Match",
					"Battle a matched defense squad for Elo.",
					"Battle",
					pal.accentBlue,
					function()
						local res = callServer({ action = "PlayRanked" })
						if res.ok then
							setStatus("Ranked match complete!", false)
							refreshMode("Ranked")
						else
							setStatus(tostring(res.error or "Match failed"), true)
						end
					end
				)
			else
				for _, opp in ipairs(opps) do
					local oppName = tostring(opp.name or "Challenger")
					local oppElo = tonumber(opp.elo or opp.rating) or 1000
					local oppId = tonumber(opp.userId)
					addActionRow(
						order,
						oppName,
						string.format("Elo: %d • Power: %d", oppElo, tonumber(opp.power) or 500),
						"Battle",
						pal.accentBlue,
						function()
							local res = callServer({ action = "PlayRanked", targetUserId = oppId })
							if res.ok then
								setStatus("Ranked match complete!", false)
								refreshMode("Ranked")
							else
								setStatus(tostring(res.error or "Match failed"), true)
							end
						end
					)
					order += 1
				end
			end
		elseif mode == "Rebirth" then
			titleLabel.Text = "Rebirth & Skill Tree"
			local rebirths = tonumber(data.rebirths or data.rebirthCount or cachedState.rebirthCount) or 0
			local points = tonumber(data.skillPoints or data.rebirthTokens) or 0
			local skillNodes = if type(data.skillNodes) == "table" then data.skillNodes else {}
			addActionRow(
				order,
				string.format("Rebirths: %d • Skill Points: %d", rebirths, points),
				"Rebirth grants permanent Luck & Power multipliers + Skill Points.",
				"Rebirth",
				pal.accentGold,
				function()
					local res = callServer({ action = "PerformRebirth" })
					if res.ok then
						setStatus("Rebirth complete!", false)
						refreshMode("Rebirth")
					else
						setStatus(tostring(res.error or "Rebirth requirements not met"), true)
					end
				end
			)
			order += 1
			local rawNodes = data.catalog or data.nodes or data.skillTree
			local nodeList: { { [string]: any } } = {}
			if type(rawNodes) == "table" then
				for k, n in pairs(rawNodes) do
					if type(n) == "table" then
						local copy = table.clone(n)
						if not copy.id and type(k) == "string" then
							copy.id = k
						end
						table.insert(nodeList, copy)
					end
				end
				table.sort(nodeList, function(a, b)
					return tostring(a.id or "") < tostring(b.id or "")
				end)
			end
			for _, node in ipairs(nodeList) do
				local nId = tostring(node.id or "")
				local nName = tostring(node.displayName or node.name or nId)
				local lv = tonumber(node.level or skillNodes[nId]) or 0
				local maxLv = tonumber(node.maxLevel) or 5
				addActionRow(
					order,
					string.format("%s (%d/%d)", nName, lv, maxLv),
					tostring(node.description or "Permanent passive boost"),
					"Upgrade",
					pal.accentGreen,
					function()
						local res = callServer({ action = "BuySkillNode", nodeId = nId })
						if res.ok then
							setStatus("Skill upgraded!", false)
							refreshMode("Rebirth")
						else
							setStatus(tostring(res.error or "Cannot upgrade"), true)
						end
					end
				)
				order += 1
			end
		elseif mode == "Charms" then
			titleLabel.Text = "Charms & Talismans"
			local equipped = if type(data.equipped) == "table" then data.equipped else {}
			for slot = 1, 3 do
				local cId = equipped[slot] or equipped[tostring(slot)]
				if cId then
					addActionRow(
						order,
						string.format("Slot %d: %s", slot, tostring(cId)),
						"Currently equipped charm bonus active.",
						"Unequip",
						pal.accentRed or Color3.fromRGB(205, 65, 65),
						function()
							local res = callServer({ action = "UnequipCharm", slot = slot })
							if res.ok then
								setStatus("Charm unequipped", false)
								refreshMode("Charms")
							else
								setStatus(tostring(res.error or "Failed"), true)
							end
						end
					)
				else
					addInfoCard(
						order,
						string.format("Slot %d: Empty", slot),
						"Equip a crafted charm below to activate its bonus."
					)
				end
				order += 1
			end
			local owned = if type(data.owned) == "table" then data.owned else {}
			for _, item in ipairs(owned) do
				local cId = tostring(item.id or item)
				local cName = tostring(item.name or cId)
				addActionRow(
					order,
					"Owned: " .. cName,
					tostring(item.description or "Tap Equip to place in next open slot."),
					"Equip",
					pal.accentBlue,
					function()
						local res = callServer({ action = "EquipCharm", charmId = cId, slot = 1 })
						if res.ok then
							setStatus("Charm equipped!", false)
							refreshMode("Charms")
						else
							setStatus(tostring(res.error or "Equip failed"), true)
						end
					end
				)
				order += 1
			end
			local recipes = if type(data.recipes or data.catalog) == "table" then (data.recipes or data.catalog) else {}
			for _, rec in ipairs(recipes) do
				local rId = tostring(rec.id or "")
				local rName = tostring(rec.name or rId)
				addActionRow(
					order,
					"Craft: " .. rName,
					tostring(rec.description or "Craft using Raid/Boss materials."),
					"Craft",
					pal.accentGreen,
					function()
						local res = callServer({ action = "CraftCharm", charmId = rId })
						if res.ok then
							setStatus("Charm crafted!", false)
							refreshMode("Charms")
						else
							setStatus(tostring(res.error or "Missing materials"), true)
						end
					end
				)
				order += 1
			end
		end
	end

	local function openMode(mode: string)
		backdrop.Visible = true
		refreshMode(mode)
	end

	closeBtn.Activated:Connect(function()
		backdrop.Visible = false
	end)

	local controller: EndgameController = {
		openTower = function()
			openMode("Tower")
		end,
		openRaid = function()
			openMode("Raid")
		end,
		openRanked = function()
			openMode("Ranked")
		end,
		openRebirth = function()
			openMode("Rebirth")
		end,
		openCharms = function()
			openMode("Charms")
		end,
		openWorldBoss = function()
			openMode("WorldBoss")
		end,
		closeAll = function()
			backdrop.Visible = false
		end,
		applyTheme = function()
			local pal = Theme.getPalette()
			panel.BackgroundColor3 = pal.bgPanel
			titleLabel.TextColor3 = pal.textPrimary
			if activeMode and backdrop.Visible then
				renderCurrent(activeMode, lastData)
			end
		end,
		updateState = function(state: { [string]: any })
			cachedState = state
		end,
	}

	return controller
end

return EndgameModals
