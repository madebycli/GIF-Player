--!nonstrict
-- This module is evaluated in each panel's own VM. The host renders native controls.
return function(initialTab, source)
    local tab = initialTab
    local query = ""
    local category = ""
    local scrollRow = 0
    local selected = nil
    local profileName = ""
    local renameName = ""
    local nonce = 0
    local opened = false
    local hoverPath = nil
    local hoverTime = 0
    local hoverFrame = 1
    local previews = {}
    local pendingPreviews = {}
    local closeButton = true
    local snapshot = noctalia.state.get("snapshot") or { widgets = {}, profiles = {}, outputs = {}, settings = {} }
    local render
    local function tr(key) return noctalia.tr(key) end
    local function send(command)
        nonce += 1
        noctalia.state.set("command", { source = source, nonce = nonce, command = command })
    end
    local function button(text, glyph, callback)
        local props = { text = text, onClick = callback, controlSize = "sm" }
        if text == "" then props.width = 32; props.height = 32 end
        if glyph ~= "" then props.glyph = glyph end
        return ui.button(props)
    end
    local function command(action, id, values)
        local cmd = values or {}
        cmd.action = action
        cmd.id = id
        send(cmd)
    end
    local function section(label, node)
        return ui.column({ gap = 8 }, { ui.label({ text = label, color = "on_surface_variant", fontWeight = "medium" }), node })
    end
    local function selectedWidget()
        for _, widget in ipairs(snapshot.widgets or {}) do if widget.id == selected then return widget end end
        selected = snapshot.widgets and snapshot.widgets[1] and snapshot.widgets[1].id or nil
        return snapshot.widgets and snapshot.widgets[1]
    end
    local function hover(path, entered)
        if entered ~= "true" then
            if hoverPath == path then hoverPath = nil; panel.setNeedsFrameTick(false); render() end
            return
        end
        hoverPath = path
        hoverTime = 0
        hoverFrame = 1
        if previews[path] == nil and not pendingPreviews[path] then
            pendingPreviews[path] = true
            local accepted = noctalia.runAsync({ noctalia.getConfig("executable"), "preview", path }, function(result)
                pendingPreviews[path] = nil
                local data = result and noctalia.json.decode(result.stdout or "")
                if data and data.ok and type(data.frames) == "table" then
                    previews[path] = data.frames
                    if hoverPath == path then panel.setNeedsFrameTick(#data.frames > 1); render() end
                end
            end, 60000)
            if not accepted then pendingPreviews[path] = nil end
        end
        if previews[path] then panel.setNeedsFrameTick(#previews[path] > 1) end
    end
    local function library()
        local files = noctalia.state.get("library") or {}
        local categories = { tr("all_categories") }
        local seen = {}
        for _, file in ipairs(files) do
            if file.category ~= "" and not seen[file.category] then table.insert(categories, file.category); seen[file.category] = true end
        end
        local filtered = {}
        for _, file in ipairs(files) do
            if (category == "" or category == file.category) and (query == "" or noctalia.fuzzyScore(query, file.name .. " " .. file.category)) then
                table.insert(filtered, file)
            end
        end
        local selectedCategory = 0
        for i, name in ipairs(categories) do if name == category then selectedCategory = i - 1 end end
        local rows = {
            ui.row({ gap = 12 }, {
                ui.input({ key = "search", placeholder = tr("search"), value = query, flexGrow = 1, onChange = function(value) query = value; scrollRow = 0; render() end }),
                ui.select({ options = categories, selectedIndex = selectedCategory, width = 180, onChange = function(index, value) category = tonumber(index) == 0 and "" or value; scrollRow = 0; render() end }),
                button("", "refresh", function() send({ action = "library" }) end),
            }),
        }
        local activePaths = {}
        for _, widget in ipairs(snapshot.widgets or {}) do activePaths[widget.file] = (activePaths[widget.file] or 0) + 1 end
        local columns = 3
        local totalRows = math.ceil(#filtered / columns)
        local rowHeight = 148
        local start = math.clamp(scrollRow - 1, 0, math.max(0, totalRows - 1))
        local finish = math.min(totalRows, start + 7)
        local tilesRows = {}
        if start > 0 then table.insert(tilesRows, ui.box({height = start * rowHeight, fill = "surface/0"})) end
        for row = start, finish - 1 do
            local tiles = {}
            for column = 1, columns do
                local file = filtered[row * columns + column]
                if file then
                    local activeCount = activePaths[file.path] or 0
                    local function toggle()
                        if activeCount > 0 then
                            for _, widget in ipairs(snapshot.widgets or {}) do if widget.file == file.path then command("quit", widget.id) end end
                        else send({action = "spawn", gif = file.path, state = {output = noctalia.focusedOutputName() or ""}}) end
                    end
                    table.insert(tiles, ui.column({key = file.path, width = 188, height = 136, gap = 8}, {
                        ui.image({key = file.path .. "image", path = hoverPath == file.path and previews[file.path] and previews[file.path][hoverFrame] and previews[file.path][hoverFrame].path or file.path,
                            width = 188, height = 104, fit = "contain", radius = 9,
                            border = activeCount > 0 and "primary" or "surface/0", borderWidth = activeCount > 0 and 2 or 0,
                            onHover = function(entered) hover(file.path, entered) end, onClick = toggle}),
                        ui.label({text = file.name, maxLines = 1, fontSize = 13, color = activeCount > 0 and "primary" or "on_surface_variant"}),
                    }))
                else table.insert(tiles, ui.box({width = 188, height = 136, fill = "surface/0"})) end
            end
            table.insert(tilesRows, ui.row({key = "row" .. tostring(row), gap = 12, height = 136}, tiles))
        end
        if finish < totalRows then table.insert(tilesRows, ui.box({height = (totalRows - finish) * rowHeight, fill = "surface/0"})) end
        if #filtered == 0 then table.insert(tilesRows, ui.label({text = tr("empty"), color = "on_surface_variant"})) end
        table.insert(rows, ui.scroll({key = "gif-list-" .. query .. category, flexGrow = 1, gap = 12,
            onScroll = function(offset)
                local row = math.floor((tonumber(offset) or 0) / rowHeight)
                if row ~= scrollRow then scrollRow = row; render() end
            end}, tilesRows))
        return ui.column({gap = 12, flexGrow = 1}, rows)
    end
    local function active()
        local widget = selectedWidget()
        if not widget then return ui.label({ text = tr("no_active") }) end
        local names = {}
        local index = 0
        for i, item in ipairs(snapshot.widgets or {}) do table.insert(names, item.id); if item.id == widget.id then index = i - 1 end end
        local id = widget.id
        local nodes = {
            ui.select({ key = "active-id", options = names, selectedIndex = index, onChange = function(_, value) selected = value; render() end }),
            ui.row({ gap = 12 }, {
                ui.image({ path = widget.file, width = 90, height = 70, fit = "contain", radius = 9 }),
                ui.column({ flexGrow = 1, gap = 4 }, {
                    ui.label({ text = id, fontWeight = "bold", fontSize = 16 }),
                    ui.label({ text = widget.file, fontSize = 11, color = "on_surface_variant", maxLines = 2 }),
                }),
            }),
            ui.row({ gap = 8 }, {
                button(tr(widget.paused and "play" or "pause"), widget.paused and "player-play" or "player-pause", function() command(widget.paused and "play" or "pause", id) end),
                button(tr(widget.locked and "edit" or "lock"), widget.locked and "lock-open" or "lock", function() command(widget.locked and "unlock" or "lock", id) end),
                button(tr(widget.visible and "hide" or "show"), "eye", function() command(widget.visible and "hide" or "show", id) end),
                button(tr("stop"), "player-stop", function() command("quit", id) end),
                ui.button({glyph = "copy", width = 32, height = 32, tooltip = tr("duplicate"), onClick = function()
                    local state = {}
                    for _, key in ipairs({"scale", "opacity", "speed", "locked", "paused", "visible", "flip_h", "flip_v", "bouncing", "jumping", "jump_rate", "output", "layer", "canvas"}) do state[key] = widget[key] end
                    state.x = widget.base_position[1] + 30; state.y = widget.base_position[2] + 30
                    send({action = "spawn", gif = widget.file, state = state})
                end}),
            }),
        }
        for _, setting in ipairs({ { "scale", "scale", 0.1, 5, widget.scale }, { "opacity", "opacity", 0, 1, widget.opacity }, { "speed", "speed", 0.1, 10, widget.speed }, { "jump_rate", "jump-rate", 0.7, 60, widget.jump_rate } }) do
            local key, action, minimum, maximum, value = setting[1], setting[2], setting[3], setting[4], setting[5]
            local parameter = key == "jump_rate" and "seconds" or key
            table.insert(nodes, section(tr(key) .. " · " .. string.format("%.2f", value), ui.slider({ key = id .. key, min = minimum, max = maximum, step = key == "jump_rate" and 0.1 or 0.01, value = value, onChange = function(changed) local params = {}; params[parameter] = tonumber(changed); command(action, id, params) end })))
        end
        table.insert(nodes, ui.row({ gap = 12 }, {
            section("X · " .. tostring(widget.base_position[1]), ui.input({ key = id .. "x", value = tostring(widget.base_position[1]), onSubmit = function(value) if tonumber(value) then command("move", id, { x = tonumber(value), y = widget.base_position[2] }) end end })),
            section("Y · " .. tostring(widget.base_position[2]), ui.input({ key = id .. "y", value = tostring(widget.base_position[2]), onSubmit = function(value) if tonumber(value) then command("move", id, { x = widget.base_position[1], y = tonumber(value) }) end end })),
        }))
        table.insert(nodes, ui.row({ gap = 8 }, {
            button(tr(widget.bouncing and "stop_bounce" or "bounce"), "arrows-move", function() command(widget.bouncing and "stop-bounce" or "bounce", id) end),
            button(tr("hop"), "arrow-up", function() command("hop", id) end),
            ui.toggle({ width = 44, height = 24, checked = widget.jumping, onChange = function() command("jump", id) end }),
            ui.label({ text = tr("auto_jump") }),
        }))
        table.insert(nodes, ui.row({ gap = 8 }, {
            button(tr("flip_h"), "flip-horizontal", function() command("flip", id, { mode = "toggle-h" }) end),
            button(tr("flip_v"), "flip-vertical", function() command("flip", id, { mode = "toggle-v" }) end),
            button(tr("reset"), "restore", function() command("reset", id) end),
        }))
        local corners = {}
        for _, position in ipairs({ "tl", "tr", "bl", "br", "center" }) do table.insert(corners, button(position, "", function() command("corner", id, { position = position }) end)) end
        table.insert(nodes, ui.row({ gap = 8 }, corners))
        local outputs = {}
        local outputIndex = 0
        for i, output in ipairs(snapshot.outputs or {}) do table.insert(outputs, output.name); if output.name == widget.output then outputIndex = i - 1 end end
        local layers = { "overlay", "top", "bottom", "background" }
        local layerIndex = 0
        for i, value in ipairs(layers) do if value == widget.layer then layerIndex = i - 1 end end
        table.insert(nodes, ui.row({ gap = 12 }, {
            ui.select({ options = outputs, selectedIndex = outputIndex, flexGrow = 1, onChange = function(_, value) command("output", id, { output = value }) end }),
            ui.select({ options = layers, selectedIndex = layerIndex, flexGrow = 1, onChange = function(_, value) command("layer", id, { layer = value }) end }),
            ui.toggle({ width = 44, height = 24, checked = widget.canvas, onChange = function() command("canvas", id) end }),
            ui.label({ text = tr("canvas") }),
        }))
        if widget.error then table.insert(nodes, ui.label({ text = widget.error, color = "error" })) end
        if widget.warning then table.insert(nodes, ui.label({ text = widget.warning, color = "on_surface_variant" })) end
        return ui.column({ gap = 12 }, nodes)
    end
    local function profiles()
        local nodes = {
            ui.input({ key = "profile-name", placeholder = tr("profile_name"), onChange = function(value) profileName = value end, onSubmit = function(value) if value ~= "" then send({ action = "profile-save", name = value }) end end }),
            ui.row({gap = 8}, {button(tr("save_profile"), "device-floppy", function() if profileName ~= "" then send({ action = "profile-save", name = profileName }) end end)}),
            ui.input({ key = "rename-name", placeholder = tr("rename_name"), onChange = function(value) renameName = value end }),
        }
        local names = {}
        for name in pairs(snapshot.profiles or {}) do table.insert(names, name) end
        table.sort(names)
        for _, name in ipairs(names) do
            table.insert(nodes, ui.row({ key = name, gap = 8 }, {
                ui.label({ text = name, flexGrow = 1 }),
                button(tr("apply"), "player-play", function() send({ action = "profile-apply", name = name }) end),
                button(tr("overwrite"), "device-floppy", function() send({ action = "profile-save", name = name }) end),
                button(tr("rename"), "edit", function() if renameName ~= "" then send({ action = "profile-rename", name = name, new_name = renameName }) end end),
                button(tr("delete"), "trash", function() send({ action = "profile-delete", name = name }) end),
            }))
        end
        return ui.column({ gap = 12 }, nodes)
    end
    local function settings()
        local level = snapshot.logging and snapshot.logging.level or "info"
        local nextLevel = level == "info" and "debug" or level == "debug" and "trace" or level == "trace" and "off" or "info"
        return ui.column({gap = 16}, {
            ui.row({gap = 12, align = "center", fill = "surface_variant", radius = 10, border = "outline", borderWidth = 1, padding = 12}, {
                ui.label({text = tr("restore"), flexGrow = 1, maxLines = 2}),
                ui.toggle({width = 44, height = 24, checked = not snapshot.settings or snapshot.settings.restore ~= false,
                    onChange = function(value) send({action = "settings", settings = {restore = value == "true"}}) end}),
            }),
            ui.row({gap = 8}, {
                button(tr("app"), "app-window", function() noctalia.runAsync({noctalia.getConfig("executable"), "app"}, function() panel.close() end) end),
                button(tr("plugin_settings"), "settings", function() noctalia.openSettings() end),
            }),
            ui.row({gap = 8}, {
                button(tr("start_daemon"), "player-play", function() nonce += 1; noctalia.state.set("start", source .. tostring(nonce)) end),
                button(tr("quit_daemon"), "power", function() send({action = "quit-daemon"}) end),
                button(tr("doctor"), "stethoscope", function() noctalia.runAsync({noctalia.getConfig("executable"), "doctor"}, function(result) noctalia.notify(tr("title"), result and (result.stdout ~= "" and result.stdout or result.stderr) or tr("missing_engine")) end) end),
            }),
            ui.row({gap = 8}, {
                button(tr("logs_level") .. ": " .. level, "list-details", function() send({action = "logs-level", level = nextLevel}) end),
                button(tr("logs_export"), "folder", function()
                    noctalia.runAsync({noctalia.getConfig("executable"), "logs", "export"}, function(result)
                        local data = result and noctalia.json.decode(result.stdout or "")
                        noctalia.notify(tr("title"), data and (data.error or data.directory) or result and result.stderr or tr("missing_engine"))
                    end, 40000)
                end),
            }),
        })
    end
    render = function()
        if not opened then return end
        local body = source == "picker" and library() or source == "settings" and ui.column({gap = 16}, {settings(), profiles()}) or active()
        local error = noctalia.state.get("error") or ""
        if error == "" then error = snapshot.error or "" end
        local title = source == "picker" and tr("collection") or source == "settings" and tr("settings_tab") or tr("starter")
        local header = {ui.label({text = title, color = "primary", fontSize = 16, fontWeight = "bold", flexGrow = 1})}
        if source == "control" then table.insert(header, button("", "photo", function() noctalia.togglePanel("madebycli/gif-player:picker") end)) end
        if source ~= "control" then table.insert(header, button("", "adjustments", function() noctalia.togglePanel("madebycli/gif-player:control") end)) end
        if source ~= "settings" then table.insert(header, button("", "settings", function() noctalia.togglePanel("madebycli/gif-player:settings") end)) end
        if closeButton then table.insert(header, button("", "close", function() panel.close() end)) end
        local nodes = {ui.row({gap = 8, align = "center"}, header)}
        if source == "picker" then table.insert(nodes, body) else
            table.insert(nodes, ui.scroll({key = source .. "-body", flexGrow = 1, gap = 12}, {ui.column({fill = "surface_variant", border = "outline", borderWidth = 1, radius = 10, padding = 12, gap = 12}, {body})}))
        end
        if source == "control" then
            local global = {}
            local paused = true
            for _, widget in ipairs(snapshot.widgets or {}) do if not widget.paused then paused = false end end
            for _, item in ipairs({{paused and "play" or "pause", paused and "player-play" or "player-pause"}, {"lock", "lock"}, {"unlock", "lock-open"}, {"show", "eye"}, {"hide", "eye-off"}, {"stop-all", "player-stop"}}) do
                local action = item[1]
                table.insert(global, ui.button({glyph = item[2], tooltip = tr(action), width = 32, height = 32, onClick = function() command(action, "*") end}))
            end
            table.insert(nodes, ui.row({gap = 8, align = "center"}, global))
        end
        if error ~= "" then table.insert(nodes, ui.label({text = error, color = "error", maxLines = 2})) end
        panel.render(ui.column({flexGrow = 1, gap = 12}, nodes))
    end
    noctalia.state.watch("snapshot", function(value) snapshot = value or snapshot; render() end)
    noctalia.state.watch("library", render)
    noctalia.state.watch("error", render)
    noctalia.state.watch("selected", function(id) selected = id; render() end)
    local function onOpen()
        opened = true
        noctalia.state.set("panel-open-" .. source, true)
        local shell = noctalia.getSetting("shell")
        closeButton = type(shell) ~= "table" or shell.show_close_buttons ~= false
        nonce += 1
        noctalia.state.set("start", source .. tostring(nonce))
        render()
    end
    local function onClose() opened = false; previews = {}; pendingPreviews = {}; noctalia.state.set("panel-open-" .. source, false); hoverPath = nil; panel.setNeedsFrameTick(false) end
    local function onIpc(event) if event == "refresh" then send({ action = "library" }) end end
    local function onFrameTick(deltaMs)
        local frames = hoverPath and previews[hoverPath]
        if not opened or tab ~= 0 or not frames or #frames < 2 then return end
        hoverTime += deltaMs
        local total = 0
        for _, frame in ipairs(frames) do total += frame.duration_ms end
        local phase = hoverTime % total
        local index = 1
        for i, frame in ipairs(frames) do
            if phase < frame.duration_ms then index = i; break end
            phase -= frame.duration_ms
        end
        if hoverFrame ~= index then hoverFrame = index; render() end
    end
    return { onOpen = onOpen, onClose = onClose, onIpc = onIpc, onFrameTick = onFrameTick }
end
