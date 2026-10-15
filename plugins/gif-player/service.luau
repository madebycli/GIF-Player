--!nonstrict
local exe = noctalia.getConfig("executable")
local queue = {}
local busy = false
local started = false
local frontendNonce = nil
local lastAppearance = nil
local libraryDirectory = nil
local pump
local function send(command, callback)
    local encoded = noctalia.json.encode(command)
    if encoded == nil then return end
    local accepted = noctalia.runAsync({ exe, "request", encoded }, function(result)
        local data = result and noctalia.json.decode(result.stdout or "")
        if type(data) == "table" and data.error then
            noctalia.state.set("error", data.error)
        elseif not result or result.exitCode ~= 0 then
            noctalia.state.set("error", result and result.stderr or "IPC failed")
        else
            noctalia.state.set("error", "")
        end
        if callback then callback(data) end
    end, 6000)
    if not accepted and callback then callback(nil) end
end
local function refreshLibrary()
    send({ action = "library" }, function(data)
        noctalia.state.set("library", data and data.files or {})
    end)
end
pump = function()
    if busy or #queue == 0 then return end
    busy = true
    local command = table.remove(queue, 1)
    send(command, function()
        busy = false
        if command.action == "library-directory" then refreshLibrary() end
        pump()
    end)
end
noctalia.state.watch("command", function(message)
    if type(message) ~= "table" or type(message.command) ~= "table" then return end
    local command = message.command
    if command.action == "library" then refreshLibrary(); return end
    -- Coalesce pending slider changes without dropping discrete button actions.
    local coalesced = false
    if command.action == "scale" or command.action == "opacity" or command.action == "speed" or command.action == "jump-rate" then
        for i = #queue, 1, -1 do
            if queue[i].action == command.action and queue[i].id == command.id then
                queue[i] = command
                coalesced = true
                break
            end
        end
    end
    if not coalesced then table.insert(queue, command) end
    pump()
end)
local function quote(s)
    return "'" .. tostring(s):gsub("'", "'\\''") .. "'"
end
local function start()
    if started then return end
    if not noctalia.commandExists(exe) then
        noctalia.state.set("error", noctalia.tr("missing_engine"))
        return
    end
    started = noctalia.runStream(quote(exe) .. " watch --reconnect --frontend", function(line)
        local data = noctalia.json.decode(line)
        if type(data) == "table" then
            noctalia.state.set("connected", data.ok == true)
            if data.ok then
                if not data.settings or not data.settings.appearance then lastAppearance = nil end
                noctalia.state.set("snapshot", data)
                if data.collection_dir and data.collection_dir ~= libraryDirectory then libraryDirectory = data.collection_dir; refreshLibrary() end
                noctalia.state.set("error", data.error or "")
                if data.frontend_request and data.frontend_request.nonce ~= frontendNonce then
                    frontendNonce = data.frontend_request.nonce
                    noctalia.state.set("selected", data.frontend_request.id)
                    if not noctalia.state.get("panel-open-control") then
                        noctalia.togglePanel("madebycli/gif-player:control")
                    end
                    send({ action = "frontend-ack", nonce = frontendNonce })
                end
            else
                noctalia.state.set("error", data.error or "")
            end
        end
    end)
    -- Start only when the user opens a panel or issues a command; the bar itself stays idle.
end
local function appearance()
    local palette = {}
    for _, role in ipairs({"surface", "surface_variant", "on_surface", "on_surface_variant", "primary", "on_primary", "hover", "outline"}) do palette[role] = noctalia.getColor(role) end
    local shell = noctalia.getSetting("shell") or {}
    local value = {action = "appearance", palette = palette, font_family = shell.font_family or "sans-serif"}
    local encoded = noctalia.json.encode(value)
    if encoded ~= lastAppearance and noctalia.state.get("connected") then lastAppearance = encoded; send(value) end
end
function update()
    noctalia.setUpdateInterval(1000)
    appearance()
    start()
end
function onIpc(event)
    if event == "refresh" then refreshLibrary() end
end
noctalia.state.watch("start", function(value)
    if not value then return end
    noctalia.runAsync({ exe, "library" }, function(result)
        local data = result and noctalia.json.decode(result.stdout or "")
        if data and data.ok then
            noctalia.state.set("library", data.files or {})
            send({ action = "snapshot" }, function(state)
                if state and state.ok then noctalia.state.set("connected", true); noctalia.state.set("snapshot", state); appearance() end
            end)
            local directory = noctalia.getConfig("directory")
            if directory and directory ~= "" then
                send({ action = "library-directory", path = noctalia.expandPath(directory) }, refreshLibrary)
            end
        else
            noctalia.state.set("error", result and result.stderr or noctalia.tr("missing_engine"))
        end
    end, 10000)
end)
start()
