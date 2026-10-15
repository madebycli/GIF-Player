--!nonstrict
-- This module is evaluated in each panel's own VM. The host renders native controls.
return function(initialTab, source)
    local tab = initialTab
    local contentWidth = source == "settings" and 556 or 596
    local query = ""
    local searchReset = 0
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
    local expanded = {}
    local directory = nil
    local renameTarget = nil
    local deleteTarget = nil
    local function tr(key) return noctalia.tr(key) end
    local function send(command)
        nonce += 1
        noctalia.state.set("command", { source = source, nonce = nonce, command = command })
    end
    local function button(text, glyph, callback, options)
        local props = { text = text, onClick = callback, controlSize = "sm" }
        if text == "" then props.width = 32; props.height = 32 end
        if glyph ~= "" then props.glyph = glyph end
        props.tooltip = text
        for key, value in pairs(options or {}) do props[key] = value end
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
        local search = {ui.input({key = "search" .. tostring(searchReset), placeholder = tr("search"), value = query, focus = true, flexGrow = 1,
            onChange = function(value) query = value; scrollRow = 0; render() end})}
        if #categories > 1 then table.insert(search, ui.select({options = categories, selectedIndex = selectedCategory, width = 160, onChange = function(index, value) category = tonumber(index) == 0 and "" or value; scrollRow = 0; render() end})) end
        table.insert(search, button("", query ~= "" and "x" or "refresh", function()
            if query ~= "" then query = ""; searchReset += 1; scrollRow = 0; render() else send({action = "library"}) end
        end, {tooltip = tr(query ~= "" and "clear_search" or "refresh")}))
        local rows = {ui.row({gap = 8}, search)}
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
                            tooltip = tr(activeCount > 0 and "click_stop" or "click_play") .. " · " .. file.name,
                            onHover = function(entered) hover(file.path, entered) end, onClick = toggle}),
                        ui.label({text = file.name, maxLines = 1, fontSize = 13, color = activeCount > 0 and "primary" or "on_surface_variant"}),
                    }))
                else table.insert(tiles, ui.box({width = 188, height = 136, fill = "surface/0"})) end
            end
            table.insert(tilesRows, ui.row({key = "row" .. tostring(row), gap = 12, height = 136}, tiles))
        end
        if finish < totalRows then table.insert(tilesRows, ui.box({height = (totalRows - finish) * rowHeight, fill = "surface/0"})) end
        if #filtered == 0 then
            table.insert(tilesRows, ui.column({gap = 12, paddingV = 24}, {
                ui.label({text = tr(#files == 0 and "collection_empty" or "search_empty"), fontSize = 18, fontWeight = "medium"}),
                ui.label({text = tr("collection_empty_hint"), color = "on_surface_variant", maxLines = 2}),
                ui.row({}, {button(tr("settings_tab"), "settings", function() noctalia.togglePanel("madebycli/gif-player:settings") end)}),
            }))
        end
        table.insert(rows, ui.scroll({key = "gif-list-" .. query .. category, flexGrow = 1, gap = 12,
            onScroll = function(offset)
                local row = math.floor((tonumber(offset) or 0) / rowHeight)
                if row ~= scrollRow then scrollRow = row; render() end
            end}, tilesRows))
        table.insert(rows, ui.label({text = tr("picker_hint"), fontSize = 11, color = "on_surface_variant", maxLines = 1}))
        return ui.column({gap = 12, flexGrow = 1}, rows)
    end
    local function card(title, nodes)
        local children = {ui.label({text = title, fontWeight = "medium", color = "on_surface"})}
        for _, node in ipairs(nodes) do table.insert(children, node) end
        return ui.column({width = contentWidth - 28, align = "stretch", gap = 12, padding = 14, radius = 12, fill = "surface_variant", border = "outline", borderWidth = 1}, children)
    end
    local function fold(key, title, summary, build)
        local nodes = {ui.row({gap = 8, align = "center"}, {
            button(title, expanded[key] and "chevron-down" or "chevron-right", function() expanded[key] = not expanded[key]; render() end,
                {variant = "ghost", contentAlign = "start", flexGrow = 1, selected = expanded[key] == true}),
            ui.label({text = summary or "", color = "on_surface_variant", fontSize = 12}),
        })}
        if expanded[key] then table.insert(nodes, build()) end
        return ui.column({key = key, width = contentWidth - 24, align = "stretch", gap = 12, padding = 12, radius = 12, fill = "surface_variant", border = "outline", borderWidth = 1}, nodes)
    end
    local function slider(widget, key, action, minimum, maximum, label, display)
        return ui.column({gap = 4}, {
            ui.row({align = "center"}, {ui.label({text = label, flexGrow = 1, fontSize = 13}), ui.label({text = display, color = "primary", fontSize = 13})}),
            ui.slider({key = widget.id .. key, min = minimum, max = maximum, step = key == "jump_rate" and 0.1 or 0.01, value = widget[key], controlSize = "sm",
                onChange = function(value) local values = {}; values[key == "jump_rate" and "seconds" or key] = tonumber(value); command(action, widget.id, values) end}),
        })
    end
    local function toggleRow(label, checked, callback, hint)
        return ui.row({gap = 12, align = "center"}, {
            ui.column({gap = 3, flexGrow = 1}, {ui.label({text = label}), hint and ui.label({text = hint, color = "on_surface_variant", fontSize = 12, maxLines = 2}) or ui.spacer({height = 0})}),
            ui.toggle({width = 44, height = 24, checked = checked, onChange = callback}),
        })
    end
    local function active()
        local widget = selectedWidget()
        if not widget then return ui.column({key = "control-empty", gap = 16, padding = 24, align = "center"}, {
            ui.glyph({name = "photo", size = 40, color = "primary"}),
            ui.label({text = tr("no_active_title"), fontSize = 18, fontWeight = "medium"}),
            ui.label({text = tr("no_active"), color = "on_surface_variant", maxLines = 2}),
            button(tr("browse_gifs"), "photo", function() noctalia.togglePanel("madebycli/gif-player:picker") end, {variant = "primary"}),
        }) end
        local id = widget.id
        local nodes = {}
        if #(snapshot.widgets or {}) > 1 then
            local names = {}; local index = 0
            for i, item in ipairs(snapshot.widgets) do table.insert(names, item.id); if item.id == id then index = i - 1 end end
            table.insert(nodes, section(tr("active_tab") .. " · " .. tostring(#names), ui.select({key = "active-id", options = names, selectedIndex = index,
                onChange = function(_, value) selected = value; render() end})))
        end
        local state = widget.loading and "loading" or widget.error and "load_failed" or not widget.visible and "hidden" or widget.paused and "paused" or "playing"
        table.insert(nodes, ui.row({gap = 14, align = "center"}, {
            ui.image({path = widget.file, width = 76, height = 68, fit = "contain", radius = 10}),
            ui.column({gap = 6, flexGrow = 1}, {ui.label({text = id, fontWeight = "bold", fontSize = 18, maxLines = 1}), ui.label({text = tr(state), color = "on_surface_variant", fontSize = 12})}),
            button("", "copy", function()
                local settings = {}
                for _, key in ipairs({"scale", "opacity", "speed", "locked", "paused", "visible", "flip_h", "flip_v", "bouncing", "jumping", "jump_rate", "output", "layer", "canvas"}) do settings[key] = widget[key] end
                settings.x = widget.base_position[1] + 30; settings.y = widget.base_position[2] + 30
                send({action = "spawn", gif = widget.file, state = settings})
            end, {tooltip = tr("duplicate")}),
        }))
        table.insert(nodes, ui.row({gap = 8}, {
            button(tr(widget.paused and "play" or "pause"), widget.paused and "player-play" or "player-pause", function() command(widget.paused and "play" or "pause", id) end, {variant = "primary", flexGrow = 1}),
            button(tr(widget.locked and "edit" or "done_editing"), widget.locked and "arrows-move" or "check", function() command(widget.locked and "unlock" or "lock", id) end, {selected = not widget.locked, flexGrow = 1}),
            button(tr(widget.visible and "hide" or "show"), widget.visible and "eye-off" or "eye", function() command(widget.visible and "hide" or "show", id) end),
            button(tr("stop"), "player-stop", function() command("quit", id) end, {variant = "destructive"}),
        }))
        if not widget.locked then table.insert(nodes, ui.label({text = tr("edit_hint"), color = "primary", fontSize = 12, maxLines = 2})) end
        table.insert(nodes, card(tr("quick_adjustments"), {
            slider(widget, "scale", "scale", 0.1, 5, tr("scale"), string.format("%.0f%%", widget.scale * 100)),
            slider(widget, "opacity", "opacity", 0, 1, tr("opacity"), string.format("%.0f%%", widget.opacity * 100)),
            slider(widget, "speed", "speed", 0.1, 10, tr("speed"), string.format("%.2f×", widget.speed)),
        }))
        table.insert(nodes, fold("placement", tr("placement"), widget.output or "", function()
            local outputs = {}; local outputIndex = 0
            for i, output in ipairs(snapshot.outputs or {}) do table.insert(outputs, output.name); if output.name == widget.output then outputIndex = i - 1 end end
            local corners = {}
            for _, item in ipairs({{"tl", "arrow-up-left"}, {"tr", "arrow-up-right"}, {"bl", "arrow-down-left"}, {"br", "arrow-down-right"}}) do
                table.insert(corners, button("", item[2], function() command("corner", id, {position = item[1]}) end, {tooltip = tr("corner_" .. item[1])}))
            end
            table.insert(corners, button(tr("center"), "focus-centered", function() command("center", id) end, {flexGrow = 1}))
            return ui.column({gap = 12}, {
                section(tr("monitor"), ui.select({options = outputs, selectedIndex = outputIndex, onChange = function(_, value) command("output", id, {output = value}) end})),
                ui.row({gap = 12}, {
                    ui.column({flexGrow = 1, gap = 6}, {ui.label({text = "X", color = "on_surface_variant"}), ui.input({key = id .. "x" .. tostring(widget.base_position[1]), value = string.format("%.0f", widget.base_position[1]), onSubmit = function(value) if tonumber(value) then command("move", id, {x = tonumber(value), y = widget.base_position[2]}) end end})}),
                    ui.column({flexGrow = 1, gap = 6}, {ui.label({text = "Y", color = "on_surface_variant"}), ui.input({key = id .. "y" .. tostring(widget.base_position[2]), value = string.format("%.0f", widget.base_position[2]), onSubmit = function(value) if tonumber(value) then command("move", id, {x = widget.base_position[1], y = tonumber(value)}) end end})}),
                }),
                ui.row({gap = 8}, corners),
            })
        end))
        table.insert(nodes, fold("motion", tr("motion"), (widget.bouncing or widget.jumping) and tr("enabled") or "", function()
            local children = {
                toggleRow(tr("bounce"), widget.bouncing, function() command(widget.bouncing and "stop-bounce" or "bounce", id) end),
                toggleRow(tr("auto_jump"), widget.jumping, function() command("jump", id) end),
            }
            if widget.jumping then table.insert(children, slider(widget, "jump_rate", "jump-rate", 0.7, 60, tr("jump_rate"), string.format("%.1f s", widget.jump_rate))) end
            table.insert(children, ui.row({}, {button(tr("hop"), "arrow-up", function() command("hop", id) end)}))
            return ui.column({gap = 14}, children)
        end))
        table.insert(nodes, fold("advanced", tr("advanced"), "", function()
            local layers = {"overlay", "top", "bottom", "background"}; local labels = {}; local layerIndex = 0
            for i, value in ipairs(layers) do table.insert(labels, tr("layer_" .. value)); if value == widget.layer then layerIndex = i - 1 end end
            return ui.column({gap = 14}, {
                ui.row({gap = 8}, {
                    button(tr("flip_h"), "flip-horizontal", function() command("flip", id, {mode = "toggle-h"}) end, {selected = widget.flip_h}),
                    button(tr("flip_v"), "flip-vertical", function() command("flip", id, {mode = "toggle-v"}) end, {selected = widget.flip_v}),
                }),
                section(tr("layer"), ui.select({options = labels, selectedIndex = layerIndex, onChange = function(index) command("layer", id, {layer = layers[tonumber(index) + 1]}) end})),
                toggleRow(tr("canvas"), widget.canvas, function() command("canvas", id) end),
                ui.label({text = widget.file, fontSize = 11, color = "on_surface_variant", maxLines = 2}),
                ui.row({}, {button(tr("reset"), "restore", function() command("reset", id) end)}),
            })
        end))
        if widget.error then table.insert(nodes, ui.label({text = widget.error, color = "error", maxLines = 3})) end
        if widget.warning then table.insert(nodes, ui.label({text = widget.warning, color = "on_surface_variant", maxLines = 3})) end
        return ui.column({key = "control-content", width = contentWidth, padding = 0, align = "stretch", gap = 12}, nodes)
    end
    local function profiles()
        local nodes = {
            ui.label({text = tr("layouts_hint"), fontSize = 12, color = "on_surface_variant", maxLines = 2}),
            ui.row({gap = 8}, {
                ui.input({key = "profile-name", placeholder = tr("profile_name"), flexGrow = 1, onChange = function(value) profileName = value; render() end, onSubmit = function(value) if value ~= "" then send({action = "profile-save", name = value}) end end}),
                button(tr(snapshot.profiles and snapshot.profiles[profileName] and "overwrite" or "save"), "device-floppy", function() if profileName ~= "" then send({action = "profile-save", name = profileName}) end end, {enabled = profileName ~= ""}),
            }),
        }
        local names = {}; for name in pairs(snapshot.profiles or {}) do table.insert(names, name) end; table.sort(names)
        for _, name in ipairs(names) do
            local row = {ui.label({text = name, flexGrow = 1, maxLines = 1}),
                button(tr("apply"), "player-play", function() send({action = "profile-apply", name = name}) end),
                button("", "device-floppy", function() send({action = "profile-save", name = name}) end, {tooltip = tr("overwrite")}),
                button("", "edit", function() renameTarget = name; renameName = name; render() end, {tooltip = tr("rename")}),
                button("", "trash", function() deleteTarget = name; render() end, {tooltip = tr("delete")}),
            }
            table.insert(nodes, ui.row({key = name, gap = 8, align = "center"}, row))
            if renameTarget == name then table.insert(nodes, ui.row({gap = 8}, {
                ui.input({key = "rename-" .. name, value = name, flexGrow = 1, focus = true, onChange = function(value) renameName = value end, onSubmit = function(value) if value ~= "" then send({action = "profile-rename", name = name, new_name = value}); renameTarget = nil; render() end end}),
                button(tr("save"), "check", function() if renameName ~= "" then send({action = "profile-rename", name = name, new_name = renameName}); renameTarget = nil; render() end end),
                button(tr("cancel"), "x", function() renameTarget = nil; render() end),
            })) end
            if deleteTarget == name then table.insert(nodes, ui.row({gap = 8, align = "center"}, {
                ui.label({text = tr("delete_layout"), flexGrow = 1}),
                button(tr("delete"), "trash", function() send({action = "profile-delete", name = name}); deleteTarget = nil; render() end, {variant = "destructive"}),
                button(tr("cancel"), "x", function() deleteTarget = nil; render() end),
            })) end
        end
        return ui.column({width = contentWidth - 24, align = "stretch", gap = 12}, nodes)
    end
    local function settings()
        local level = snapshot.logging and snapshot.logging.level or "info"
        local levels = {"info", "debug", "trace", "warn", "error", "off"}; local labels = {}; local index = 0
        for i, value in ipairs(levels) do table.insert(labels, tr("log_" .. value)); if value == level then index = i - 1 end end
        local function saveDirectory(value) if value and value ~= "" then send({action = "library-directory", path = value}) end end
        local nodes = {
            card(tr("collection"), {
                ui.label({text = tr("directory_hint"), color = "on_surface_variant", fontSize = 12, maxLines = 2}),
                ui.row({gap = 8}, {
                    ui.input({key = "directory-" .. tostring(snapshot.collection_dir or ""), value = snapshot.collection_dir or noctalia.getConfig("directory") or "", placeholder = tr("settings.directory"), flexGrow = 1,
                        onChange = function(value) directory = value end, onSubmit = saveDirectory}),
                    button(tr("save"), "check", function() saveDirectory(directory) end),
                }),
                toggleRow(tr("restore"), not snapshot.settings or snapshot.settings.restore ~= false, function(value) send({action = "settings", settings = {restore = value == "true"}}) end),
            }),
            fold("layouts", tr("profiles"), "", profiles),
            fold("diagnostics", tr("doctor"), "", function()
                local children = {
                    section(tr("logs_level"), ui.select({key = "log-level", options = labels, selectedIndex = index, onChange = function(value) send({action = "logs-level", level = levels[tonumber(value) + 1]}) end})),
                    ui.label({text = tr(level == "trace" and "trace_hint" or "diagnostics_hint"), fontSize = 12, color = "on_surface_variant", maxLines = 2}),
                    ui.row({gap = 8}, {
                        button(tr("logs_export"), "folder", function()
                            noctalia.runAsync({noctalia.getConfig("executable"), "logs", "export"}, function(result)
                                local data = result and noctalia.json.decode(result.stdout or "")
                                noctalia.notify(tr("title"), data and (data.error or data.directory) or result and result.stderr or tr("missing_engine"))
                            end, 40000)
                        end),
                        button(tr("doctor"), "stethoscope", function() noctalia.runAsync({noctalia.getConfig("executable"), "doctor"}, function(result) noctalia.notify(tr("title"), result and (result.stdout ~= "" and result.stdout or result.stderr) or tr("missing_engine")) end) end),
                    }),
                }
                local report = snapshot.logging and snapshot.logging.last_export
                if report and report.ok and report.directory and not report.pending then
                    table.insert(children, ui.label({text = tr("export_complete"), color = "primary", fontSize = 12}))
                    table.insert(children, button(tr("open_folder"), "folder-open", function() noctalia.runAsync({"xdg-open", report.directory}, function(result) if not result or result.exitCode ~= 0 then noctalia.notify(tr("title"), result and result.stderr or report.directory) end end) end))
                end
                return ui.column({gap = 12}, children)
            end),
            fold("application", tr("application"), "", function() return ui.column({gap = 12}, {
                ui.row({gap = 8}, {
                    button(tr("app"), "app-window", function() noctalia.runAsync({noctalia.getConfig("executable"), "app"}, function() panel.close() end) end),
                    button(tr("plugin_settings"), "settings", function() noctalia.openSettings() end),
                }),
                ui.row({gap = 8}, {
                    button(tr("start_daemon"), "player-play", function() nonce += 1; noctalia.state.set("start", source .. tostring(nonce)) end),
                    button(tr("quit_daemon"), "power", function() send({action = "quit-daemon"}) end),
                }),
            }) end),
        }
        return ui.column({width = contentWidth, align = "stretch", gap = 12}, nodes)
    end
    render = function()
        if not opened then return end
        local connected = noctalia.state.get("connected") == true
        local body = source == "picker" and library() or source == "settings" and settings() or not connected and ui.column({key = "control-offline", gap = 16, paddingV = 24}, {
            ui.label({text = tr("disconnected"), fontSize = 18, fontWeight = "medium"}),
            ui.label({text = tr("reconnect_hint"), color = "on_surface_variant", maxLines = 2}),
            ui.row({}, {button(tr("start_daemon"), "player-play", function() nonce += 1; noctalia.state.set("start", source .. tostring(nonce)) end, {variant = "primary"})}),
        }) or active()
        local error = noctalia.state.get("error") or ""
        if error == "" then error = snapshot.error or "" end
        local title = source == "picker" and tr("collection") or source == "settings" and tr("settings_tab") or tr("starter")
        local header = {ui.label({text = title, color = "primary", fontSize = 16, fontWeight = "bold", flexGrow = 1})}
        if source == "control" then table.insert(header, button(tr("collection"), "photo", function() noctalia.togglePanel("madebycli/gif-player:picker") end, {variant = "ghost"})) end
        if source ~= "control" then table.insert(header, button(tr("starter"), "adjustments", function() noctalia.togglePanel("madebycli/gif-player:control") end, {variant = "ghost"})) end
        if source ~= "settings" then table.insert(header, button("", "settings", function() noctalia.togglePanel("madebycli/gif-player:settings") end, {tooltip = tr("settings_tab")})) end
        if closeButton then table.insert(header, button("", "x", function() panel.close() end, {tooltip = tr("close")})) end
        local nodes = {ui.row({gap = 8, align = "center"}, header)}
        if source == "picker" then table.insert(nodes, body) else
            table.insert(nodes, ui.scroll({key = source .. "-body", width = contentWidth + 16, align = "stretch", flexGrow = 1, gap = 12}, {body}))
        end
        if source == "control" and #(snapshot.widgets or {}) > 1 then
            local paused = true; local locked = true; local visible = false
            for _, widget in ipairs(snapshot.widgets) do if not widget.paused then paused = false end; if not widget.locked then locked = false end; if widget.visible then visible = true end end
            table.insert(nodes, fold("all", tr("all_gifs"), tostring(#snapshot.widgets), function() return ui.column({gap = 8}, {
                ui.row({gap = 8}, {
                    button(tr(paused and "play_all" or "pause_all"), paused and "player-play" or "player-pause", function() command(paused and "play" or "pause", "*") end),
                    button(tr(locked and "edit_all" or "done_editing"), locked and "arrows-move" or "check", function() command(locked and "unlock" or "lock", "*") end),
                }),
                ui.row({gap = 8}, {
                    button(tr(visible and "hide_all" or "show_all"), visible and "eye-off" or "eye", function() command(visible and "hide" or "show", "*") end),
                    button(tr("stop-all"), "player-stop", function() command("stop-all", "*") end, {variant = "destructive"}),
                }),
            }) end))
        end
        if error ~= "" then table.insert(nodes, ui.label({text = error, color = "error", maxLines = 2})) end
        panel.render(ui.column({width = contentWidth + 16, align = "stretch", flexGrow = 1, gap = 12}, nodes))
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
