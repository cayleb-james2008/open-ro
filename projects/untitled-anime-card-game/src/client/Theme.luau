--!strict

export type ThemeMode = "Dark" | "Light"

export type Palette = {
	mode: ThemeMode,
	bgDeep: Color3,
	bgPanel: Color3,
	bgCard: Color3,
	bgMuted: Color3,
	bgOverlay: Color3,
	border: Color3,
	borderStrong: Color3,
	textPrimary: Color3,
	textSecondary: Color3,
	textMuted: Color3,
	textOnAccent: Color3,
	accentGold: Color3,
	accentGoldDark: Color3,
	accentBlue: Color3,
	accentGreen: Color3,
	accentRed: Color3,
	accentPurple: Color3,
	shadow: Color3,
}

export type RarityStyle = {
	name: string,
	shortTag: string,
	shapeKind: string,
	oddsLabel: string,
	color: Color3,
	glow: Color3,
	bgTint: Color3,
	order: number,
}

export type MutationStyle = {
	name: string,
	tag: string,
	multLabel: string,
	color: Color3,
	secondary: Color3,
}

local Theme = {}

Theme.MIN_TOUCH_PX = 44
Theme.FONT_DISPLAY = Enum.Font.GothamBold
Theme.FONT_BODY = Enum.Font.GothamMedium
Theme.FONT_MONO = Enum.Font.GothamBold

local DARK_PALETTE: Palette = {
	mode = "Dark",
	bgDeep = Color3.fromRGB(11, 15, 29),
	bgPanel = Color3.fromRGB(19, 26, 48),
	bgCard = Color3.fromRGB(27, 36, 64),
	bgMuted = Color3.fromRGB(36, 48, 82),
	bgOverlay = Color3.fromRGB(6, 9, 18),
	border = Color3.fromRGB(66, 84, 134),
	borderStrong = Color3.fromRGB(245, 185, 66),
	textPrimary = Color3.fromRGB(244, 247, 255),
	textSecondary = Color3.fromRGB(194, 206, 234),
	textMuted = Color3.fromRGB(142, 158, 194),
	textOnAccent = Color3.fromRGB(18, 22, 36),
	accentGold = Color3.fromRGB(245, 185, 66),
	accentGoldDark = Color3.fromRGB(196, 136, 26),
	accentBlue = Color3.fromRGB(68, 158, 255),
	accentGreen = Color3.fromRGB(54, 204, 132),
	accentRed = Color3.fromRGB(238, 82, 96),
	accentPurple = Color3.fromRGB(168, 108, 255),
	shadow = Color3.fromRGB(4, 6, 14),
}

local LIGHT_PALETTE: Palette = {
	mode = "Light",
	bgDeep = Color3.fromRGB(234, 240, 250),
	bgPanel = Color3.fromRGB(250, 252, 255),
	bgCard = Color3.fromRGB(240, 244, 252),
	bgMuted = Color3.fromRGB(220, 228, 242),
	bgOverlay = Color3.fromRGB(16, 22, 40),
	border = Color3.fromRGB(174, 188, 214),
	borderStrong = Color3.fromRGB(186, 118, 12),
	textPrimary = Color3.fromRGB(18, 25, 46),
	textSecondary = Color3.fromRGB(54, 66, 96),
	textMuted = Color3.fromRGB(88, 102, 134),
	textOnAccent = Color3.fromRGB(255, 255, 255),
	accentGold = Color3.fromRGB(196, 124, 10),
	accentGoldDark = Color3.fromRGB(152, 92, 4),
	accentBlue = Color3.fromRGB(28, 108, 216),
	accentGreen = Color3.fromRGB(22, 142, 82),
	accentRed = Color3.fromRGB(196, 44, 58),
	accentPurple = Color3.fromRGB(118, 58, 204),
	shadow = Color3.fromRGB(146, 160, 188),
}

