#!/usr/bin/env python3
"""Execute Luau render callbacks against the upstream Noctalia property contract.

This is a script/API and responsive-layout check, not a real Noctalia compositor test.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

p = argparse.ArgumentParser()
p.add_argument("--plugin", default="plugins/gif-player")
p.add_argument("--kind", choices=["sound", "gif"], default="gif")
p.add_argument("--noctalia-source", required=True)
p.add_argument("--luau", default="luau")
args = p.parse_args()
plugin = Path(args.plugin)
upstream = Path(args.noctalia_source)
manifest = tomllib.loads((plugin / "plugin.toml").read_text())
levels = json.loads((upstream / "docs/plugin-api.json").read_text())
assert manifest["plugin_api"] <= max(level["level"] for level in levels)
assert all(panel.get("placement") == "floating" for panel in manifest["panel"])
source = (upstream / "src/ui/ui_tree_reconciler.cpp").read_text()
props = {name: re.findall(r'"([^"]+)"', body) for name, body in re.findall(r'k(\w+)\s*=\s*\{(.*?)\};', source, re.S)}
types = {"column":"Flex", "row":"Flex", "box":"Box", "label":"Label", "glyph":"Glyph", "image":"Image", "button":"Button", "select":"Select", "slider":"Slider", "input":"Input", "toggle":"Toggle", "scroll":"Scroll", "spacer":"Common", "separator":"Separator"}

def lua(value):
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, (int, float)):
        return str(value)
    if value is None:
        return "nil"
    if isinstance(value, dict):
        return "{" + ",".join("[" + lua(k) + "]=" + lua(v) for k,v in value.items()) + "}"
    return "{" + ",".join(lua(v) for v in value) + "}"

harness = r'''
local currentWidth, currentHeight = 1366, 768
local settings = {shell={corner_radius_scale=1,panel={transparency_mode="solid",borders=true},show_close_buttons=true},accessibility={ui_scale=1}}
local values, watchers = {}, {}
local tree
local function encode(value)
    if type(value) ~= "table" then return tostring(value) end
    local keys = {}; for key in pairs(value) do table.insert(keys,key) end
    table.sort(keys,function(a,b)return tostring(a)<tostring(b)end)
    local parts={};for _,key in ipairs(keys)do table.insert(parts,tostring(key)..":"..encode(value[key]))end
    return "{"..table.concat(parts,",").."}"
end
noctalia={getSetting=function(key)return settings[key]end,
    outputs=function()return {{name="small",width=currentWidth,height=currentHeight,scale=1,focused=true}}end,
    focusedOutputName=function()return "small"end,getColor=function()return "#abcdef"end,
    tr=function(key)return key end,trp=function(key)return key end,getConfig=function()return "gif-player"end,
    fuzzyScore=function()return true end,json={encode=encode,decode=function()return nil end},
    state={get=function(key)return values[key]end,watch=function(key,callback)watchers[key]=callback end,
        set=function(key,value)values[key]=value;if watchers[key]then watchers[key](value)end end},
    runAsync=function()return true end,togglePanel=function()end,expandPath=function(path)return path end}
panel={render=function(value)tree=value end,setWantsSecondTicks=function()end,setNeedsFrameTick=function()end,close=function()end}
ui={}
'''
for name, group in types.items():
    allowed = {key: True for key in props[group] + ["key"]}
    harness += f'''ui.{name}=function(properties,children)
        local allowed={lua(allowed)}
        for key in pairs(properties or {{}}) do assert(allowed[key],"Unknown {name} property: "..key) end
        return {{type={lua(name)},props=properties or {{}},children=children or {{}}}}
    end\n'''
harness += "local host=(function()\n" + (plugin / "host.luau").read_text() + "\nend)()\n"
if args.kind == "sound":
    harness += (plugin / "sound.luau").read_text().replace('local host = require("./host.luau")', "")
    harness += '''
    local output={id=1,kind="sink",label=string.rep("Long device name ",10),volume=.32,is_default=true,muted=false}
    local input={id=2,kind="source",label="Microphone",volume=.88,is_default=true,muted=false}
    local streams={};for i=1,50 do streams[i]={id=i+10,kind="sink-input",label="Music app "..i,volume=.54,muted=false,device=1}end
    values.snapshot={outputs={output},inputs={input},playback=streams,recording={}}
    onOpen();watchers.snapshot(values.snapshot)
    local updateView=update
    '''
else:
    panel_code = (plugin / "panel.luau").read_text().replace('local host = require("./host.luau")', "")
    harness += "local buildPanel=(function()\n" + panel_code + "\nend)()\n"
    harness += '''
    values.connected=true
    values.snapshot={widgets={{id="test",file="/tmp/test.gif",base_position={0,0},scale=1,opacity=1,speed=1,paused=false,locked=true,visible=true,output="small",layer="overlay",canvas=false,jump_rate=2}},profiles={},outputs={},settings={}}
    values.library={};for i=1,100 do values.library[i]={path="/tmp/gif"..i..".gif",name="GIF "..i,category="",thumbnail=""}end
    local handlers=buildPanel(0,SOURCE)
    handlers.onOpen()
    local updateView=handlers.update
    '''
harness += '''
local cases={{1366,768,1},{1280,720,1.25},{800,600,1.5},{640,360,1},{3840,2160,2}}
local function inspect(node,maxWidth,inScroll)
    if node.props.width then assert(node.props.width<=maxWidth,"Fixed width exceeds viewport: "..node.type.." "..node.props.width.." > "..maxWidth)end
    if node.props.height and not inScroll then assert(node.props.height<=currentHeight/settings.accessibility.ui_scale,"Fixed height exceeds monitor")end
    for _,child in ipairs(node.children)do inspect(child,maxWidth,inScroll or node.type=="scroll")end
end
for _,case in ipairs(cases)do
    currentWidth,currentHeight=case[1],case[2];settings.accessibility.ui_scale=case[3]
    host.openViewport()
    updateView()
    assert(tree,"Panel did not render")
    inspect(tree,(currentWidth-16)/case[3])
end
currentWidth,currentHeight=640,360;settings.accessibility.ui_scale=1
host.openViewport()
local smallWidth=host.dimensions(1120,720)
currentWidth,currentHeight=1366,768
assert(host.dimensions(1120,720)==smallWidth,"An open surface must not grow beyond its negotiated extent")
host.openViewport()
assert(host.dimensions(1120,720)>smallWidth,"A new open must use the larger output")
settings.shell.corner_radius_scale=0
settings.shell.panel.transparency_mode="glass"
settings.shell.panel.borders=false
local card=host.card()
assert(card.radius==0 and card.fill=="surface_variant/0.65" and card.borderWidth==0)
settings.shell.corner_radius_scale=2
settings.shell.panel.transparency_mode="soft"
assert(host.card().radius==24 and host.style().panel_opacity==.8)
print("Native script property, monitor-size, scale and theme contracts passed")
'''
with tempfile.TemporaryDirectory(prefix="noctalia-contract-") as temp:
    script = Path(temp) / "contract.luau"
    for name in (["sound"] if args.kind == "sound" else ["picker", "control", "settings"]):
        script.write_text(harness.replace("SOURCE", lua(name)))
        subprocess.run([args.luau, str(script)], check=True)
