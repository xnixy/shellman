"""Writes the Islands icon pack from the definitions below. Run from the repo root:

    python tools/islands/make.py

Output (committed, the canonical form of the pack):
    assets/icon-packs/islands/index.theme
    assets/icon-packs/islands/scalable/apps/<name>.svg   the tile, for 32 px and up
    assets/icon-packs/islands/24x24/apps/<name>.svg      the same tile with a heavier, larger glyph,
                                                         for list rows and anything 16-24 px

Every icon is the dock tile of docs/design/desktop-concepts.html (`dock()`): a 46 x 46 square with
13 corner radius, a two-stop gradient at 135 or 160 degrees, a 1 px white top highlight at 20%,
and a 24 x 24 line glyph at (11, 11): stroke 1.8, round caps and joins, no fill, white.
"""

import math
import os
import sys

ROOT = os.path.join(os.path.dirname(__file__), "..", "..", "assets", "icon-packs", "islands")

# Gradients: (css angle, stops). The first twelve are the mockup's own tiles.
PAL = {
    "files": (160, "#7db4ff", "#3f7ff0"),
    "sunset": (135, "#f97316", "#facc15"),
    "night": (160, "#1f2937", "#0b0f17"),
    "indigo": (160, "#818cf8", "#4f46e5"),
    "pink": (135, "#f43f8e", "#7c3aed"),
    "navy": (160, "#1e3a5f", "#0f172a"),
    "grey": (160, "#9ca3af", "#4b5563"),
    "coral": (135, "#ff7a59", "#ffcb6b"),
    "violet": (135, "#5b8cff", "#b06bff"),
    "teal": (135, "#1fd1a5", "#2f7bff"),
    "graphite": (160, "#111827", "#6b7280"),
    "sky": (135, "#0ea5e9", "#22d3ee"),
    # A few more in the same key, for the categories.
    "green": (135, "#34d399", "#0f9f6e"),
    "rose": (135, "#fb7185", "#e11d48"),
    "amber": (160, "#fbbf24", "#e8790c"),
    "steel": (160, "#94a3b8", "#475569"),
    "ocean": (160, "#60a5fa", "#1d4ed8"),
    "leaf": (160, "#4ade80", "#15803d"),
    "ember": (160, "#fb923c", "#c2410c"),
    "stone": (160, "#a8a29e", "#57534e"),
    "plum": (135, "#c084fc", "#7c3aed"),
    "mint": (135, "#2dd4bf", "#0e7490"),
    # Apps' own colours.
    "firefox": (135, "#ffb43a", "#ff4f5e", "#8f4cf5"),
    "chrome": (135, "#ea4335", "#fbbc04", "#34a853"),
    "edge": (135, "#3fd08a", "#1b9de2", "#0b4ea2"),
    "brave": (160, "#ff7a3d", "#ec2f1c"),
    "discord": (160, "#8790f8", "#5865f2"),
    "steam": (160, "#16243a", "#1a7fbf"),
    "spotify": (160, "#2ee06f", "#139c45"),
    "vscode": (160, "#3fb0f5", "#0062a8"),
    "slack": (135, "#7a2a7f", "#3f0e40"),
    "telegram": (160, "#41c0fc", "#1b8fcf"),
    "whatsapp": (160, "#5fd769", "#1da63a"),
    "zoom": (160, "#5b9bff", "#0b5cff"),
    "obs": (160, "#46454b", "#131316"),
    "vlc": (160, "#ffab40", "#f26a00"),
    "notion": (160, "#ffffff", "#dedee3"),
    "obsidian": (135, "#b79cff", "#6c31e3"),
    "github": (160, "#a27cf2", "#6639ba"),
    "qbittorrent": (160, "#6aa9f2", "#2c64b6"),
    "onepassword": (160, "#3fa2ff", "#0057d9"),
    "tailscale": (160, "#4a4a4a", "#181818"),
    "zed": (160, "#4f7df0", "#14286b"),
    "git": (135, "#f6774c", "#d23a1a"),
    "xbox": (160, "#3cb43c", "#0b6e0b"),
}

# Glyphs on the 24 x 24 grid. `D` is a solid white dot (the mockup's `fill="currentColor"`).
def D(x, y, r=0.9, o=None):
    op = f' fill-opacity="{o}"' if o else ""
    return f'<circle cx="{x}" cy="{y}" r="{r}" fill="#fff" stroke="none"{op}/>'