Theme.Rarities = {
	Common = {
		name = "Common",
		shortTag = "C",
		shapeKind = "Circle",
		oddsLabel = "1-in-2",
		color = Color3.fromRGB(148, 160, 184),
		glow = Color3.fromRGB(180, 192, 214),
		bgTint = Color3.fromRGB(42, 49, 68),
		order = 1,
	},
	Uncommon = {
		name = "Uncommon",
		shortTag = "U",
		shapeKind = "Rounded",
		oddsLabel = "1-in-5",
		color = Color3.fromRGB(58, 204, 124),
		glow = Color3.fromRGB(108, 240, 164),
		bgTint = Color3.fromRGB(24, 66, 48),
		order = 2,
	},
	Rare = {
		name = "Rare",
		shortTag = "R",
		shapeKind = "Diamond",
		oddsLabel = "1-in-20",
		color = Color3.fromRGB(68, 152, 255),
		glow = Color3.fromRGB(122, 192, 255),
		bgTint = Color3.fromRGB(24, 52, 94),
		order = 3,
	},
	Epic = {
		name = "Epic",
		shortTag = "E",
		shapeKind = "Hex",
		oddsLabel = "1-in-100",
		color = Color3.fromRGB(178, 96, 255),
		glow = Color3.fromRGB(214, 156, 255),
		bgTint = Color3.fromRGB(58, 30, 96),
		order = 4,
	},
	Legendary = {
		name = "Legendary",
		shortTag = "L",
		shapeKind = "Star",
		oddsLabel = "1-in-1,000",
		color = Color3.fromRGB(255, 192, 52),
		glow = Color3.fromRGB(255, 226, 120),
		bgTint = Color3.fromRGB(84, 58, 18),
		order = 5,
	},
	Mythic = {
		name = "Mythic",
		shortTag = "M",
		shapeKind = "Flame",
		oddsLabel = "1-in-25,000",
		color = Color3.fromRGB(255, 76, 98),
		glow = Color3.fromRGB(255, 144, 156),
		bgTint = Color3.fromRGB(92, 22, 36),
		order = 6,
	},
	Secret = {
		name = "Secret",
		shortTag = "S",
		shapeKind = "Rift",
		oddsLabel = "1-in-250,000",
		color = Color3.fromRGB(48, 224, 212),
		glow = Color3.fromRGB(132, 250, 240),
		bgTint = Color3.fromRGB(16, 62, 68),
		order = 7,
	},
	Universal = {
		name = "Universal",
		shortTag = "UV",
		shapeKind = "Prism",
		oddsLabel = "1-in-2.5M",
		color = Color3.fromRGB(255, 128, 228),
		glow = Color3.fromRGB(164, 220, 255),
		bgTint = Color3.fromRGB(68, 26, 86),
		order = 8,
	},
	Nexus = {
		name = "Nexus",
		shortTag = "NX",
		shapeKind = "Crown",
		oddsLabel = "1-in-25M",
		color = Color3.fromRGB(255, 232, 138),
		glow = Color3.fromRGB(130, 236, 255),
		bgTint = Color3.fromRGB(48, 42, 88),
		order = 9,
	},
} :: { [string]: RarityStyle }

Theme.RarityOrder = {
	"Common",
	"Uncommon",
	"Rare",
	"Epic",
	"Legendary",
	"Mythic",
	"Secret",
	"Universal",
	"Nexus",
}

Theme.Mutations = {
	Normal = {
		name = "Normal",
		tag = "",
		multLabel = "1.0x",
		color = Color3.fromRGB(170, 180, 200),
		secondary = Color3.fromRGB(120, 130, 150),
	},
	Shiny = {
		name = "Shiny",
		tag = "SHINY",
		multLabel = "1.5x",
		color = Color3.fromRGB(120, 225, 255),
		secondary = Color3.fromRGB(255, 255, 255),
	},
	Golden = {
		name = "Golden",
		tag = "GOLDEN",
		multLabel = "2.25x",
		color = Color3.fromRGB(255, 204, 64),
		secondary = Color3.fromRGB(255, 242, 166),
	},
	Rainbow = {
		name = "Rainbow",
		tag = "RAINBOW",
		multLabel = "4.0x",
		color = Color3.fromRGB(255, 118, 214),
		secondary = Color3.fromRGB(98, 238, 255),
	},
	Void = {
		name = "Void",
		tag = "VOID",
		multLabel = "7.5x",
		color = Color3.fromRGB(186, 82, 255),
		secondary = Color3.fromRGB(36, 18, 56),
	},
} :: { [string]: MutationStyle }

local currentMode: ThemeMode = "Dark"
local listeners: { (Palette) -> () } = {}

function Theme.getMode(): ThemeMode
	return currentMode
end

function Theme.getPalette(): Palette
	if currentMode == "Light" then
		return LIGHT_PALETTE
	end
	return DARK_PALETTE
end

function Theme.normalizeMode(mode: any): ThemeMode
	if type(mode) == "string" and string.lower(mode) == "light" then
		return "Light"
	end
	return "Dark"
end

function Theme.setMode(mode: ThemeMode | string)
	local normalized = Theme.normalizeMode(mode)
	if currentMode == normalized then
		return
	end
	currentMode = normalized
	local palette = Theme.getPalette()
	for _, fn in listeners do
		fn(palette)
	end
end

function Theme.onChanged(fn: (Palette) -> ()): () -> ()
	table.insert(listeners, fn)
	return function()
		local idx = table.find(listeners, fn)
		if idx then
			table.remove(listeners, idx)
		end
	end
end

function Theme.getRarity(rarityName: string?): RarityStyle
	if rarityName and Theme.Rarities[rarityName] then
		return Theme.Rarities[rarityName]
	end
	return Theme.Rarities.Common
end

function Theme.getMutation(mutationName: string?): MutationStyle
	if mutationName and Theme.Mutations[mutationName] then
		return Theme.Mutations[mutationName]
	end
	return Theme.Mutations.Normal
end

return Theme
