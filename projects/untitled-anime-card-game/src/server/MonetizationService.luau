--!strict
local MarketplaceService = game:GetService("MarketplaceService")
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Shared = ReplicatedStorage:WaitForChild("Shared")
local ConfigFolder = Shared:WaitForChild("Config")
local Products = require(ConfigFolder:WaitForChild("Products"))

local MonetizationService = {}

local GROUP_ID = 0 -- Set to published Roblox Group ID when available
local initialized = false
local onGrantCallback: ((Player, { [string]: any }, string) -> ())?
local profileLookup: ((Player) -> { [string]: any }?)?

local function ensureTables(profile: { [string]: any })
	if type(profile.processedReceipts) ~= "table" then
		profile.processedReceipts = {}
	end
	if type(profile.ownedPasses) ~= "table" then
		profile.ownedPasses = {}
	end
	if type(profile.potions) ~= "table" then
		profile.potions = {}
	end
end

local function applyDevProduct(profile: { [string]: any }, productId: string): boolean
	if productId == "prod_starter_bundle" then
		profile.gems = (profile.gems or 0) + 2500
		profile.tickets = (profile.tickets or 0) + 25
		profile.yen = (profile.yen or 0) + 10000
		profile.potions.Major = (profile.potions.Major or 0) + 2
	elseif productId == "prod_gems_pouch" then
		profile.gems = (profile.gems or 0) + 1200
	elseif productId == "prod_gems_chest" then
		profile.gems = (profile.gems or 0) + 3500
	elseif productId == "prod_gems_vault" then
		profile.gems = (profile.gems or 0) + 8000
	elseif productId == "prod_gems_treasury" then
		profile.gems = (profile.gems or 0) + 20000
	elseif productId == "prod_tickets_15" then
		profile.tickets = (profile.tickets or 0) + 15
	elseif productId == "prod_tickets_50" then
		profile.tickets = (profile.tickets or 0) + 50
	elseif productId == "prod_tickets_150" then
		profile.tickets = (profile.tickets or 0) + 150
	elseif productId == "prod_potions_alchemist" then
		profile.potions.Major = (profile.potions.Major or 0) + 3
		profile.potions.Super = (profile.potions.Super or 0) + 1
	elseif productId == "prod_potions_boss" then
		profile.potions.Super = (profile.potions.Super or 0) + 3
		profile.potions.Boss = (profile.potions.Boss or 0) + 2
	elseif productId == "prod_trait_crystals_25" then
		profile.traitCrystals = (profile.traitCrystals or 0) + 25
	elseif productId == "prod_season_pass_s1" then
		profile.seasonPassPremium = true
	else
		return false
	end
	return true
end

function MonetizationService.grantProduct(
	player: Player?,
	profile: { [string]: any },
	productId: string,
	receiptId: string?
): (boolean, string?)
	local entry = Products.getById(productId)
	if not entry then
		return false, "Unknown product."
	end
	ensureTables(profile)

	if receiptId and receiptId ~= "" then
		if profile.processedReceipts[receiptId] then
			return true, nil
		end
		profile.processedReceipts[receiptId] = true
	end

	if entry.kind == "Gamepass" then
		profile.ownedPasses[productId] = true
	else
		local ok = applyDevProduct(profile, productId)
		if not ok then
			return false, "Unhandled dev product."
		end
	end

	if player and onGrantCallback then
		onGrantCallback(player, profile, productId)
	end
	return true, nil
end

