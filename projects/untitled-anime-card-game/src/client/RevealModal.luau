--!strict

local TweenService = game:GetService("TweenService")

local Theme = require(script.Parent:WaitForChild("Theme"))
local Components = require(script.Parent:WaitForChild("Components"))

export type RollCardResult = {
	uid: string?,
	id: string?,
	cardId: string?,
	name: string?,
	rarity: string,
	mutation: string?,
	trait: string?,
	role: string?,
	level: number?,
	stars: number?,
	hp: number?,
	atk: number?,
	spd: number?,
	odds: number?,
	isNew: boolean?,
	autoSold: boolean?,
	soldYen: number?,
}

export type RevealSettings = {
	reducedMotion: boolean,
	reducedFlash: boolean,
	skipBelowRarity: string,
}

export type RevealCallbacks = {
	getSettings: () -> RevealSettings,
	playSfx: ((cue: string) -> ())?,
}

export type RevealModalController = {
	play: (results: { RollCardResult }, isAuto: boolean?, onComplete: (() -> ())?) -> (),
	setContext: (bannerName: string, weatherText: string) -> (),
	skip: () -> (),
	isPlaying: () -> boolean,
	destroy: () -> (),
}

local DRAMATIC_BANNERS: { [number]: string } = {
	[5] = "LEGENDARY PULL!",
	[6] = "MYTHIC AWAKENING!",
	[7] = "SECRET REVEAL!",
	[8] = "UNIVERSAL ASCENSION!",
	[9] = "NEXUS CONVERGENCE!",
}

local RevealModal = {}

local function shouldSkipBuildup(highestOrder: number, thresholdName: string): boolean
	if thresholdName == "" or thresholdName == "None" then
		return false
	end
	local threshold = Theme.getRarity(thresholdName)
	return highestOrder <= threshold.order
end

