--!strict
export type MutationId = "Normal" | "Shiny" | "Golden" | "Rainbow" | "Void"

export type MutationDef = {
	id: MutationId,
	name: string,
	order: number,
	mult: number,
	baseWeight: number,
	badge: string,
	colorHex: string,
	colorRgb: { number },
}

local Mutations = {}

Mutations.Order = { "Normal", "Shiny", "Golden", "Rainbow", "Void" } :: { MutationId }

Mutations.ById = {
	Normal = {
		id = "Normal",
		name = "Normal",
		order = 1,
		mult = 1.0,
		baseWeight = 0.9200,
		badge = "NORM",
		colorHex = "#CBD5E1",
		colorRgb = { 203, 213, 225 },
	},
	Shiny = {
		id = "Shiny",
		name = "Shiny",
		order = 2,
		mult = 1.5,
		baseWeight = 0.0550,
		badge = "SHINY",
		colorHex = "#38BDF8",
		colorRgb = { 56, 189, 248 },
	},
	Golden = {
		id = "Golden",
		name = "Golden",
		order = 3,
		mult = 2.25,
		baseWeight = 0.0180,
		badge = "GOLD",
		colorHex = "#FACC15",
		colorRgb = { 250, 204, 21 },
	},
	Rainbow = {
		id = "Rainbow",
		name = "Rainbow",
		order = 4,
		mult = 4.0,
		baseWeight = 0.0060,
		badge = "PRISM",
		colorHex = "#F472B6",
		colorRgb = { 244, 114, 182 },
	},
	Void = {
		id = "Void",
		name = "Void",
		order = 5,
		mult = 7.5,
		baseWeight = 0.0010,
		badge = "VOID",
		colorHex = "#7C3AED",
		colorRgb = { 124, 58, 237 },
	},
} :: { [MutationId]: MutationDef }

function Mutations.get(id: string): MutationDef
	return Mutations.ById[id :: MutationId] or Mutations.ById.Normal
end

return Mutations