G = {
    # The mockup's own glyphs, unchanged.
    "folder": '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v8a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z"/>',
    "term": '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M7.5 9.5l3 2.5-3 2.5M13 15h4"/>',
    "image": '<rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="1.8"/><path d="M21 16l-5-5-9 9"/>',
    "grid": '<rect x="4" y="4" width="7" height="7" rx="2"/><rect x="13" y="4" width="7" height="7" rx="2"/><rect x="4" y="13" width="7" height="7" rx="2"/><rect x="13" y="13" width="7" height="7" rx="2"/>',
    "sliders": '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
    "trash": '<path d="M4 7h16M9.5 7V4.5h5V7M6 7l1 13h10l1-13"/>',
    "trash-full": '<path d="M6 7l1 13h10l1-13z" fill="#fff" fill-opacity=".3" stroke="none"/><path d="M4 7h16M9.5 7V4.5h5V7M6 7l1 13h10l1-13"/><path d="M10 11v5.5M14 11v5.5"/>',
    "globe": '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.6 2.6 2.6 14.4 0 17M12 3.5c-2.6 2.6-2.6 14.4 0 17"/>',
    "chat": '<path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7a2.5 2.5 0 0 1-2.5 2.5H10l-4.5 4v-4A2.5 2.5 0 0 1 4 13.5z"/>',
    "music": '<path d="M9 18V6l11-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>',
    "gear": '<circle cx="12" cy="12" r="3"/><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"/>',
    "game": '<rect x="2.5" y="7" width="19" height="11" rx="5.5"/><path d="M7 10.5v4M5 12.5h4"/>' + D(15.5, 11.5) + D(17.5, 13.5),
    "doc": '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h4"/>',
    "cal": '<rect x="3.5" y="5" width="17" height="15.5" rx="3"/><path d="M3.5 10h17M8 3v4M16 3v4"/>',
    "clock": '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>',
    "monitor": '<rect x="3" y="4" width="18" height="12" rx="2.5"/><path d="M8 20h8M12 16v4"/>',
    "down": '<path d="M12 4v11M7 10.5l5 5 5-5M5 20h14"/>',
    # Drawn for the pack, in the same hand.
    "pulse": '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M6.5 12.5h2.5l1.5-3.5 3 6.5 1.5-3h2.5"/>',
    "mail": '<rect x="3" y="5" width="18" height="14" rx="3"/><path d="M4 7.5l8 5.5 8-5.5"/>',
    "play": '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M10 9v6l5-3z"/>',
    "calc": '<rect x="5" y="3" width="14" height="18" rx="3"/><path d="M8.5 7.5h7"/>' + D(9, 12) + D(12, 12) + D(15, 12) + D(9, 16) + D(12, 16) + D(15, 16),
    "camera": '<path d="M4 9a2.5 2.5 0 0 1 2.5-2.5h1.5L9.5 4.5h5L16 6.5h1.5A2.5 2.5 0 0 1 20 9v8a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 17z"/><circle cx="12" cy="13" r="3.5"/>',
    "note": '<path d="M19 14V6.5A2.5 2.5 0 0 0 16.5 4h-9A2.5 2.5 0 0 0 5 6.5v11A2.5 2.5 0 0 0 7.5 20H13z"/><path d="M13 20v-3.5a2.5 2.5 0 0 1 2.5-2.5H19M8.5 9h7M8.5 12.5h4"/>',
    "map": '<path d="M3.5 6.5l5.5-2 6 2 5.5-2v13l-5.5 2-6-2-5.5 2z"/><path d="M9 4.5v13M15 6.5v13"/>',
    "bag": '<path d="M5 8.5h14l-1.1 10a2 2 0 0 1-2 1.8H8.1a2 2 0 0 1-2-1.8z"/><path d="M9 11V7.5a3 3 0 0 1 6 0V11"/>',
    "box": '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M12 4v1.5M12 7.5V9M12 11v1.5"/><rect x="10" y="14" width="4" height="3.5" rx="1"/>',
    "code": '<path d="M8.5 7.5 4 12l4.5 4.5M15.5 7.5 20 12l-4.5 4.5M13.5 5l-3 14"/>',
    "remote": '<rect x="3" y="4" width="18" height="12" rx="2.5"/><path d="M8 20h8M12 16v4M10 8h4v4M14 8l-4.5 4.5"/>',
    "cube": '<path d="M12 3.5l7.5 4.2v8.6L12 20.5l-7.5-4.2V7.7z"/><path d="M12 12l7.5-4.3M12 12v8.5M12 12 4.5 7.7"/>',
    "crop": '<path d="M7 3.5v11A2.5 2.5 0 0 0 9.5 17h11M3.5 7h11A2.5 2.5 0 0 1 17 9.5v11"/>',
    "palette": '<path d="M12 3.5a8.5 8.5 0 0 0 0 17c1.4 0 2-1 1.5-2.2-.5-1.3.3-2.3 1.7-2.3H17a3.5 3.5 0 0 0 3.5-3.5c0-5-3.8-9-8.5-9z"/>' + D(8, 11, 1.1) + D(10.5, 7.5, 1.1) + D(15, 8, 1.1),
    "mic": '<rect x="9" y="3.5" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v2.5"/>',
    "key": '<circle cx="9" cy="15" r="4.5"/><path d="M12.2 11.8 19.5 4.5M17 7l2.5 2.5M14.6 9.4l2 2"/>',
    "sheet": '<rect x="4" y="4" width="16" height="16" rx="3"/><path d="M4 9.5h16M4 14.5h16M10 4v16"/>',
    "slides": '<rect x="3.5" y="4" width="17" height="12" rx="2.5"/><path d="M12 16v3.5M8.5 20h7M8 12.5l2.5-3 2 2 3.5-4"/>',
    "pdf": '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M8.5 17.5c2-1 4.5-5 3.5-7.5-.6-1.4-2-.5-1.2 1.3.9 2.2 3 4.5 5.2 4.6"/>',
    "access": '<circle cx="12" cy="12" r="8.5"/>' + D(12, 7.6, 1.2) + '<path d="M8 10h8M12 10v3.5l-2.5 4M12 13.5l2.5 4"/>',
    "wrench": '<path d="M14.8 4.2a4.5 4.5 0 0 0-4.5 6.1L4.6 16a1.9 1.9 0 0 0 2.7 2.7l5.7-5.7a4.5 4.5 0 0 0 6.1-4.5l-2.8 2.8-2.4-.4-.4-2.4z"/>',
    "shield": '<path d="M12 3.5l7 2.6v5.4c0 4.4-2.9 7.6-7 9-4.1-1.4-7-4.6-7-9V6.1z"/><path d="M9 12l2.2 2.2L15.5 10"/>',
    # Apps' own marks, redrawn as glyphs of this pack: the idea of the mark, not the mark.
    "firefox": '<circle cx="12.6" cy="12.8" r="4.6" fill="#fff" fill-opacity=".28"/><path d="M15.4 3.9c3.5 1.1 5.6 4.5 5.1 8.4-.6 4.6-4.7 8.1-9.3 7.7-4.6-.4-7.9-4.4-7.5-9 .2-2.1 1.1-3.9 2.5-5.3"/><path d="M6.2 5.7c.1 1.9 1 3 2.6 3.4M15.4 3.9c-1.2 1.2-1.6 2.6-1 4"/>',
    "chrome": '<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="3.3"/><path d="M12 8.7h8M9.1 13.7l-4-6.9M14.9 13.7l-4 6.9"/>',
    "edge": '<path d="M20.4 13.4A8.5 8.5 0 1 1 18.8 7"/><path d="M20.4 13.4c-.1-3.3-3.4-5.3-6.9-4.7-3 .5-4.6 3.4-3.1 5.8 1.4 2.3 5.1 2.6 7.3.6"/>',
    "brave": '<path d="M8.2 4h7.6l2.4 2.5 1.6-.4-.6 2.6.6 1.6-2.3 7.1L12 20.5l-5.1-3.1-2.3-7.1.6-1.6-.6-2.6 1.6.4z"/><path d="M10 14.5l2 1.5 2-1.5M9.5 10.5l1.5 1M14.5 10.5l-1.5 1"/>',
    "discord": '<path d="M8.4 6.6 9 5.2c-1.6.2-3 .7-4.1 1.4C3 9.5 2.5 12.4 2.8 15.3c1.5 1.1 3.1 1.8 4.6 2.3l1.1-1.7M15.6 6.6 15 5.2c1.6.2 3 .7 4.1 1.4 1.9 2.9 2.4 5.8 2.1 8.7-1.5 1.1-3.1 1.8-4.6 2.3l-1.1-1.7M7.2 15.2c3 1.3 6.6 1.3 9.6 0M7.6 7.6c2.9-1 5.9-1 8.8 0"/>' + D(9.3, 12, 1.5) + D(14.7, 12, 1.5),
    "steam": '<circle cx="15.8" cy="8.8" r="4.4"/>' + D(15.8, 8.8, 1.6) + '<circle cx="9" cy="15.6" r="3"/><path d="M11.2 13.5l1.6-1.7M6.2 14.5 3.5 13.4"/>',
    "spotify": '<circle cx="12" cy="12" r="8.5"/><path d="M7 9.4c3.6-1.1 7.4-.8 10.4.9M7.6 12.6c3-.8 6-.5 8.5.9M8.3 15.6c2.3-.6 4.6-.4 6.6.6"/>',
    "vscode": '<path d="M16.5 4 7.4 12.6 4 10M16.5 20 7.4 11.4 4 14M16.5 4l3.5 1.7v12.6L16.5 20zM4 10v4"/>',
    "slack": '<path d="M10 4v5.5M14 14.5V20M14.5 10H20M4 14h5.5"/>' + D(14, 5.6, 1.6) + D(18.4, 14, 1.6) + D(10, 18.4, 1.6) + D(5.6, 10, 1.6),
    "telegram": '<path d="M20.5 4.5 3.7 11.1c-.7.3-.7 1.2 0 1.4l4.1 1.3 1.6 5c.2.6.9.8 1.4.3l2.4-2.3 4.3 3.2c.6.4 1.3.1 1.5-.6l2.7-12.8c.2-.8-.6-1.4-1.2-1.1z"/><path d="M7.8 13.8 17.2 8l-6.6 6.4"/>',
    "whatsapp": '<path d="M4.5 19.5l1.2-3.8A8.5 8.5 0 1 1 8.4 18.4z"/><path d="M9.3 8.3c.4-.4.9-.3 1.1.1l.7 1.5c.1.3 0 .6-.2.8l-.5.5c.6 1.2 1.5 2.1 2.6 2.6l.5-.5c.2-.2.5-.3.8-.2l1.5.7c.4.2.5.7.1 1.1-1.1 1.1-2.6 1.1-4.5-.3-1.6-1.2-2.7-2.9-2.7-4.1 0-.9.3-1.7.6-2.2z"/>',
    "zoom": '<rect x="3" y="7" width="12.5" height="10" rx="3"/><path d="M15.5 11l5-3v8l-5-3"/>',
    "obs": '<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="8" r="2.4"/><circle cx="8.5" cy="14" r="2.4"/><circle cx="15.5" cy="14" r="2.4"/>',
    "vlc": '<path d="M10 4h4l4.6 14H5.4z"/><path d="M8.6 8.7h6.8M7.1 13.3h9.8M3.5 20h17"/>',
    "notion": '<path d="M5 6.5 15.8 4 19 6.6v12.2l-11 1.2L5 17.4z"/><path d="M9.5 9.5v7.5M9.5 9.5l5 7.5M14.5 9v8"/>',
    "obsidian": '<path d="M9.6 3.5 16.2 5.6l3 6.8-4.6 8.1-6.4-1.4-3.4-5.7z"/><path d="M9.6 3.5 10.2 11l-5.4 2.4M10.2 11l9 1.4M10.2 11l4.4 9.5"/>',
    "github": '<path d="M5.6 9.8 5 4.5l4.2 2.2a9.6 9.6 0 0 1 5.6 0L19 4.5l-.6 5.3a6.8 6.8 0 0 1 1.1 3.7c0 4.2-3.3 6.5-7.5 6.5s-7.5-2.3-7.5-6.5a6.8 6.8 0 0 1 1.1-3.7z"/>' + D(9.3, 13.4, 1.1) + D(14.7, 13.4, 1.1),
    "qbittorrent": '<circle cx="12" cy="12" r="8.5"/><path d="M12 7v8.5M8.5 12l3.5 3.5 3.5-3.5"/>',
    "onepassword": '<circle cx="12" cy="12" r="8.5"/><path d="M10.5 7.5h2v3.6l-1 .9 1 .9v3.6"/>',
    "tailscale": D(6.5, 6.5, 2, ".38") + D(12, 6.5, 2, ".38") + D(17.5, 6.5, 2, ".38") + D(6.5, 12, 2) + D(12, 12, 2) + D(17.5, 12, 2) + D(6.5, 17.5, 2, ".38") + D(12, 17.5, 2) + D(17.5, 17.5, 2, ".38"),
    "zed": '<rect x="3.5" y="3.5" width="17" height="17" rx="2.5"/><path d="M8 8.5h8l-8 7h8"/>',
    "git": '<path d="M10.6 3.6a2 2 0 0 1 2.8 0l7 7a2 2 0 0 1 0 2.8l-7 7a2 2 0 0 1-2.8 0l-7-7a2 2 0 0 1 0-2.8z"/><path d="M8.5 6.5l3 3v5M11.5 9.5l3 3"/>' + D(11.5, 9.5, 1.1) + D(11.5, 15, 1.1) + D(14.5, 12.5, 1.1),
    "xbox": '<circle cx="12" cy="12" r="8.5"/><path d="M6.5 6.4c2.5 1.6 6.9 6.9 10.5 12.2M17.5 6.4C15 8 10.6 13.3 7 18.6"/>',
}

