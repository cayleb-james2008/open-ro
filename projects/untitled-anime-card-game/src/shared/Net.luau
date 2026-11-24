--!strict
local Net = {}

Net.FolderName = "UACGRemotes"
Net.RequestRemoteName = "Request"
Net.SyncRemoteName = "StateSync"
Net.EventRemoteName = "PushEvent"

export type RequestAction =
	"GetState"
	| "Roll"
	| "RollBatch"
	| "SetDeck"
	| "ToggleLock"
	| "QuickSell"
	| "SellCard"
	| "LevelUpCard"
	| "AscendCard"
	| "RerollTrait"
	| "StartBattle"
	| "UnlockWorld"
	| "RedeemCode"
	| "UsePotion"
	| "ClaimDaily"
	| "Rebirth"
	| "UpdateSettings"

export type RequestPayload = {
	action: RequestAction,
	worldId: number?,
	useTicket: boolean?,
	fastRoll: boolean?,
	count: number?,
	deckIds: { string }?,
	defenseDeckIds: { string }?,
	instanceId: string?,
	instanceIds: { string }?,
	stageId: string?,
	code: string?,
	potionId: string?,
	settings: { [string]: any }?,
}

export type ResponsePayload = {
	ok: boolean,
	error: string?,
	data: any?,
}

function Net.getOrCreateRemotes(replicatedStorage: Instance): (RemoteFunction, RemoteEvent, RemoteEvent)
	local existing = replicatedStorage:FindFirstChild(Net.FolderName)
	local folder: Folder
	if existing and existing:IsA("Folder") then
		folder = existing
	else
		folder = Instance.new("Folder")
		folder.Name = Net.FolderName
		folder.Parent = replicatedStorage
	end

	local req = folder:FindFirstChild(Net.RequestRemoteName)
	local requestRemote: RemoteFunction
	if req and req:IsA("RemoteFunction") then
		requestRemote = req
	else
		requestRemote = Instance.new("RemoteFunction")
		requestRemote.Name = Net.RequestRemoteName
		requestRemote.Parent = folder
	end

	local sync = folder:FindFirstChild(Net.SyncRemoteName)
	local syncRemote: RemoteEvent
	if sync and sync:IsA("RemoteEvent") then
		syncRemote = sync
	else
		syncRemote = Instance.new("RemoteEvent")
		syncRemote.Name = Net.SyncRemoteName
		syncRemote.Parent = folder
	end

	local push = folder:FindFirstChild(Net.EventRemoteName)
	local pushRemote: RemoteEvent
	if push and push:IsA("RemoteEvent") then
		pushRemote = push
	else
		pushRemote = Instance.new("RemoteEvent")
		pushRemote.Name = Net.EventRemoteName
		pushRemote.Parent = folder
	end

	return requestRemote, syncRemote, pushRemote
end

function Net.waitForRemotes(
	replicatedStorage: Instance,
	timeoutSec: number?
): (RemoteFunction?, RemoteEvent?, RemoteEvent?)
	local waitTime = timeoutSec or 10
	local folder = replicatedStorage:WaitForChild(Net.FolderName, waitTime)
	if not folder then
		return nil, nil, nil
	end
	local req = folder:WaitForChild(Net.RequestRemoteName, waitTime)
	local sync = folder:WaitForChild(Net.SyncRemoteName, waitTime)
	local push = folder:WaitForChild(Net.EventRemoteName, waitTime)
	if req and req:IsA("RemoteFunction") and sync and sync:IsA("RemoteEvent") and push and push:IsA("RemoteEvent") then
		return req, sync, push
	end
	return nil, nil, nil
end

return Net
