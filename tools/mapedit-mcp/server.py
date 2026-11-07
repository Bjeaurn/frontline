#!/usr/bin/env python3
"""MCP server (stdio, newline-delimited JSON-RPC 2.0) that drives MapEdit, the Frontline map
viewer, through its remote protocol (Bevy remote protocol over HTTP on localhost, port 15702).

Standard library only. Register it with an MCP client as the command

    python3 tools/mapedit-mcp/server.py

Viewing tools: mapedit_launch (start the viewer and wait until the Map is ready), mapedit_status,
mapedit_load_map, mapedit_reload, mapedit_set_look, mapedit_set_camera, mapedit_set_overlay,
mapedit_entities, mapedit_screenshot (writes a PNG and returns its path; read the file to look
at it) and mapedit_quit.

Editing tools (build a Map): mapedit_new_map, mapedit_save, mapedit_save_as, mapedit_map_settings,
mapedit_palette (what can be placed), mapedit_items (what the Map holds), mapedit_select,
mapedit_move, mapedit_resize, mapedit_delete, mapedit_duplicate, mapedit_set_texture,
mapedit_set_property, mapedit_add_brush, mapedit_add_entity, mapedit_undo, mapedit_redo and
mapedit_ui (show the Esc menu or the palette panel). Edits change a working copy; nothing reaches
the Map's file before mapedit_save. A typical session: mapedit_launch, mapedit_new_map (or open an
existing Map), a few mapedit_add_brush / mapedit_add_entity, mapedit_screenshot to look, mapedit_save.

Environment: MAPEDIT_PORT (default 15702); MAPEDIT_BIN, a built `mapedit` executable to start
instead of `cargo run -p mapedit` from the repository root.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PORT = int(os.environ.get("MAPEDIT_PORT", "15702"))
URL = f"http://127.0.0.1:{PORT}/"
LOG = Path("/tmp/mapedit.log")
PROTOCOL = "2024-11-05"

_launched: subprocess.Popen | None = None


class RemoteError(Exception):
    pass


def call(method: str, params: dict | None = None, timeout: float = 10.0):
    """One JSON-RPC call to the running MapEdit."""
    body = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        body["params"] = params
    request = urllib.request.Request(URL, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            reply = json.loads(response.read())
    except (urllib.error.URLError, ConnectionError, TimeoutError) as error:
        raise RemoteError(f"MapEdit is not reachable on {URL} ({error}). Start it with mapedit_launch.") from error
    if "error" in reply:
        raise RemoteError(reply["error"].get("message", str(reply["error"])))
    return reply.get("result")


def running() -> bool:
    try:
        call("mapedit/status", timeout=2.0)
        return True
    except RemoteError:
        return False


def wait_ready(timeout: float) -> dict:
    """Waits for the Map to be loaded and the view to show the editor's working copy."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status = call("mapedit/status", timeout=2.0)
            if status.get("ready") and status.get("editor", {}).get("synced", True):
                return status
        except RemoteError:
            pass
        if _launched is not None and _launched.poll() is not None:
            raise RemoteError(f"MapEdit exited with code {_launched.returncode}; see {LOG}")
        time.sleep(0.5)
    raise RemoteError(f"the Map was not ready after {timeout:.0f} s; see {LOG}")


