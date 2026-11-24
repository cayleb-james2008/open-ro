--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type MetaController = {
	openQuests: () -> (),
	openIndex: () -> (),
	openShop: () -> (),
	openLeaderboards: () -> (),
	closeAll: () -> (),
	applyTheme: () -> (),
	updateState: (state: { [string]: any }) -> (),
}

local MetaModals = {}

local function clearChildren(container: Instance)
	for _, child in ipairs(container:GetChildren()) do
		if not child:IsA("UIListLayout") and not child:IsA("UIGridLayout") and not child:IsA("UIPadding") then
			child:Destroy()
		end
	end
end

local function tryLoadConfig(name: string): any
	local ok, result = pcall(function()
		local sharedFolder = ReplicatedStorage:FindFirstChild("Shared")
		local configs = if sharedFolder then sharedFolder:FindFirstChild("Configs") else nil
		local mod = if configs then configs:FindFirstChild(name) else nil
		if mod and mod:IsA("ModuleScript") then
			return require(mod)
		end
		return nil
	end)
	if ok then
		return result
	end
	return nil
end

function MetaModals.build(parent: Instance, api: any, _theme: any): MetaController
	local palette = Theme.getPalette()
	local cachedState: { [string]: any } = {}
	local activeMode: string? = nil
	local lastData: { [string]: any } = {}
	local activeBoardType = "Power"

	local backdrop = Instance.new("Frame")
	backdrop.Name = "MetaModalBackdrop"
	backdrop.Size = UDim2.fromScale(1, 1)
	backdrop.BackgroundColor3 = Color3.fromRGB(8, 10, 16)
	backdrop.BackgroundTransparency = 0.35
	backdrop.BorderSizePixel = 0
	backdrop.Active = true
	backdrop.Visible = false
	backdrop.ZIndex = 30
	backdrop.Parent = parent

	local panel =
		Components.makePanel(backdrop, "MetaPanel", UDim2.fromScale(0.94, 0.85), UDim2.fromScale(0.5, 0.5), 14)
	panel.AnchorPoint = Vector2.new(0.5, 0.5)
	panel.ZIndex = 31

	local sizeConstraint = Instance.new("UISizeConstraint")
	sizeConstraint.MinSize = Vector2.new(300, 380)
	sizeConstraint.MaxSize = Vector2.new(560, 680)
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
		"Menu",
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
		palette.accentRed,
		Color3.fromRGB(255, 255, 255)
	)
	closeBtn.ZIndex = 33
	closeBtn.TextSize = 13

	local statusLabel = Components.makeLabel(
		panel,
		"StatusBanner",
		"Ready",
		UDim2.new(1, -24, 0, 24),
		UDim2.fromOffset(12, 60),
		Theme.FONT_BODY,
		12,
		palette.textSecondary,
		Enum.TextXAlignment.Left
	)
	statusLabel.ZIndex = 32

	local scroll = Instance.new("ScrollingFrame")
	scroll.Name = "BodyScroll"
	scroll.Size = UDim2.new(1, -24, 1, -100)
	scroll.Position = UDim2.fromOffset(12, 88)
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
		statusLabel.TextColor3 = if isError then pal.accentRed else pal.accentGreen
	end

	local function addInfoCard(order: number, heading: string, detail: string)
		local pal = Theme.getPalette()
		local card = Instance.new("Frame")
		card.Name = "Info_" .. tostring(order)
		card.LayoutOrder = order
		card.Size = UDim2.new(1, -4, 0, 62)
		card.BackgroundColor3 = pal.bgCard
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
			13,
			pal.textPrimary,
			Enum.TextXAlignment.Left
		)
		h.ZIndex = 34
		local d = Components.makeLabel(
			card,
			"Detail",
			detail,
			UDim2.new(1, -16, 0, 30),
			UDim2.fromOffset(8, 28),
			Theme.FONT_BODY,
			11,
			pal.textSecondary,
			Enum.TextXAlignment.Left
		)
		d.TextWrapped = true
		d.ZIndex = 34
	end

	local function addActionRow(
		order: number,
		heading: string,
		detail: string,
		btnText: string,
		btnColor: Color3,
		enabled: boolean,
		onClick: () -> ()
	)
		local pal = Theme.getPalette()
		local row = Instance.new("Frame")
		row.Name = "Row_" .. tostring(order)
		row.LayoutOrder = order
		row.Size = UDim2.new(1, -4, 0, 68)
		row.BackgroundColor3 = pal.bgCard
		row.BorderSizePixel = 0
		row.ZIndex = 33
		row.Parent = scroll
		Components.addCorner(row, 10)
		Components.addStroke(row, pal.border, 1, 0.25)
		local h = Components.makeLabel(
			row,
			"Heading",
			heading,
			UDim2.new(1, -126, 0, 22),
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
			UDim2.new(1, -126, 0, 34),
			UDim2.fromOffset(10, 28),
			Theme.FONT_BODY,
			11,
			pal.textSecondary,
			Enum.TextXAlignment.Left
		)
		d.TextWrapped = true
		d.ZIndex = 34
		local btn = Components.makeButton(
			row,
			"ActionBtn",
			btnText,
			UDim2.fromOffset(108, 44),
			UDim2.new(1, -116, 0.5, -22),
			if enabled then btnColor else pal.bgMuted,
			Color3.fromRGB(255, 255, 255)
		)
		btn.ZIndex = 34
		btn.TextSize = 12
		btn.Active = enabled
		btn.AutoButtonColor = enabled
		if enabled then
			btn.Activated:Connect(onClick)
		end
	end

	local renderCurrent: (mode: string, data: { [string]: any }) -> ()

	local function refreshMode(mode: string)
		activeMode = mode
		if mode == "Index" then
			lastData = cachedState
			setStatus("54-Card Collection Index", false)
			renderCurrent("Index", cachedState)
			return
		end
		local payload: { [string]: any } = if mode == "Quests"
			then { action = "GetRetentionStatus" }
			elseif mode == "Shop" then { action = "GetShopStatus" }
			else { action = "GetLeaderboards", boardType = activeBoardType }
		setStatus("Loading " .. mode .. "...", false)
		local res = callServer(payload)
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

		if mode == "Quests" then
			titleLabel.Text = "Quests, Streaks & Codes"
			-- Promo Code Box
			local codeRow = Instance.new("Frame")
			codeRow.Name = "PromoCodeRow"
			codeRow.LayoutOrder = order
			codeRow.Size = UDim2.new(1, -4, 0, 62)
			codeRow.BackgroundColor3 = pal.bgCard
			codeRow.BorderSizePixel = 0
			codeRow.ZIndex = 33
			codeRow.Parent = scroll
			Components.addCorner(codeRow, 10)
			Components.addStroke(codeRow, pal.border, 1, 0.25)

			local codeBox = Instance.new("TextBox")
			codeBox.Name = "CodeInput"
			codeBox.Size = UDim2.new(1, -132, 0, 44)
			codeBox.Position = UDim2.fromOffset(10, 9)
			codeBox.BackgroundColor3 = pal.bgDeep
			codeBox.TextColor3 = pal.textPrimary
			codeBox.PlaceholderText = "Enter promo code..."
			codeBox.PlaceholderColor3 = pal.textMuted
			codeBox.Font = Theme.FONT_BODY
			codeBox.TextSize = 13
			codeBox.Text = ""
			codeBox.ClearTextOnFocus = false
			codeBox.ZIndex = 34
			codeBox.Parent = codeRow
			Components.addCorner(codeBox, 8)

			local redeemBtn = Components.makeButton(
				codeRow,
				"RedeemBtn",
				"Redeem",
				UDim2.fromOffset(106, 44),
				UDim2.new(1, -114, 0.5, -22),
				pal.accentGold,
				pal.textOnAccent
			)
			redeemBtn.ZIndex = 34
			redeemBtn.TextSize = 12
			redeemBtn.Activated:Connect(function()
				local code = string.gsub(codeBox.Text, "^%s*(.-)%s*$", "%1")
				if code == "" then
					setStatus("Enter a promo code first", true)
					return
				end
				local res = callServer({ action = "RedeemCode", code = code })
				if res.ok then
					codeBox.Text = ""
					setStatus("Code redeemed!", false)
					refreshMode("Quests")
				else
					setStatus(tostring(res.error or "Invalid or used code"), true)
				end
			end)
			order += 1

			local streakDay = tonumber(data.loginStreak or data.streakDay) or 1
			local canClaimStreak = data.canClaimStreak ~= false
			addActionRow(
				order,
				string.format("7-Day Login Streak (Day %d)", streakDay),
				"Daily login bonus with Tickets & Gems.",
				if canClaimStreak then "Claim" else "Claimed",
				pal.accentGreen,
				canClaimStreak,
				function()
					local res = callServer({ action = "ClaimLoginStreak" })
					if res.ok then
						setStatus("Login streak claimed!", false)
						refreshMode("Quests")
					else
						setStatus(tostring(res.error or "Already claimed today"), true)
					end
				end
			)
			order += 1

			local playtime = if type(data.playtimeMilestones) == "table" then data.playtimeMilestones else {}
			for _, ms in ipairs(playtime) do
				local mId = tostring(ms.id or "")
				local ready = ms.ready == true and ms.claimed ~= true
				local btnTxt = if ms.claimed then "Claimed" elseif ready then "Claim" else "Locked"
				addActionRow(
					order,
					"Playtime: " .. tostring(ms.label or mId),
					tostring(ms.rewardText or "Session milestone reward"),
					btnTxt,
					pal.accentBlue,
					ready,
					function()
						local res = callServer({ action = "ClaimPlaytime", milestoneId = mId })
						if res.ok then
							setStatus("Playtime reward claimed!", false)
							refreshMode("Quests")
						else
							setStatus(tostring(res.error or "Not ready"), true)
						end
					end
				)
				order += 1
			end

			local quests = if type(data.quests) == "table" then data.quests else {}
			for _, q in ipairs(quests) do
				local qId = tostring(q.id or "")
				local prog = tonumber(q.progress) or 0
				local goal = tonumber(q.goal) or 1
				local ready = prog >= goal and q.claimed ~= true
				local btnTxt = if q.claimed
					then "Claimed"
					elseif ready then "Claim"
					else string.format("%d/%d", prog, goal)
				addActionRow(
					order,
					tostring(q.title or qId),
					tostring(q.description or "Complete objective for rewards"),
					btnTxt,
					pal.accentPurple,
					ready,
					function()
						local res = callServer({ action = "ClaimQuest", questId = qId })
						if res.ok then
							setStatus("Quest reward claimed!", false)
							refreshMode("Quests")
						else
							setStatus(tostring(res.error or "Quest incomplete"), true)
						end
					end
				)
				order += 1
			end
		elseif mode == "Index" then
			titleLabel.Text = "54-Card Collection Index"
			local cardsCfg = tryLoadConfig("Cards")
			local list = if cardsCfg and type(cardsCfg.List) == "table" then cardsCfg.List else {}
			local discoveredMap: { [string]: boolean } = {}
			local rawDisc = cachedState.discoveredCards or cachedState.index
			if type(rawDisc) == "table" then
				for k, v in pairs(rawDisc) do
					if type(k) == "string" and v then
						discoveredMap[k] = true
					elseif type(v) == "string" then
						discoveredMap[v] = true
					end
				end
			end
			if type(cachedState.cards) == "table" then
				for _, owned in ipairs(cachedState.cards) do
					local cid = owned.id or owned.cardId
					if cid then
						discoveredMap[tostring(cid)] = true
					end
				end
			end
			local discCount = 0
			for _ in pairs(discoveredMap) do
				discCount += 1
			end
			local totalCount = math.max(#list, 54)
			local luckBonus = math.floor((discCount / totalCount) * 50)
			addInfoCard(
				order,
				string.format("Discovered: %d / %d Cards", discCount, totalCount),
				string.format("Live Index Luck Bonus: +%d%% Luck on all rolls", luckBonus)
			)
			order += 1

			local groups = { "World 1", "World 2", "World 3", "World 4", "World 5", "Exclusives" }
			for _, gName in ipairs(groups) do
				local gCards: { string } = {}
				for _, c in ipairs(list) do
					local w = tonumber(c.worldId)
					local matches = (gName == "Exclusives" and (c.isExclusive or not w))
						or (gName == ("World " .. tostring(w)))
					if matches then
						local mark = if discoveredMap[c.id] then "[Found] " else "[???] "
						table.insert(gCards, mark .. tostring(c.name or c.id))
					end
				end
				local summary = if #gCards > 0
					then table.concat(gCards, " • ")
					else "Roll banners or clear endgame modes to discover."
				addInfoCard(order, gName, summary)
				order += 1
			end
		elseif mode == "Shop" then
			titleLabel.Text = "Card Shop & Passes"
			local hasPremium = data.hasPremium == true
			local inGroup = data.inGroup == true
			addInfoCard(
				order,
				"Account Perks",
				string.format(
					"Roblox Premium: %s  |  Group Member: %s",
					if hasPremium then "Active (+10% Luck)" else "Inactive",
					if inGroup then "Active (+10% Yen)" else "Join Group"
				)
			)
			order += 1

			local prodCfg = tryLoadConfig("Products")
			local items = if type(data.products) == "table" and #data.products > 0
				then data.products
				elseif prodCfg and type(prodCfg.List) == "table" then prodCfg.List
				else {}
			for _, p in ipairs(items) do
				local pId = tostring(p.id or p.key or "")
				local pName = tostring(p.name or pId)
				local price = tonumber(p.robux or p.priceRobux) or 0
				local owned = p.owned == true
				local btnTxt = if owned then "Owned" else string.format("%d R$", price)
				addActionRow(
					order,
					pName,
					tostring(p.description or "Studio test purchase preview supported."),
					btnTxt,
					pal.accentGold,
					not owned,
					function()
						local res = callServer({ action = "PromptPurchase", productId = pId })
						if res.ok then
							setStatus("Purchase processed: " .. pName, false)
							refreshMode("Shop")
						else
							setStatus(tostring(res.error or "Purchase cancelled"), true)
						end
					end
				)
				order += 1
			end
		elseif mode == "Leaderboards" then
			titleLabel.Text = "Global Leaderboards"
			local tabRow = Instance.new("Frame")
			tabRow.Name = "TabRow"
			tabRow.LayoutOrder = order
			tabRow.Size = UDim2.new(1, -4, 0, 48)
			tabRow.BackgroundTransparency = 1
			tabRow.ZIndex = 33
			tabRow.Parent = scroll
			local tabLayout = Instance.new("UIGridLayout")
			tabLayout.CellSize = UDim2.new(0.32, 0, 0, 44)
			tabLayout.CellPadding = UDim2.fromOffset(6, 0)
			tabLayout.Parent = tabRow
			for _, bType in ipairs({ "Power", "Tower", "Ranked" }) do
				local isSel = (activeBoardType == bType)
				local tBtn = Components.makeButton(
					tabRow,
					bType .. "Tab",
					bType,
					UDim2.fromOffset(80, 44),
					UDim2.fromOffset(0, 0),
					if isSel then pal.accentBlue else pal.bgMuted,
					Color3.fromRGB(255, 255, 255)
				)
				tBtn.ZIndex = 34
				tBtn.TextSize = 13
				tBtn.Activated:Connect(function()
					activeBoardType = bType
					refreshMode("Leaderboards")
				end)
			end
			order += 1

			local entries = if type(data.entries) == "table" then data.entries else {}
			if #entries == 0 then
				addInfoCard(
					order,
					activeBoardType .. " Rankings",
					"No entries recorded yet. Play to set the first score!"
				)
			else
				for idx, entry in ipairs(entries) do
					addInfoCard(
						order,
						string.format("#%d %s", idx, tostring(entry.name or "Player")),
						string.format("Score: %s", tostring(entry.score or entry.value or 0))
					)
					order += 1
				end
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

	local controller: MetaController = {
		openQuests = function()
			openMode("Quests")
		end,
		openIndex = function()
			openMode("Index")
		end,
		openShop = function()
			openMode("Shop")
		end,
		openLeaderboards = function()
			openMode("Leaderboards")
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

return MetaModals
