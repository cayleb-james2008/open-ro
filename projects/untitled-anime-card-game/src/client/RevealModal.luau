--!strict

local TweenService = game:GetService("TweenService")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))
local SettingsModal = require(script.Parent:WaitForChild("SettingsModal"))

export type RollCardResult = {
	uid: string?,
	id: string,
	name: string,
	rarity: string,
	mutation: string?,
	role: string?,
	hp: number?,
	atk: number?,
	spd: number?,
	level: number?,
	stars: number?,
	oddsN: number?,
	isNew: boolean?,
	autoSoldYen: number?,
}

export type RevealModalController = {
	play: (results: { RollCardResult }, isAuto: boolean?, onComplete: (() -> ())?) -> (),
	skip: () -> (),
	isPlaying: () -> boolean,
	destroy: () -> (),
}

local RevealModal = {}

function RevealModal.create(parent: Instance, getSettings: () -> SettingsModal.SettingsState): RevealModalController
	local playing = false
	local skipped = false
	local runToken = 0
	local activeFinishCallback: (() -> ())? = nil

	local overlay = Instance.new("TextButton")
	overlay.Name = "RevealOverlay"
	overlay.Size = UDim2.fromScale(1, 1)
	overlay.BackgroundColor3 = Color3.fromRGB(8, 11, 24)
	overlay.BackgroundTransparency = 0.12
	overlay.BorderSizePixel = 0
	overlay.Text = ""
	overlay.AutoButtonColor = false
	overlay.Visible = false
	overlay.ZIndex = 35
	overlay.Parent = parent

	local stage = Instance.new("Frame")
	stage.Name = "Stage"
	stage.Size = UDim2.fromScale(1, 1)
	stage.BackgroundTransparency = 1
	stage.ZIndex = 36
	stage.Parent = overlay

	local auraRing = Instance.new("Frame")
	auraRing.Name = "AuraRing"
	auraRing.Size = UDim2.fromOffset(220, 220)
	auraRing.Position = UDim2.fromScale(0.5, 0.44)
	auraRing.AnchorPoint = Vector2.new(0.5, 0.5)
	auraRing.BackgroundColor3 = Color3.fromRGB(255, 195, 70)
	auraRing.BackgroundTransparency = 0.8
	auraRing.BorderSizePixel = 0
	auraRing.ZIndex = 36
	auraRing.Parent = stage
	Components.addCorner(auraRing, 999)
	local auraStroke = Components.addStroke(auraRing, Color3.fromRGB(255, 215, 100), 4, 0.15)

	local rayCross = Instance.new("Frame")
	rayCross.Name = "RayCross"
	rayCross.Size = UDim2.fromOffset(300, 22)
	rayCross.Position = UDim2.fromScale(0.5, 0.44)
	rayCross.AnchorPoint = Vector2.new(0.5, 0.5)
	rayCross.BackgroundColor3 = Color3.fromRGB(255, 220, 120)
	rayCross.BackgroundTransparency = 0.65
	rayCross.BorderSizePixel = 0
	rayCross.ZIndex = 36
	rayCross.Parent = stage
	Components.addCorner(rayCross, 11)

	local bannerTitle = Components.makeLabel(
		stage,
		"BannerTitle",
		"SUMMONING...",
		UDim2.new(0.9, 0, 0, 34),
		UDim2.fromScale(0.05, 0.08),
		Theme.FONT_DISPLAY,
		22,
		Color3.fromRGB(245, 248, 255),
		Enum.TextXAlignment.Center
	)
	bannerTitle.ZIndex = 38

	local oddsBanner = Components.makeLabel(
		stage,
		"OddsBanner",
		"",
		UDim2.new(0.9, 0, 0, 22),
		UDim2.new(0.05, 0, 0.08, 36),
		Theme.FONT_MONO,
		14,
		Color3.fromRGB(245, 195, 80),
		Enum.TextXAlignment.Center
	)
	oddsBanner.ZIndex = 38

	local cardsHolder = Instance.new("Frame")
	cardsHolder.Name = "CardsHolder"
	cardsHolder.Size = UDim2.fromScale(0.94, 0.56)
	cardsHolder.Position = UDim2.fromScale(0.5, 0.46)
	cardsHolder.AnchorPoint = Vector2.new(0.5, 0.5)
	cardsHolder.BackgroundTransparency = 1
	cardsHolder.ZIndex = 37
	cardsHolder.Parent = stage

	local cardsLayout = Instance.new("UIListLayout")
	cardsLayout.FillDirection = Enum.FillDirection.Horizontal
	cardsLayout.HorizontalAlignment = Enum.HorizontalAlignment.Center
	cardsLayout.VerticalAlignment = Enum.VerticalAlignment.Center
	cardsLayout.Padding = UDim.new(0, 12)
	cardsLayout.Wraps = true
	cardsLayout.Parent = cardsHolder

	local statusPill = Components.makeLabel(
		stage,
		"StatusPill",
		"",
		UDim2.new(0.9, 0, 0, 26),
		UDim2.fromScale(0.05, 0.76),
		Theme.FONT_DISPLAY,
		14,
		Color3.fromRGB(116, 238, 168),
		Enum.TextXAlignment.Center
	)
	statusPill.ZIndex = 38

	local actionBtn = Components.makeButton(
		stage,
		"ActionButton",
		"Continue",
		UDim2.fromOffset(180, 48),
		UDim2.new(0.5, -90, 1, -76),
		Theme.getPalette().accentGold,
		Theme.getPalette().textOnAccent
	)
	actionBtn.ZIndex = 39

	local flashFrame = Instance.new("Frame")
	flashFrame.Name = "FlashFrame"
	flashFrame.Size = UDim2.fromScale(1, 1)
	flashFrame.BackgroundColor3 = Color3.fromRGB(255, 255, 255)
	flashFrame.BackgroundTransparency = 1
	flashFrame.BorderSizePixel = 0
	flashFrame.ZIndex = 39
	flashFrame.Parent = overlay

	local function clearCards()
		for _, child in cardsHolder:GetChildren() do
			if not child:IsA("UIListLayout") then
				child:Destroy()
			end
		end
	end

	local function finishReveal()
		if not playing then
			return
		end
		playing = false
		overlay.Visible = false
		clearCards()
		stage.Position = UDim2.fromOffset(0, 0)
		local cb = activeFinishCallback
		activeFinishCallback = nil
		if cb then
			cb()
		end
	end

	local function shouldSkipBuildup(highestOrder: number, skipSetting: string): boolean
		if skipSetting == "None" or skipSetting == "" then
			return false
		end
		local threshold = Theme.getRarity(skipSetting).order
		return highestOrder < threshold
	end

	local function renderResults(results: { RollCardResult }, highestRarity: Theme.RarityStyle)
		clearCards()
		local count = #results
		local cardW = if count <= 1 then 210 else 154
		local cardH = if count <= 1 then 280 else 210

		local newCount = 0
		local soldYen = 0
		for _, item in results do
			local cardFrame =
				Components.makeCardView(cardsHolder, item, UDim2.fromOffset(cardW, cardH), UDim2.fromOffset(0, 0))
			cardFrame.ZIndex = 38
			if item.isNew then
				newCount += 1
				local newTag = Components.makeLabel(
					cardFrame,
					"NewTag",
					"NEW!",
					UDim2.fromOffset(56, 20),
					UDim2.new(1, -62, 0.56, -22),
					Theme.FONT_DISPLAY,
					11,
					Color3.fromRGB(255, 224, 92),
					Enum.TextXAlignment.Right
				)
				newTag.ZIndex = 39
			end
			if item.autoSoldYen and item.autoSoldYen > 0 then
				soldYen += item.autoSoldYen
			end
		end

		local lead = results[1]
		if count == 1 and lead then
			bannerTitle.Text = string.upper(highestRarity.name) .. " — " .. lead.name
			local oddsN = lead.oddsN or 0
			local oddsText = if oddsN > 0 then Components.formatOdds(oddsN) else highestRarity.oddsLabel
			oddsBanner.Text = oddsText
		else
			bannerTitle.Text = string.format("%dx ROLL COMPLETE — BEST: %s", count, string.upper(highestRarity.name))
			oddsBanner.Text = highestRarity.oddsLabel
		end
		bannerTitle.TextColor3 = highestRarity.glow
		oddsBanner.TextColor3 = highestRarity.color

		local notes: { string } = {}
		if newCount > 0 then
			table.insert(
				notes,
				string.format("%d New Card%s Added to Index", newCount, if newCount > 1 then "s" else "")
			)
		end
		if soldYen > 0 then
			table.insert(notes, string.format("Auto-Sold Dupes: +%s Yen", Components.formatNumber(soldYen)))
		end
		statusPill.Text = table.concat(notes, "  |  ")
	end

	actionBtn.MouseButton1Click:Connect(function()
		if not playing then
			return
		end
		if not skipped then
			skipped = true
		else
			finishReveal()
		end
	end)

	overlay.MouseButton1Click:Connect(function()
		if not playing then
			return
		end
		if not skipped then
			skipped = true
		else
			finishReveal()
		end
	end)

	return {
		play = function(results: { RollCardResult }, isAuto: boolean?, onComplete: (() -> ())?)
			if #results == 0 then
				if onComplete then
					onComplete()
				end
				return
			end

			runToken += 1
			local myToken = runToken
			playing = true
			skipped = false
			activeFinishCallback = onComplete

			local highestOrder = 1
			local highestRarity = Theme.Rarities.Common
			for _, res in results do
				local rStyle = Theme.getRarity(res.rarity)
				if rStyle.order >= highestOrder then
					highestOrder = rStyle.order
					highestRarity = rStyle
				end
			end

			local settings = getSettings()
			local skipBuildup = shouldSkipBuildup(highestOrder, settings.skipBelowRarity)

			overlay.BackgroundColor3 = highestRarity.bgTint
			auraRing.BackgroundColor3 = highestRarity.color
			auraStroke.Color = highestRarity.glow
			rayCross.BackgroundColor3 = highestRarity.glow

			if skipBuildup then
				skipped = true
				if isAuto then
					-- Instant finish on auto-roll below skip threshold
					finishReveal()
					return
				end
			end

			overlay.Visible = true
			cardsHolder.Visible = false
			statusPill.Text = ""
			actionBtn.Text = "Skip"

			if not skipBuildup then
				bannerTitle.Text = "ROLLING..."
				bannerTitle.TextColor3 = Color3.fromRGB(244, 247, 255)
				oddsBanner.Text = highestRarity.oddsLabel
				oddsBanner.TextColor3 = highestRarity.color

				if not settings.reducedMotion then
					auraRing.Size = UDim2.fromOffset(90, 90)
					rayCross.Rotation = 0
					TweenService:Create(
						auraRing,
						TweenInfo.new(0.45, Enum.EasingStyle.Back, Enum.EasingDirection.Out),
						{ Size = UDim2.fromOffset(260, 260) }
					):Play()
					TweenService:Create(
						rayCross,
						TweenInfo.new(0.55, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
						{ Rotation = 90 }
					):Play()
				end

				local waitSeconds = if highestOrder >= 6 then 0.75 elseif highestOrder >= 5 then 0.55 else 0.35
				local elapsed = 0
				while elapsed < waitSeconds and not skipped and runToken == myToken do
					elapsed += task.wait(0.05)
				end
				if runToken ~= myToken then
					return
				end
			end

			skipped = true
			cardsHolder.Visible = true
			renderResults(results, highestRarity)
			actionBtn.Text = "Continue"

			if not settings.reducedFlash and highestOrder >= 4 then
				flashFrame.BackgroundColor3 = highestRarity.glow
				flashFrame.BackgroundTransparency = 0.35
				TweenService:Create(
					flashFrame,
					TweenInfo.new(0.35, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
					{ BackgroundTransparency = 1 }
				):Play()
			end

			if not settings.reducedMotion and highestOrder >= 5 then
				task.spawn(function()
					for i = 1, 6 do
						if runToken ~= myToken or not playing then
							break
						end
						local amp = if highestOrder >= 6 then 10 else 5
						local dx = if i % 2 == 0 then amp else -amp
						stage.Position = UDim2.fromOffset(dx, (i % 3 - 1) * 3)
						task.wait(0.03)
					end
					if runToken == myToken then
						stage.Position = UDim2.fromOffset(0, 0)
					end
				end)
			end

			if isAuto then
				local hold = if highestOrder >= 5 then 1.1 else 0.4
				task.delay(hold, function()
					if runToken == myToken and playing then
						finishReveal()
					end
				end)
			end
		end,
		skip = function()
			if not playing then
				return
			end
			if not skipped then
				skipped = true
			else
				finishReveal()
			end
		end,
		isPlaying = function(): boolean
			return playing
		end,
		destroy = function()
			runToken += 1
			playing = false
			overlay:Destroy()
		end,
	}
end

return RevealModal