def launch(args: dict) -> dict:
    global _launched
    if running():
        status = call("mapedit/status")
        if "map" in args and args["map"] != status["map"]:
            call("mapedit/load_map", {"id": args["map"]})
        return wait_ready(120.0)
    command = [os.environ["MAPEDIT_BIN"]] if "MAPEDIT_BIN" in os.environ else ["cargo", "run", "-q", "-p", "mapedit", "--"]
    command += ["--remote-port", str(PORT)]
    for flag in ("map", "look", "mode", "size"):
        if flag in args:
            command += [f"--{flag}", str(args[flag])]
    if args.get("top"):
        command.append("--top")
    log = LOG.open("wb")
    _launched = subprocess.Popen(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    return wait_ready(float(args.get("timeout", 300)))


def screenshot(args: dict) -> dict:
    path = Path(args.get("path") or f"/tmp/mapedit-{int(time.time())}.png").expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    # Show the latest edits: the view reloads after each one.
    wait_ready(float(args.get("timeout", 20)))
    time.sleep(0.5)
    call("mapedit/screenshot", {"path": str(path)})
    # The PNG is written a few frames later on a worker thread: wait for it to appear and settle.
    deadline = time.time() + float(args.get("timeout", 20))
    size = -1
    while time.time() < deadline:
        if path.exists():
            now = path.stat().st_size
            if now > 0 and now == size:
                return {"path": str(path), "bytes": now}
            size = now
        time.sleep(0.4)
    raise RemoteError(f"no screenshot appeared at {path} (is the window minimised or hidden?)")


def quit_(args: dict) -> dict:
    global _launched
    call("mapedit/quit", {"discard": args.get("discard", False)})
    _launched = None
    return {"quitting": True}


NUMBER = {"type": "number"}
VEC3 = {"type": "array", "items": NUMBER, "minItems": 3, "maxItems": 3}

ITEM = {
    "type": "object",
    "description": "An item: {entity: 0, brush: 12} is brush 12 of the worldspawn (entity 0); {entity: 5} is point entity 5. Defaults to the selection. List them with mapedit_items; indices shift when something is deleted.",
    "properties": {"entity": {"type": "integer"}, "brush": {"type": "integer"}},
    "required": ["entity"],
}
SURFACE = {"type": "string", "enum": ["concrete", "plaster", "metal", "corrugated", "wood", "ground", "brick"]}


def method(name: str, description: str, properties: dict | None = None, required: list | None = None, rpc: str | None = None) -> dict:
    """A tool that forwards its arguments to one remote method."""
    schema: dict = {"type": "object", "properties": properties or {}}
    if required:
        schema["required"] = required
    rpc = rpc or f"mapedit/{name}"
    return {"name": f"mapedit_{name}", "description": description, "inputSchema": schema, "run": lambda a: call(rpc, a)}


EDIT_TOOLS = [
    method(
        "new_map",
        "Create a new Map: maps/<id>.map with a 60 x 60 m floor (top at y = 0, centred on the origin), two spawns per Team (Alpha at z = +24 facing north, Bravo at z = -24) and a worldspawn with a look; registered in data/maps.ron for Team Deathmatch only (add objectives, then set its modes with mapedit_map_settings). It is opened for editing.",
        {"id": {"type": "string", "description": "lowercase letters, digits, _"}, "name": {"type": "string"}, "discard": {"type": "boolean", "description": "drop unsaved changes of the open Map"}},
        ["id"],
    ),
    method(
        "save",
        "Write the working copy to the Map's .map file (and its data/maps.ron entry when settings changed or it is new). The answer names `generator` when a tools/mapgen script writes this file: running it overwrites the edits.",
    ),
    method(
        "save_as",
        "Save a copy under a new id, register it in data/maps.ron and open it. The original file stays as it was last saved.",
        {"id": {"type": "string"}, "name": {"type": "string"}},
        ["id"],
    ),
    method(
        "map_settings",
        "Read or change the Map's settings: name, look (null: the Map's own), modes (tdm dom hq sd; empty: every mode) and factions [alpha id, bravo id]. Call without arguments to read. mapedit_save persists them.",
        {
            "name": {"type": "string"},
            "look": {"type": ["string", "null"]},
            "modes": {"type": "array", "items": {"type": "string", "enum": ["tdm", "dom", "hq", "sd"]}},
            "factions": {"type": "array", "items": {"type": "string"}, "minItems": 2, "maxItems": 2},
        },
    ),
    method(
        "palette",
        "What can be placed: surfaces (with texture scale), brush primitives, entity classes with their properties and accepted values, prop models with their bounds, looks, Game Modes and Factions.",
    ),
    method(
        "items",
        "List the brushes and point entities of the open Map with bounds, surface or class and properties (paged: offset, limit, default 100). Filter by kind (brush, entity), class, or near [x,y,z] within radius metres.",
        {
            "kind": {"type": "string", "enum": ["brush", "entity", "all"]},
            "class": {"type": "string"},
            "near": VEC3,
            "radius": NUMBER,
            "offset": {"type": "integer"},
            "limit": {"type": "integer"},
        },
    ),
    method(
        "select",
        "Select something (it is highlighted in the view and is the default target of the edit tools): an `item`, or the first thing a `ray` {origin, direction} hits, or what is under window pixel `screen` [x, y]; `clear: true` deselects.",
        {"item": ITEM, "ray": {"type": "object", "properties": {"origin": VEC3, "direction": VEC3}}, "screen": {"type": "array", "items": NUMBER}, "clear": {"type": "boolean"}},
    ),
    method("move", "Move an item by delta [x,y,z] metres (game space: y up, -z north).", {"item": ITEM, "delta": VEC3}, ["delta"]),
    method(
        "resize",
        "Resize a brush: give `min` and `max` (its new bounding box, metres), or `axis` (x, y, z), `side` (+ or -) and `amount` (metres that face moves outward; negative moves it inward). Exact for boxes and ramps.",
        {"item": ITEM, "min": VEC3, "max": VEC3, "axis": {"type": "string", "enum": ["x", "y", "z"]}, "side": {"type": "string", "enum": ["+", "-"]}, "amount": NUMBER},
    ),
    method("delete", "Delete an item (the worldspawn itself stays).", {"item": ITEM}),
    method("duplicate", "Copy an item, moved by offset (default 1 m in +x); the copy is selected.", {"item": ITEM, "offset": VEC3}),
    method(
        "set_texture",
        "Paint a brush (all its faces) with a surface; the texture keeps its real-world scale. The surface also becomes the default for new brushes.",
        {"item": ITEM, "surface": SURFACE},
        ["surface"],
    ),
    method(
        "set_property",
        "Set an entity key/value, e.g. team (alpha bravo any), modes (\"tdm dom\"), angle (degrees, 0 faces north), label, model, collider. An empty value removes the key.",
        {"item": ITEM, "key": {"type": "string"}, "value": {"type": ["string", "number", "boolean"]}},
        ["key", "value"],
    ),
    method(
        "add_brush",
        "Add a brush to the worldspawn and select it. shape: box (give min and max, or centre and size), wedge (a ramp filling min..max, rising towards `rise`), or the palette primitives wall (4 x 3 x 0.4 m), slab (4 x 0.2 x 4 m), ramp (4 long, 1.5 high, 2 wide, rising towards `rise`), which take `position` (bottom centre) instead of bounds. Metres, game space (y up). The floor of a new Map has its top at y = 0.",
        {
            "shape": {"type": "string", "enum": ["box", "wedge", "wall", "slab", "ramp"]},
            "min": VEC3,
            "max": VEC3,
            "centre": VEC3,
            "size": VEC3,
            "position": VEC3,
            "surface": SURFACE,
            "rise": {"type": "string", "enum": ["+x", "-x", "+z", "-z"]},
        },
        ["shape"],
    ),
    method(
        "add_entity",
        "Add an entity: spawn_point (props team alpha|bravo|any, modes), dom_flag (label A|B|C), hq_site, bomb_site (label A|B), ammo_crate, light_point, prop (model e.g. models/downtown/Prop_Bollard.glb, collider none|box|convex). position is where it stands on the ground (feet / base), metres; angle in degrees (0 faces -z). See mapedit_palette.",
        {"class": {"type": "string"}, "position": VEC3, "angle": NUMBER, "props": {"type": "object"}},
        ["class", "position"],
    ),
    method("undo", "Undo the last edit."),
    method("redo", "Redo the last undone edit."),
    method(
        "ui",
        "Show the Esc menu (menu: main, load, save_as, new, settings, close) and/or the palette panel (palette: true/false, tab: surfaces, brushes, entities, props), e.g. to put them in a screenshot.",
        {"menu": {"type": "string", "enum": ["main", "load", "save_as", "new", "settings", "close"]}, "palette": {"type": "boolean"}, "tab": {"type": "string", "enum": ["surfaces", "brushes", "entities", "props"]}},
    ),
]

TOOLS = [
    {
        "name": "mapedit_launch",
        "description": "Start the MapEdit viewer (or reuse a running one) and wait until its Map is loaded. Opens a window.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "map": {"type": "string", "description": "Map id, e.g. z_town"},
                "look": {"type": "string", "description": "Look preset instead of the Map's own"},
                "mode": {"type": "string", "enum": ["tdm", "dom", "hq", "sd"], "description": "Show only what this Game Mode uses"},
                "top": {"type": "boolean", "description": "Start in the top view"},
                "size": {"type": "string", "description": "Window size, e.g. 1280x720"},
                "timeout": NUMBER,
            },
        },
        "run": lambda a: launch(a),
    },
    {
        "name": "mapedit_status",
        "description": "Map, look, camera, overlays, entity counts, the known Maps and looks, and the editor state (editor.dirty, selection, synced, can_undo, generator script).",
        "inputSchema": {"type": "object", "properties": {}},
        "run": lambda a: call("mapedit/status"),
    },
    {
        "name": "mapedit_load_map",
        "description": "Open another Map of data/maps.ron by id (for viewing and editing). Refused while there are unsaved changes unless discard is true.",
        "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "discard": {"type": "boolean"}}, "required": ["id"]},
        "run": lambda a: call("mapedit/load_map", {"id": a["id"], "discard": a.get("discard", False)}),
    },
    {
        "name": "mapedit_reload",
        "description": "Read the Map's .map file again (it also reloads by itself when the file changes on disk). Refused while there are unsaved changes unless discard is true.",
        "inputSchema": {"type": "object", "properties": {"discard": {"type": "boolean"}}},
        "run": lambda a: call("mapedit/reload", {"discard": a.get("discard", False)}),
    },
    {
        "name": "mapedit_set_look",
        "description": "Show the Map under a look preset (null: the Map's own).",
        "inputSchema": {"type": "object", "properties": {"name": {"type": ["string", "null"]}}, "required": ["name"]},
        "run": lambda a: call("mapedit/set_look", {"name": a["name"]}),
    },
    {
        "name": "mapedit_set_camera",
        "description": (
            "Move the camera. Game space is metres, y up, -z is north. Give a `preset` (top, aerial), or "
            "`position` with `look_at`, or `position` with `yaw` and `pitch` in degrees (yaw 0 looks north)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "preset": {"type": "string", "enum": ["top", "aerial"]},
                "position": VEC3,
                "look_at": VEC3,
                "yaw": NUMBER,
                "pitch": NUMBER,
            },
        },
        "run": lambda a: call("mapedit/set_camera", a),
    },
    {
        "name": "mapedit_set_overlay",
        "description": "Overlay switches: mode (all, tdm, dom, hq, sd), labels, through_walls, colliders (wireframe), fog. All optional.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["all", "tdm", "dom", "hq", "sd"]},
                "labels": {"type": "boolean"},
                "through_walls": {"type": "boolean"},
                "colliders": {"type": "boolean"},
                "fog": {"type": "boolean"},
            },
        },
        "run": lambda a: call("mapedit/set_overlay", a),
    },
    {
        "name": "mapedit_entities",
        "description": "Every spawn point, Flag, HQ site, bomb site and ammo crate with its position in game metres.",
        "inputSchema": {"type": "object", "properties": {}},
        "run": lambda a: call("mapedit/entities"),
    },
    {
        "name": "mapedit_screenshot",
        "description": "Save a PNG of the viewer's window and return its path (read the file to look at it). Default path /tmp/mapedit-<time>.png.",
        "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "timeout": NUMBER}},
        "run": lambda a: screenshot(a),
    },
    {
        "name": "mapedit_quit",
        "description": "Close the viewer (refused with unsaved changes unless discard is true).",
        "inputSchema": {"type": "object", "properties": {"discard": {"type": "boolean"}}},
        "run": quit_,
    },
] + EDIT_TOOLS
BY_NAME = {tool["name"]: tool for tool in TOOLS}