function MonetizationService.promptPurchase(
	player: Player,
	profile: { [string]: any },
	productId: string
): (boolean, string?, { [string]: any }?)
	local entry = Products.getById(productId)
	if not entry then
		return false, "Unknown product.", nil
	end
	local assetId = entry.robloxId or 0
	if assetId == 0 or not Products.isPurchaseEnabled(productId) then
		if RunService:IsStudio() or game.GameId == 0 then
			local testReceipt = string.format("studio_%s_%d", productId, os.time())
			local ok, err = MonetizationService.grantProduct(player, profile, productId, testReceipt)
			if not ok then
				return false, err, nil
			end
			return true, nil, { granted = true, testMode = true, productId = productId }
		end
		return false, "Product not enabled for live sale yet.", nil
	end

	local okPrompt = pcall(function()
		if entry.kind == "Gamepass" then
			MarketplaceService:PromptGamePassPurchase(player, assetId)
		else
			MarketplaceService:PromptProductPurchase(player, assetId)
		end
	end)
	if not okPrompt then
		return false, "Could not open purchase prompt.", nil
	end
	return true, nil, { prompted = true, productId = productId }
end

function MonetizationService.getPerks(player: Player?, profile: { [string]: any }): { [string]: any }
	ensureTables(profile)
	local passes = profile.ownedPasses
	local isPremium = false
	local inGroup = false
	if player then
		isPremium = player.MembershipType == Enum.MembershipType.Premium
		if GROUP_ID > 0 then
			local ok, res = pcall(function()
				return player:IsInGroup(GROUP_ID)
			end)
			inGroup = ok and res == true
		end
	end

	local passLuckMult = 1.0
	if passes.pass_lucky then
		passLuckMult *= 1.5
	end
	if passes.pass_2x_luck then
		passLuckMult *= 2.0
	end
	if passes.pass_mega_lucky then
		passLuckMult *= 3.0
	end
	if passes.pass_vip then
		passLuckMult *= 1.2
	end

	local premiumBonus = if isPremium then 0.15 else 0
	local groupBonus = if inGroup then 1.0 else 0
	local yenMult = (if isPremium then 1.15 else 1.0) * (if passes.pass_vip then 1.25 else 1.0)

	return {
		isPremium = isPremium,
		inGroup = inGroup,
		premiumBonus = premiumBonus,
		groupBonus = groupBonus,
		passLuckMult = passLuckMult,
		yenMult = yenMult,
		extraStorage = if passes.pass_extra_storage then 150 else 0,
		extraDeckSlots = if passes.pass_extra_deck_slots then 2 else 0,
		hasFastRoll = passes.pass_fast_roll == true,
		hasAutoRoll = passes.pass_auto_roll == true,
		hasMultiRoll = passes.pass_multi_roll == true,
		hasVip = passes.pass_vip == true,
		hasMutationHunter = passes.pass_mutation_hunter == true,
	}
end

function MonetizationService.getShopStatus(player: Player, profile: { [string]: any }): { [string]: any }
	return {
		gamepasses = Products.Gamepasses,
		devProducts = Products.DevProducts,
		ownedPasses = profile.ownedPasses or {},
		perks = MonetizationService.getPerks(player, profile),
	}
end

function MonetizationService.init(
	getProfileFn: (Player) -> { [string]: any }?,
	onGrantFn: ((Player, { [string]: any }, string) -> ())?
)
	if initialized then
		return
	end
	initialized = true
	profileLookup = getProfileFn
	onGrantCallback = onGrantFn

	pcall(function()
		MarketplaceService.ProcessReceipt = function(receiptInfo)
			local player = Players:GetPlayerByUserId(receiptInfo.PlayerId)
			if not player or not profileLookup then
				return Enum.ProductPurchaseDecision.NotProcessedYet
			end
			local profile = profileLookup(player)
			if not profile then
				return Enum.ProductPurchaseDecision.NotProcessedYet
			end
			for id, entry in pairs(Products.DevProducts) do
				if entry.robloxId == receiptInfo.ProductId then
					local ok = MonetizationService.grantProduct(player, profile, id, tostring(receiptInfo.PurchaseId))
					return if ok
						then Enum.ProductPurchaseDecision.PurchaseGranted
						else Enum.ProductPurchaseDecision.NotProcessedYet
				end
			end
			return Enum.ProductPurchaseDecision.NotProcessedYet
		end
	end)
end

return MonetizationService
