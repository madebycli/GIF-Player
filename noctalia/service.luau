--!nonstrict
-- Single Noctalia integration point for GIF Player daemon status.
-- The migration CLI exposes a long-lived JSONL stream. Panels/widgets consume
-- only noctalia.state so they never create independent polling loops.

local lastPayload = ""
local lastLineMs = 0
local streamStarted = false
local fallbackPending = false

local function publish(payload)
  if type(payload) ~= "table" then return end
  local widgets = type(payload.widgets) == "table" and payload.widgets or {}
  local online = payload.ok == true
  local errorMessage = online and "" or tostring(payload.error or "daemon offline")

  local encoded = noctalia.json.encode({ online = online, widgets = widgets, error = errorMessage })
  if encoded == nil or encoded == lastPayload then
    lastLineMs = noctalia.nowMs()
    return
  end

  lastPayload = encoded
  lastLineMs = noctalia.nowMs()
  noctalia.state.set("daemon_online", online)
  noctalia.state.set("widgets", widgets)
  noctalia.state.set("widget_count", #widgets)
  noctalia.state.set("daemon_error", errorMessage)
  noctalia.state.set("bridge_generation", (noctalia.state.get("bridge_generation") or 0) + 1)
end

local function decodeLine(line)
  local payload, err = noctalia.json.decode(line or "")
  if payload == nil then
    noctalia.log("gif-player bridge: invalid watch line: " .. tostring(err))
    return
  end
  publish(payload)
end

local function refreshOnce()
  if fallbackPending then return end
  fallbackPending = true
  noctalia.runAsync({ "gif-player", "list", "--json" }, function(result)
    fallbackPending = false
    local payload, err = noctalia.json.decode(result.stdout or "")
    if payload == nil then
      publish({ ok = false, widgets = {}, error = err or result.stderr or "gif-player unavailable" })
      return
    end
    publish(payload)
  end, 5000)
end

local function startStream()
  if streamStarted then return end
  if not noctalia.commandExists("gif-player") then
    publish({ ok = false, widgets = {}, error = "gif-player is not installed" })
    return
  end

  -- runStream currently accepts a command string. This command contains no
  -- user-controlled values, so shell parsing cannot inject configured paths.
  streamStarted = noctalia.runStream("gif-player watch --interval 1.0", decodeLine)
  if not streamStarted then
    refreshOnce()
  end
end

function update()
  noctalia.setUpdateInterval(5000)
  if not streamStarted then
    startStream()
    refreshOnce()
    return
  end

  -- The compatibility watcher emits a heartbeat at least every five seconds.
  -- If it dies during the migration, one cheap fallback refresh keeps the UI
  -- truthful. The final native daemon will make watch event-driven.
  if lastLineMs == 0 or noctalia.nowMs() - lastLineMs > 8000 then
    refreshOnce()
  end
end

function onEnable()
  startStream()
  refreshOnce()
end

startStream()
refreshOnce()