function RevealModal.create(
	parent: Instance,
	getSettingsOrCallbacks: (() -> RevealSettings) | RevealCallbacks,
	playSfxOpt: ((cue: string) -> ())?
): RevealModalController
	local getSettings: () -> RevealSettings
	local playSfx: ((cue: string) -> ())?
	if typeof(getSettingsOrCallbacks) == "function" then
		getSettings = getSettingsOrCallbacks
		playSfx = playSfxOpt
	else
		getSettings = getSettingsOrCallbacks.getSettings
		playSfx = getSettingsOrCallbacks.playSfx or playSfxOpt
	end

	local function emitSfx(cue: string)
		if playSfx then
			playSfx(cue)
		end
	end

	local overlay = Instance.new("TextButton")
	overlay.Name = "RevealOverlay"
	overlay.Size = UDim2.fromScale(1, 1)
	overlay.BackgroundColor3 = Color3.fromRGB(6, 9, 18)
	overlay.BackgroundTransparency = 0.14
	overlay.BorderSizePixel = 0
	overlay.Text = ""
	overlay.AutoButtonColor = false
	overlay.Visible = false
	overlay.ZIndex = 60
	overlay.Parent = parent

	local stage = Instance.new("Frame")
	stage.Name = "Stage"
	stage.Size = UDim2.fromScale(1, 1)
	stage.Position = UDim2.fromOffset(0, 0)
	stage.BackgroundTransparency = 1
	stage.ZIndex = 61
	stage.Parent = overlay

	local flashFrame = Instance.new("Frame")
	flashFrame.Name = "FlashFrame"
	flashFrame.Size = UDim2.fromScale(1, 1)
	flashFrame.BackgroundColor3 = Color3.fromRGB(255, 255, 255)
	flashFrame.BackgroundTransparency = 1
	flashFrame.BorderSizePixel = 0
	flashFrame.ZIndex = 75
	flashFrame.Parent = overlay

	local lightPillar = Instance.new("Frame")
	lightPillar.Name = "LightPillar"
	lightPillar.Size = UDim2.fromScale(0, 1)
	lightPillar.Position = UDim2.fromScale(0.5, 0.5)
	lightPillar.AnchorPoint = Vector2.new(0.5, 0.5)
	lightPillar.BackgroundColor3 = Color3.fromRGB(255, 215, 110)
	lightPillar.BackgroundTransparency = 0.35
	lightPillar.BorderSizePixel = 0
	lightPillar.Visible = false
	lightPillar.ZIndex = 61
	lightPillar.Parent = stage
	local pillarGrad = Instance.new("UIGradient")
	pillarGrad.Transparency = NumberSequence.new({
		NumberSequenceKeypoint.new(0, 0.85),
		NumberSequenceKeypoint.new(0.5, 0.1),
		NumberSequenceKeypoint.new(1, 0.85),
	})
	pillarGrad.Parent = lightPillar

	local shockwaveRing = Instance.new("Frame")
	shockwaveRing.Name = "ShockwaveRing"
	shockwaveRing.Size = UDim2.fromOffset(60, 60)
	shockwaveRing.Position = UDim2.fromScale(0.5, 0.46)
	shockwaveRing.AnchorPoint = Vector2.new(0.5, 0.5)
	shockwaveRing.BackgroundTransparency = 1
	shockwaveRing.Visible = false
	shockwaveRing.ZIndex = 62
	shockwaveRing.Parent = stage
	Components.addCorner(shockwaveRing, 999)
	local shockwaveStroke = Components.addStroke(shockwaveRing, Color3.fromRGB(255, 255, 255), 4, 0.1)

	local auraRing = Instance.new("Frame")
	auraRing.Name = "AuraRing"
	auraRing.Size = UDim2.fromOffset(180, 180)
	auraRing.Position = UDim2.fromScale(0.5, 0.46)
	auraRing.AnchorPoint = Vector2.new(0.5, 0.5)
	auraRing.BackgroundColor3 = Color3.fromRGB(255, 196, 68)
	auraRing.BackgroundTransparency = 0.72
	auraRing.BorderSizePixel = 0
	auraRing.ZIndex = 62
	auraRing.Parent = stage
	Components.addCorner(auraRing, 999)
	local auraStroke = Components.addStroke(auraRing, Color3.fromRGB(255, 224, 128), 3, 0.15)

	local rayCross = Instance.new("Frame")
	rayCross.Name = "RayCross"
	rayCross.Size = UDim2.fromOffset(320, 14)
	rayCross.Position = UDim2.fromScale(0.5, 0.46)
	rayCross.AnchorPoint = Vector2.new(0.5, 0.5)
	rayCross.BackgroundColor3 = Color3.fromRGB(255, 224, 128)
	rayCross.BackgroundTransparency = 0.45
	rayCross.BorderSizePixel = 0
	rayCross.ZIndex = 62
	rayCross.Parent = stage
	Components.addCorner(rayCross, 8)

	local rayCrossVert = Instance.new("Frame")
	rayCrossVert.Name = "RayCrossVert"
	rayCrossVert.Size = UDim2.fromOffset(14, 320)
	rayCrossVert.Position = UDim2.fromScale(0.5, 0.46)
	rayCrossVert.AnchorPoint = Vector2.new(0.5, 0.5)
	rayCrossVert.BackgroundColor3 = Color3.fromRGB(255, 224, 128)
	rayCrossVert.BackgroundTransparency = 0.45
	rayCrossVert.BorderSizePixel = 0
	rayCrossVert.ZIndex = 62
	rayCrossVert.Parent = stage
	Components.addCorner(rayCrossVert, 8)

	local rayCrossCounter = Instance.new("Frame")
	rayCrossCounter.Name = "RayCrossCounter"
	rayCrossCounter.Size = UDim2.fromOffset(250, 10)
	rayCrossCounter.Position = UDim2.fromScale(0.5, 0.46)
	rayCrossCounter.AnchorPoint = Vector2.new(0.5, 0.5)
	rayCrossCounter.Rotation = 45
	rayCrossCounter.BackgroundColor3 = Color3.fromRGB(255, 255, 255)
	rayCrossCounter.BackgroundTransparency = 0.55
	rayCrossCounter.BorderSizePixel = 0
	rayCrossCounter.Visible = false
	rayCrossCounter.ZIndex = 62
	rayCrossCounter.Parent = stage
	Components.addCorner(rayCrossCounter, 6)

	local contextPill = Components.makeLabel(
		stage,
		"ContextPill",
		"Banner: Standard  |  Weather: Clear Skies",
		UDim2.new(0.92, 0, 0, 24),
		UDim2.new(0.5, 0, 0, 18),
		Theme.FONT_BODY,
		12,
		Color3.fromRGB(200, 214, 242),
		Enum.TextXAlignment.Center
	)
	contextPill.AnchorPoint = Vector2.new(0.5, 0)
	contextPill.ZIndex = 65

	local bannerTitle = Components.makeLabel(
		stage,
		"BannerTitle",
		"SUMMONING...",
		UDim2.new(0.92, 0, 0, 38),
		UDim2.new(0.5, 0, 0, 44),
		Theme.FONT_DISPLAY,
		26,
		Color3.fromRGB(255, 255, 255),
		Enum.TextXAlignment.Center
	)
	bannerTitle.AnchorPoint = Vector2.new(0.5, 0)
	bannerTitle.ZIndex = 65

	local oddsBanner = Components.makeLabel(
		stage,
		"OddsBanner",
		"",
		UDim2.new(0.9, 0, 0, 22),
		UDim2.new(0.5, 0, 0, 82),
		Theme.FONT_MONO,
		13,
		Color3.fromRGB(245, 185, 66),
		Enum.TextXAlignment.Center
	)
	oddsBanner.AnchorPoint = Vector2.new(0.5, 0)
	oddsBanner.ZIndex = 65

	local cardsHolder = Instance.new("Frame")
	cardsHolder.Name = "CardsHolder"
	cardsHolder.Size = UDim2.fromScale(0.92, 0.64)
	cardsHolder.Position = UDim2.fromScale(0.5, 0.50)
	cardsHolder.AnchorPoint = Vector2.new(0.5, 0.5)
	cardsHolder.BackgroundTransparency = 1
	cardsHolder.ZIndex = 64
	cardsHolder.Parent = stage

	local statusPill = Components.makeLabel(
		stage,
		"StatusPill",
		"",
		UDim2.new(0.9, 0, 0, 24),
		UDim2.new(0.5, 0, 1, -86),
		Theme.FONT_BODY,
		13,
		Color3.fromRGB(190, 232, 198),
		Enum.TextXAlignment.Center
	)
	statusPill.AnchorPoint = Vector2.new(0.5, 0)
	statusPill.ZIndex = 65

	local actionBtn = Components.makeButton(
		stage,
		"ActionButton",
		"Skip",
		UDim2.fromOffset(180, 46),
		UDim2.new(0.5, -90, 1, -56),
		Theme.getPalette().accentBlue,
		Color3.fromRGB(255, 255, 255)
	)
	actionBtn.ZIndex = 66

	local runToken = 0
	local playing = false
	local skipped = false
	local activeFinishCallback: (() -> ())? = nil

	local function clearCards()
		for _, child in cardsHolder:GetChildren() do
			child:Destroy()
		end
	end

	local function finishReveal()
		runToken += 1
		playing = false
		skipped = true
		overlay.Visible = false
		lightPillar.Visible = false
		shockwaveRing.Visible = false
		rayCrossCounter.Visible = false
		stage.Position = UDim2.fromOffset(0, 0)
		clearCards()
		local cb = activeFinishCallback
		activeFinishCallback = nil
		if cb then
			cb()
		end
	end

	local function addCardOverlays(cardFrame: Frame, res: RollCardResult, rStyle: Theme.RarityStyle)
		local palette = Theme.getPalette()
		local oddsText = if typeof(res.odds) == "number" and res.odds > 0
			then Components.formatOdds(res.odds)
			else rStyle.oddsLabel

		local oddsChip = Instance.new("Frame")
		oddsChip.Name = "OddsChip"
		oddsChip.Size = UDim2.fromOffset(82, 20)
		oddsChip.Position = UDim2.new(1, -88, 0, 8)
		oddsChip.BackgroundColor3 = rStyle.bgTint
		oddsChip.BorderSizePixel = 0
		oddsChip.ZIndex = cardFrame.ZIndex + 4
		oddsChip.Parent = cardFrame
		Components.addCorner(oddsChip, 6)
		Components.addStroke(oddsChip, rStyle.color, 1, 0.2)

		local oddsLbl = Components.makeLabel(
			oddsChip,
			"OddsText",
			oddsText,
			UDim2.fromScale(1, 1),
			UDim2.fromOffset(0, 0),
			Theme.FONT_MONO,
			10,
			rStyle.glow,
			Enum.TextXAlignment.Center
		)
		oddsLbl.ZIndex = oddsChip.ZIndex + 1

		if res.isNew then
			local newRibbon = Instance.new("Frame")
			newRibbon.Name = "NewRibbon"
			newRibbon.Size = UDim2.fromOffset(52, 20)
			newRibbon.Position = UDim2.fromOffset(8, 34)
			newRibbon.BackgroundColor3 = palette.accentGreen
			newRibbon.BorderSizePixel = 0
			newRibbon.ZIndex = cardFrame.ZIndex + 5
			newRibbon.Parent = cardFrame
			Components.addCorner(newRibbon, 6)
			Components.makeLabel(
				newRibbon,
				"Text",
				"NEW!",
				UDim2.fromScale(1, 1),
				UDim2.fromOffset(0, 0),
				Theme.FONT_DISPLAY,
				10,
				Color3.fromRGB(255, 255, 255),
				Enum.TextXAlignment.Center
			).ZIndex = newRibbon.ZIndex
				+ 1
		end

		local pillY = if res.isNew then 58 else 34
		if res.mutation and res.mutation ~= "" and res.mutation ~= "Normal" then
			local mutStyle = Theme.getMutation(res.mutation)
			local mutPill = Instance.new("Frame")
			mutPill.Name = "MutationPill"
			mutPill.Size = UDim2.fromOffset(86, 18)
			mutPill.Position = UDim2.fromOffset(8, pillY)
			mutPill.BackgroundColor3 = mutStyle.color
			mutPill.BorderSizePixel = 0
			mutPill.ZIndex = cardFrame.ZIndex + 4
			mutPill.Parent = cardFrame
			Components.addCorner(mutPill, 5)
			local mutGrad = Instance.new("UIGradient")
			mutGrad.Color = ColorSequence.new(mutStyle.color, mutStyle.secondary)
			mutGrad.Rotation = 25
			mutGrad.Parent = mutPill
			Components.makeLabel(
				mutPill,
				"Text",
				string.upper(res.mutation),
				UDim2.fromScale(1, 1),
				UDim2.fromOffset(0, 0),
				Theme.FONT_MONO,
				9,
				Color3.fromRGB(255, 255, 255),
				Enum.TextXAlignment.Center
			).ZIndex = mutPill.ZIndex
				+ 1
			pillY += 22
		end

		if res.trait and res.trait ~= "" then
			local traitPill = Instance.new("Frame")
			traitPill.Name = "TraitPill"
			traitPill.Size = UDim2.fromOffset(86, 18)
			traitPill.Position = UDim2.fromOffset(8, pillY)
			traitPill.BackgroundColor3 = palette.bgMuted
			traitPill.BorderSizePixel = 0
			traitPill.ZIndex = cardFrame.ZIndex + 4
			traitPill.Parent = cardFrame
			Components.addCorner(traitPill, 5)
			Components.addStroke(traitPill, palette.accentGold, 1, 0.3)
			Components.makeLabel(
				traitPill,
				"Text",
				res.trait,
				UDim2.fromScale(1, 1),
				UDim2.fromOffset(0, 0),
				Theme.FONT_MONO,
				9,
				palette.accentGold,
				Enum.TextXAlignment.Center
			).ZIndex = traitPill.ZIndex
				+ 1
		end
	end

	local function popCardIn(cardFrame: Frame, delaySec: number, reducedMotion: boolean)
		if reducedMotion then
			return
		end
		local uiScale = Instance.new("UIScale")
		uiScale.Scale = 0.72
		uiScale.Parent = cardFrame
		task.delay(delaySec, function()
			if cardFrame.Parent then
				TweenService
					:Create(
						uiScale,
						TweenInfo.new(0.24, Enum.EasingStyle.Back, Enum.EasingDirection.Out),
						{ Scale = 1 }
					)
					:Play()
			end
		end)
	end

	local function renderResults(results: { RollCardResult }, highestRarity: Theme.RarityStyle, reducedMotion: boolean)
		clearCards()

		local dramatic = DRAMATIC_BANNERS[highestRarity.order]
		bannerTitle.Text = dramatic or (string.upper(highestRarity.name) .. " PULL!")
		bannerTitle.TextColor3 = highestRarity.glow
		oddsBanner.Text = highestRarity.oddsLabel
		oddsBanner.TextColor3 = highestRarity.color

		if #results == 1 then
			local res = results[1]
			local rStyle = Theme.getRarity(res.rarity)
			local cardView =
				Components.makeCardView(cardsHolder, res :: any, UDim2.fromOffset(220, 290), UDim2.fromScale(0.5, 0.5))
			cardView.AnchorPoint = Vector2.new(0.5, 0.5)
			cardView.ZIndex = 65
			addCardOverlays(cardView, res, rStyle)
			popCardIn(cardView, 0, reducedMotion)
		else
			-- Batch Roll x10 Highlight Summary: featured rarest pull on top + scrollable mini-grid
			local bestIdx = 1
			local bestOrder = 0
			for i, res in results do
				local rStyle = Theme.getRarity(res.rarity)
				if rStyle.order > bestOrder then
					bestOrder = rStyle.order
					bestIdx = i
				end
			end
			local bestRes = results[bestIdx]
			local bestStyle = Theme.getRarity(bestRes.rarity)

			local heroCard = Components.makeCardView(
				cardsHolder,
				bestRes :: any,
				UDim2.fromOffset(186, 210),
				UDim2.new(0.5, 0, 0, 4)
			)
			heroCard.AnchorPoint = Vector2.new(0.5, 0)
			heroCard.ZIndex = 66
			addCardOverlays(heroCard, bestRes, bestStyle)
			popCardIn(heroCard, 0, reducedMotion)

			local gridScroll = Instance.new("ScrollingFrame")
			gridScroll.Name = "BatchSummaryGrid"
			gridScroll.Size = UDim2.new(1, 0, 1, -222)
			gridScroll.Position = UDim2.fromOffset(0, 222)
			gridScroll.BackgroundTransparency = 1
			gridScroll.BorderSizePixel = 0
			gridScroll.CanvasSize = UDim2.fromOffset(0, 0)
			gridScroll.AutomaticCanvasSize = Enum.AutomaticSize.Y
			gridScroll.ScrollBarThickness = 6
			gridScroll.ZIndex = 65
			gridScroll.Parent = cardsHolder

			local grid = Instance.new("UIGridLayout")
			grid.CellSize = UDim2.fromOffset(124, 152)
			grid.CellPadding = UDim2.fromOffset(8, 8)
			grid.HorizontalAlignment = Enum.HorizontalAlignment.Center
			grid.SortOrder = Enum.SortOrder.LayoutOrder
			grid.Parent = gridScroll

			for idx, res in results do
				local rStyle = Theme.getRarity(res.rarity)
				local mini =
					Components.makeCardView(gridScroll, res :: any, UDim2.fromOffset(124, 152), UDim2.fromOffset(0, 0))
				mini.LayoutOrder = idx
				mini.ZIndex = 65
				addCardOverlays(mini, res, rStyle)
				popCardIn(mini, math.min(idx * 0.03, 0.28), reducedMotion)
			end
		end

		local newCount = 0
		local soldYen = 0
		for _, res in results do
			if res.isNew then
				newCount += 1
			end
			if res.autoSold and (res.soldYen or 0) > 0 then
				soldYen += res.soldYen or 0
			end
		end

		local notes: { string } = {}
		if newCount > 0 then
			table.insert(notes, string.format("New Cards: %d", newCount))
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
			rayCrossVert.BackgroundColor3 = highestRarity.glow
			rayCrossCounter.BackgroundColor3 = highestRarity.color
			lightPillar.BackgroundColor3 = highestRarity.glow
			shockwaveStroke.Color = highestRarity.glow

			if skipBuildup then
				skipped = true
				if isAuto then
					finishReveal()
					return
				end
			end

			overlay.Visible = true
			cardsHolder.Visible = false
			statusPill.Text = ""
			actionBtn.Text = "Skip"
			lightPillar.Visible = false
			shockwaveRing.Visible = false
			rayCrossCounter.Visible = false

			emitSfx("summon_roll")

			if not skipBuildup then
				bannerTitle.Text = if highestOrder >= 5 then "FATE CONVERGING..." else "SUMMONING..."
				bannerTitle.TextColor3 = Color3.fromRGB(244, 247, 255)
				oddsBanner.Text = highestRarity.oddsLabel
				oddsBanner.TextColor3 = highestRarity.color

				local waitSeconds = if highestOrder >= 5 then 1.05 elseif highestOrder >= 3 then 0.60 else 0.30
				if not settings.reducedMotion then
					auraRing.Size = UDim2.fromOffset(72, 72)
					rayCross.Rotation = 0
					rayCrossVert.Rotation = 0
					TweenService:Create(
						auraRing,
						TweenInfo.new(waitSeconds * 0.75, Enum.EasingStyle.Back, Enum.EasingDirection.Out),
						{
							Size = UDim2.fromOffset(
								if highestOrder >= 5 then 310 else 250,
								if highestOrder >= 5 then 310 else 250
							),
						}
					):Play()

					if highestOrder >= 3 then
						TweenService:Create(
							rayCross,
							TweenInfo.new(waitSeconds, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Rotation = 135 }
						):Play()
						TweenService:Create(
							rayCrossVert,
							TweenInfo.new(waitSeconds, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Rotation = 135 }
						):Play()
					end

					if highestOrder >= 5 then
						lightPillar.Visible = true
						lightPillar.Size = UDim2.new(0, 12, 1, 0)
						lightPillar.BackgroundTransparency = 0.25
						TweenService:Create(
							lightPillar,
							TweenInfo.new(waitSeconds * 0.85, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Size = UDim2.new(0, 180, 1, 0), BackgroundTransparency = 0.5 }
						):Play()

						rayCrossCounter.Visible = true
						rayCrossCounter.Rotation = 45
						TweenService:Create(
							rayCrossCounter,
							TweenInfo.new(waitSeconds, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Rotation = -135 }
						):Play()

						shockwaveRing.Visible = true
						shockwaveRing.Size = UDim2.fromOffset(60, 60)
						shockwaveStroke.Transparency = 0.1
						TweenService:Create(
							shockwaveRing,
							TweenInfo.new(waitSeconds * 0.9, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Size = UDim2.fromOffset(480, 480) }
						):Play()
						TweenService:Create(
							shockwaveStroke,
							TweenInfo.new(waitSeconds * 0.9, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
							{ Transparency = 1 }
						):Play()
					end
				end

				local elapsed = 0
				while elapsed < waitSeconds and not skipped and runToken == myToken do
					elapsed += task.wait(0.05)
				end
				if runToken ~= myToken then
					return
				end
			end

			skipped = true
			lightPillar.Visible = false
			shockwaveRing.Visible = false
			rayCrossCounter.Visible = false
			cardsHolder.Visible = true
			renderResults(results, highestRarity, settings.reducedMotion)
			actionBtn.Text = "Continue"

			if highestOrder >= 5 then
				emitSfx("legendary_drop")
			else
				emitSfx("card_flip")
			end

			if not settings.reducedFlash and highestOrder >= 3 then
				flashFrame.BackgroundColor3 = highestRarity.glow
				flashFrame.BackgroundTransparency = if highestOrder >= 5 then 0.28 else 0.55
				TweenService:Create(
					flashFrame,
					TweenInfo.new(0.35, Enum.EasingStyle.Quad, Enum.EasingDirection.Out),
					{ BackgroundTransparency = 1 }
				):Play()
			end

			if not settings.reducedMotion and highestOrder >= 5 then
				task.spawn(function()
					for i = 1, 7 do
						if runToken ~= myToken or not playing then
							break
						end
						local amp = if highestOrder >= 7 then 12 else 7
						local dx = if i % 2 == 0 then amp else -amp
						stage.Position = UDim2.fromOffset(dx, (i % 3 - 1) * 4)
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
		setContext = function(bannerName: string, weatherText: string)
			contextPill.Text = string.format("Banner: %s  |  Weather: %s", bannerName, weatherText)
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