# The pack: file name, glyph, gradient, and an optional dark glyph for a light tile.
ICONS = [
    # shellman's own apps.
    ("shellman-files", "folder", "files"),
    ("shellman-settings", "gear", "grey"),
    ("shellman-terminal", "term", "night"),
    ("shellman-tasks", "pulse", "teal"),
    ("shellman-launcher", "grid", "violet"),
    ("shellman-control", "sliders", "indigo"),
    ("user-trash", "trash", "steel"),
    ("user-trash-full", "trash-full", "steel"),
    # Categories: what any app of the kind gets when it has no tile of its own.
    ("web-browser", "globe", "sky"),
    ("utilities-terminal", "term", "graphite"),
    ("accessories-text-editor", "doc", "ocean"),
    ("system-file-manager", "folder", "amber"),
    ("internet-mail", "mail", "ocean"),
    ("office-calendar", "cal", "rose"),
    ("internet-chat", "chat", "indigo"),
    ("multimedia-audio-player", "music", "pink"),
    ("multimedia-video-player", "play", "rose"),
    ("multimedia-photo-manager", "image", "teal"),
    ("applications-games", "game", "navy"),
    ("accessories-calculator", "calc", "graphite"),
    ("camera", "camera", "night"),
    ("clock", "clock", "plum"),
    ("accessories-notes", "note", "amber"),
    ("maps", "map", "green"),
    ("software-store", "bag", "ocean"),
    ("preferences-system", "gear", "steel"),
    ("utilities-system-monitor", "pulse", "green"),
    ("archive-manager", "box", "stone"),
    ("document-viewer", "pdf", "rose"),
    ("code-editor", "code", "violet"),
    ("preferences-desktop-remote-desktop", "remote", "mint"),
    ("virtual-machine", "cube", "ocean"),
    ("accessories-screenshot-tool", "crop", "pink"),
    ("accessories-drawing", "palette", "coral"),
    ("audio-input-microphone", "mic", "rose"),
    ("download-manager", "down", "mint"),
    ("password-manager", "key", "graphite"),
    ("x-office-document", "doc", "ocean"),
    ("x-office-spreadsheet", "sheet", "leaf"),
    ("x-office-presentation", "slides", "ember"),
    ("preferences-desktop-accessibility", "access", "ocean"),
    ("administration", "wrench", "steel"),
    ("preferences-system-privacy", "shield", "mint"),
    # Well-known apps, in their own colours.
    ("firefox", "firefox", "firefox"),
    ("google-chrome", "chrome", "chrome"),
    ("microsoft-edge", "edge", "edge"),
    ("brave-browser", "brave", "brave"),
    ("discord", "discord", "discord"),
    ("steam", "steam", "steam"),
    ("spotify", "spotify", "spotify"),
    ("visual-studio-code", "vscode", "vscode"),
    ("slack", "slack", "slack"),
    ("telegram", "telegram", "telegram"),
    ("whatsapp", "whatsapp", "whatsapp"),
    ("zoom", "zoom", "zoom"),
    ("com.obsproject.studio", "obs", "obs"),
    ("vlc", "vlc", "vlc"),
    ("notion", "notion", "notion", "#1f1f23"),
    ("obsidian", "obsidian", "obsidian"),
    ("github-desktop", "github", "github"),
    ("qbittorrent", "qbittorrent", "qbittorrent"),
    ("1password", "onepassword", "onepassword"),
    ("tailscale", "tailscale", "tailscale"),
    ("zed", "zed", "zed"),
    ("git", "git", "git"),
    ("xbox", "xbox", "xbox"),
]