def handle(message: dict) -> dict | None:
    method, ident = message.get("method"), message.get("id")
    if ident is None:  # a notification: nothing to answer
        return None
    if method == "initialize":
        version = message.get("params", {}).get("protocolVersion", PROTOCOL)
        result = {
            "protocolVersion": version,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "mapedit", "version": "0.1.0"},
        }
    elif method == "ping":
        result = {}
    elif method == "tools/list":
        result = {"tools": [{k: v for k, v in tool.items() if k != "run"} for tool in TOOLS]}
    elif method == "tools/call":
        params = message.get("params", {})
        tool = BY_NAME.get(params.get("name"))
        if tool is None:
            return {"jsonrpc": "2.0", "id": ident, "error": {"code": -32602, "message": f"unknown tool {params.get('name')}"}}
        try:
            value = tool["run"](params.get("arguments") or {})
            result = {"content": [{"type": "text", "text": json.dumps(value, indent=2)}]}
        except (RemoteError, KeyError) as error:
            result = {"isError": True, "content": [{"type": "text", "text": f"{type(error).__name__}: {error}"}]}
    else:
        return {"jsonrpc": "2.0", "id": ident, "error": {"code": -32601, "message": f"method not found: {method}"}}
    return {"jsonrpc": "2.0", "id": ident, "result": result}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            reply = handle(json.loads(line))
        except Exception as error:  # keep the server alive whatever a tool does
            reply = {"jsonrpc": "2.0", "id": None, "error": {"code": -32603, "message": str(error)}}
        if reply is not None:
            sys.stdout.write(json.dumps(reply) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