TILE = 46
RADIUS = 13
GLYPH = 24


def gradient(angle, stops):
    """CSS linear-gradient(angle, ...) on the tile, as SVG user-space end points."""
    a = math.radians(angle)
    dx, dy = math.sin(a), -math.cos(a)
    half = (abs(TILE * dx) + abs(TILE * dy)) / 2
    c = TILE / 2
    f = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")
    x1, y1, x2, y2 = c - dx * half, c - dy * half, c + dx * half, c + dy * half
    n = len(stops)
    s = "".join(f'<stop offset="{i / (n - 1):g}" stop-color="{col}"/>' for i, col in enumerate(stops))
    return f'<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="{f(x1)}" y1="{f(y1)}" x2="{f(x2)}" y2="{f(y2)}">{s}</linearGradient>'


def svg(glyph, pal, ink=None, small=False):
    angle, *stops = PAL[pal]
    ink = ink or "#fff"
    # Small: the glyph a fifth bigger and its line 2.6 instead of 1.8, so it holds at 16 px.
    scale = 1.2 if small else 1.0
    width = 2.6 if small else 1.8
    off = (TILE - GLYPH * scale) / 2
    body = G[glyph]
    if ink != "#fff":
        body = body.replace('fill="#fff"', f'fill="{ink}"')
    tf = f"translate({off:g} {off:g})" + (f" scale({scale:g})" if scale != 1 else "")
    tile = f'<rect width="{TILE}" height="{TILE}" rx="{RADIUS}" fill="url(#g)"/>'
    # The top highlight: the tile minus itself moved down 1, clipped to the tile (`inset 0 1px 0 #ffffff33`).
    shine = (
        f'<path clip-path="url(#c)" fill="#fff" fill-opacity=".2" fill-rule="evenodd" '
        f'd="M{RADIUS} 0h{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1 {RADIUS} {RADIUS}v{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1-{RADIUS} {RADIUS}h-{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1-{RADIUS}-{RADIUS}v-{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1 {RADIUS}-{RADIUS}z'
        f'M{RADIUS} 1h{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1 {RADIUS} {RADIUS}v{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1-{RADIUS} {RADIUS}h-{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1-{RADIUS}-{RADIUS}v-{TILE - 2 * RADIUS}a{RADIUS} {RADIUS} 0 0 1 {RADIUS}-{RADIUS}z"/>'
    )
    clip = f'<clipPath id="c"><rect width="{TILE}" height="{TILE}" rx="{RADIUS}"/></clipPath>'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{TILE}" height="{TILE}" viewBox="0 0 {TILE} {TILE}">'
        f"<defs>{gradient(angle, stops)}{clip}</defs>{tile}{shine}"
        f'<g transform="{tf}" fill="none" stroke="{ink}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round">{body}</g>'
        "</svg>\n"
    )


THEME = """[Icon Theme]
Name=Islands
Comment=shellman's default icons: glass-island tiles with white line glyphs
Directories=scalable/apps,24x24/apps

[scalable/apps]
Size=46
MinSize=32
MaxSize=512
Type=Scalable
Context=Applications

[24x24/apps]
Size=24
MinSize=16
MaxSize=31
Type=Scalable
Context=Applications
"""


def main():
    names = [i[0] for i in ICONS]
    dupes = {n for n in names if names.count(n) > 1}
    if dupes:
        sys.exit(f"twice: {dupes}")
    big = os.path.join(ROOT, "scalable", "apps")
    small = os.path.join(ROOT, "24x24", "apps")
    for d in (big, small):
        os.makedirs(d, exist_ok=True)
        for f in os.listdir(d):
            if f.endswith(".svg"):
                os.remove(os.path.join(d, f))
    for name, glyph, pal, *ink in ICONS:
        ink = ink[0] if ink else None
        for d, s in ((big, False), (small, True)):
            with open(os.path.join(d, name + ".svg"), "w", newline="\n") as fh:
                fh.write(svg(glyph, pal, ink, s))
    with open(os.path.join(ROOT, "index.theme"), "w", newline="\n") as fh:
        fh.write(THEME)
    print(f"{len(ICONS)} icons")


if __name__ == "__main__":
    main()
