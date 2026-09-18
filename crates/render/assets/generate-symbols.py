#!/usr/bin/env python3
"""Rebuild the original monochrome symbol set (requires cairosvg and pillow).

Each symbol is authored on a 24x24 grid, rasterized to a 96px alpha mask and tinted
by the renderer (`Primitive::Symbol`). Original artwork, repository MIT license.
Also writes src/symbols.rs, the embedded lookup table.
"""
import io
import math
import re
from pathlib import Path
import cairosvg
from PIL import Image

HERE = Path(__file__).resolve().parent
OUT = HERE / "symbols"
S = 'fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"'
F = 'fill="#fff"'


def arc(cx, cy, r, a0, a1):
    """SVG arc from angle a0 to a1 (degrees clockwise from 12 o'clock)."""
    p = lambda a: (cx + r * math.sin(math.radians(a)), cy - r * math.cos(math.radians(a)))
    (x0, y0), (x1, y1) = p(a0), p(a1)
    return f"M{x0:.2f} {y0:.2f}A{r} {r} 0 {int(abs(a1 - a0) > 180)} 1 {x1:.2f} {y1:.2f}"


def gear(teeth=8, outer=10.2, inner=7.6, hole=3.4):
    pts = []
    for i in range(teeth):
        base = 360 / teeth * i
        for da, r in ((-13, inner), (-8, outer), (8, outer), (13, inner)):
            a = math.radians(base + da)
            pts.append(f"{12 + r * math.sin(a):.2f} {12 - r * math.cos(a):.2f}")
    ring = f"M{12 + hole} 12a{hole} {hole} 0 1 0 {-2 * hole} 0a{hole} {hole} 0 1 0 {2 * hole} 0Z"
    return f'<path {F} fill-rule="evenodd" d="M{"L".join(pts)}Z{ring}"/>'


def sun(rays=8):
    lines = "".join(
        f"M{12 + 7.2 * math.sin(a):.2f} {12 - 7.2 * math.cos(a):.2f}L{12 + 9.6 * math.sin(a):.2f} {12 - 9.6 * math.cos(a):.2f}"
        for a in (math.radians(360 / rays * i) for i in range(rays)))
    return f'<circle cx="12" cy="12" r="4.2" {F}/><path {S} d="{lines}"/>'


SYMBOLS = {
    "fruit": f'<path {F} d="M12.1 7.4c1.7-1.6 4.2-1.7 5.9-.2-1.5 1-2.2 2.4-2.1 4 .1 1.8 1.1 3.2 2.7 3.9-.6 1.8-1.5 3.4-2.7 4.7-1 1.1-2.1 1.3-3.3.7-1.1-.5-2-.5-3.1 0-1.3.6-2.3.3-3.2-.7-2.5-2.8-3.6-6.7-2.2-9.6 1-2.1 2.9-3.2 4.9-3.1 1.1.1 2.2.5 3.1 1.100Z"/><path {F} d="M12 6.300c-.2-2 1.3-3.8 3.5-4.2.3 2.1-1.3 4-3.5 4.200Z"/>',
    "windows": "".join(f'<rect x="{2.5 + c * 9.8}" y="{2.5 + r * 9.8}" width="9.2" height="9.2" rx=".8" {F}/>' for r in range(2) for c in range(2)),
    "new-tab": f'<rect x="3" y="3" width="18" height="18" rx="3.5" {S}/><path {S} d="M12 8v8M8 12h8"/>',
    "pin": f'<path {S} d="m14.5 3 6.5 6.5-3 1-3.5 3.5.5 4-2 2L4 11l2-2 4 .500L13.5 6Zm-6 12.500L3.5 20.5"/>',
    "eject": f'<path {F} d="M12 4.5 20 14H4Z"/><rect x="4" y="16.5" width="16" height="3" rx="1" {F}/>',
    "info": f'<circle cx="12" cy="12" r="9" {S}/><path {S} d="M12 11v6"/><circle cx="12" cy="7.6" r="1.3" {F}/>',
    "backspace": f'<path {S} d="M8.5 5H19a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H8.500L3 12Zm3.5 4.5 5 5m0-5-5 5"/>',
    "night-light": f'<path {F} d="M12 3a7 7 0 0 0-4 12.700V18a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1v-2.300A7 7 0 0 0 12 3ZM9.5 20.500h5v.500a1 1 0 0 1-1 1h-3a1 1 0 0 1-1-1Z"/>',
    "screenshot": f'<path {S} d="M3.5 8V5.500a2 2 0 0 1 2-2H8m8 0h2.500a2 2 0 0 1 2 2V8m0 8v2.500a2 2 0 0 1-2 2H16m-8 0H5.500a2 2 0 0 1-2-2V16"/><circle cx="12" cy="12" r="3" {F}/>',
    "location": f'<path {F} d="M20.5 3.5 3.5 10.800l6.7 2.1.9.9 2.1 6.700Z"/>',
    "dnd": f'<circle cx="12" cy="12" r="9" {S}/><path {S} d="M7.5 12h9"/>',
    "wallet": f'<rect x="2.5" y="5.5" width="19" height="14" rx="2.5" {S}/><path {S} d="M2.5 10h19M6 15h4"/>',
    "paperclip": f'<path {S} d="m20 11.5-8.3 8.300a5 5 0 0 1-7.1-7.100l8.5-8.500a3.3 3.3 0 0 1 4.7 4.700l-8.4 8.400a1.7 1.7 0 0 1-2.4-2.400l7.7-7.7"/>',
    "reply": f'<path {S} d="M9.5 5 3.5 11l6 6M3.5 11H13a7.5 7.5 0 0 1 7.5 7.5"/>',
    "archive": f'<rect x="2.5" y="4" width="19" height="5" rx="1.5" {S}/><path {S} d="M4.5 9v9.500A1.5 1.5 0 0 0 6 20h12a1.5 1.5 0 0 0 1.5-1.500V9M9.5 13h5"/>',
    "flag": f'<path {S} d="M5 21V4m0 1c5-2.5 8 2.5 14 0v9c-6 2.5-9-2.5-14 0"/>',
    "calendar": f'<rect x="3" y="4.5" width="18" height="16.5" rx="2.5" {S}/><path {S} d="M3 9.500h18M8 2.500v4M16 2.500v4"/>',
    "chat": f'<path {S} d="M12 3.500c5 0 9 3.4 9 7.700s-4 7.7-9 7.700c-1 0-2-.1-2.9-.400L4.5 20.500l1.2-3.800C4 15.3 3 13.4 3 11.200c0-4.3 4-7.7 9-7.700Z"/>',
    "terminal": f'<rect x="2.5" y="4" width="19" height="16" rx="2.5" {S}/><path {S} d="m6.5 9 3.5 3-3.5 3M12.5 15.500h5"/>',
    "wifi": f'<path {S} stroke-width="2.3" d="{arc(12, 19.5, 14, -45, 45)}{arc(12, 19.5, 9.3, -45, 45)}{arc(12, 19.5, 4.6, -45, 45)}"/><circle cx="12" cy="19.3" r="1.7" {F}/>',
    "wifi-fill": f'<path {F} d="M12 21.5 0.9 8.2a16.5 16.5 0 0 1 22.2 0Z"/>',
    "cellular": "".join(f'<rect x="{2.5 + i * 5.2}" y="{16 - i * 3.6}" width="3.6" height="{5 + i * 3.6}" rx="1" {F}/>' for i in range(4)),
    "signal": f'<path {F} d="M2.5 21.5 21.5 2.5v19Z"/>',
    "battery": f'<rect x="1.5" y="7" width="19" height="10" rx="3" fill="none" stroke="#fff" stroke-opacity=".5" stroke-width="1.2"/><rect x="3.3" y="8.8" width="15.4" height="6.4" rx="1.6" {F}/><path {F} fill-opacity=".55" d="M21.7 10.2c1 .3 1.5 1 1.5 1.8s-.5 1.5-1.5 1.8Z"/>',
    "battery-vertical": f'<path {F} d="M9.5 2h5v2H17a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h2.5Z"/>',
    "bluetooth": f'<path {S} d="m6.5 7.5 11 9-5.5 5v-19l5.5 5-11 9"/>',
    "volume": f'<path {F} d="M3 9.5h3.5L12 5v14l-5.5-4.5H3Z"/><path {S} d="{arc(12, 12, 4.5, 40, 140)}{arc(12, 12, 8.5, 40, 140)}"/>',
    "volume-mute": f'<path {F} d="M3 9.5h3.5L12 5v14l-5.5-4.5H3Z"/><path {S} d="m16 9.5 5 5m0-5-5 5"/>',
    "search": f'<circle cx="10.5" cy="10.5" r="6.5" {S}/><path {S} d="m15.5 15.5 5.5 5.5"/>',
    "chevron-left": f'<path {S} d="M15 4.5 7.5 12l7.5 7.5"/>',
    "chevron-right": f'<path {S} d="M9 4.5l7.5 7.5L9 19.5"/>',
    "chevron-up": f'<path {S} d="M4.5 15 12 7.5l7.5 7.5"/>',
    "chevron-down": f'<path {S} d="M4.5 9l7.5 7.5L19.5 9"/>',
    "arrow-left": f'<path {S} d="M20 12H4.5m6.5-7-7 7 7 7"/>',
    "arrow-right": f'<path {S} d="M4 12h15.5M13 5l7 7-7 7"/>',
    "arrow-up": f'<path {S} d="M12 20V4.5M5 11.5l7-7 7 7"/>',
    "reload": f'<path {S} d="{arc(12, 12, 8, 60, 360)}"/><path {S} d="M12 1.5 15.5 4 12 7"/>',
    "close": f'<path {S} d="m5 5 14 14M19 5 5 19"/>',
    "plus": f'<path {S} d="M12 4v16M4 12h16"/>',
    "check": f'<path {S} d="m4.5 12.5 5 5L19.5 7"/>',
    "more": "".join(f'<circle cx="{5 + i * 7}" cy="12" r="1.8" {F}/>' for i in range(3)),
    "more-vertical": "".join(f'<circle cx="12" cy="{5 + i * 7}" r="1.8" {F}/>' for i in range(3)),
    "menu": f'<path {S} d="M4 6.5h16M4 12h16M4 17.5h16"/>',
    "grid": "".join(f'<rect x="{3.5 + c * 6.5}" y="{3.5 + r * 6.5}" width="4" height="4" rx="1" {F}/>' for r in range(3) for c in range(3)),
    "grid-view": "".join(f'<rect x="{4 + c * 9}" y="{4 + r * 9}" width="7" height="7" rx="1.6" {S}/>' for r in range(2) for c in range(2)),
    "list-view": f'<path {S} d="M8.5 6h12M8.5 12h12M8.5 18h12"/>' + "".join(f'<circle cx="4" cy="{6 + i * 6}" r="1.3" {F}/>' for i in range(3)),
    "sidebar": f'<rect x="2.5" y="4.5" width="19" height="15" rx="3" {S}/><path {S} d="M9 4.5v15M5 8.5h1.5M5 11.5h1.5"/>',
    "control-center": f'<rect x="2.5" y="4.5" width="19" height="6" rx="3" {S} stroke-width="1.7"/><circle cx="6.2" cy="7.5" r="2.2" {F}/><rect x="2.5" y="13.5" width="19" height="6" rx="3" {S} stroke-width="1.7"/><circle cx="17.8" cy="16.5" r="2.2" {F}/>',
    "gear": gear(),
    "sun": sun(),
    "moon": f'<path {F} d="M20.5 14.8A9 9 0 1 1 9.2 3.5a7.2 7.2 0 0 0 11.3 11.3Z"/>',
    "bell": f'<path {F} d="M12 2.5a6.5 6.5 0 0 0-6.5 6.5v4.2L3.6 17a.8.8 0 0 0 .7 1.2h15.4a.8.8 0 0 0 .7-1.2l-1.9-3.8V9A6.5 6.5 0 0 0 12 2.5Zm-2.6 17a2.7 2.7 0 0 0 5.2 0Z"/>',
    "airplane": f'<path {F} d="M12 2c.9 0 1.6 1 1.6 2.4V9l8.4 5v2.2l-8.4-2.6v4.6l2.4 1.8V22L12 20.8 8 22v-2l2.4-1.8v-4.6L2 16.2V14l8.4-5V4.4C10.4 3 11.1 2 12 2Z"/>',
    "lock": f'<rect x="5" y="10.5" width="14" height="10.5" rx="2.4" {F}/><path {S} d="M8 10.5V7.8a4 4 0 0 1 8 0v2.7"/>',
    "rotate-lock": f'<rect x="7.5" y="11" width="9" height="7" rx="1.5" {F}/><path {S} stroke-width="1.6" d="M9.5 11V9.5a2.5 2.5 0 0 1 5 0V11"/><path {S} stroke-width="1.7" d="{arc(12, 13, 10, 25, 335)}"/>',
    "power": f'<path {S} d="M12 3v9"/><path {S} d="{arc(12, 13, 8, 35, 325)}"/>',
    "mic": f'<rect x="9" y="2.5" width="6" height="11.5" rx="3" {F}/><path {S} d="M5.5 11.5a6.5 6.5 0 0 0 13 0M12 18v3.5"/>',
    "camera": f'<path {F} fill-rule="evenodd" d="M8.2 5 9.6 3h4.8l1.4 2H19a2.5 2.5 0 0 1 2.5 2.5v10A2.5 2.5 0 0 1 19 20H5a2.5 2.5 0 0 1-2.5-2.5v-10A2.5 2.5 0 0 1 5 5Zm3.8 3.6a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z"/>',
    "flashlight": f'<path {F} d="M7 2.5h10v3.2l-2.3 3.5V20a1.5 1.5 0 0 1-1.5 1.5h-2.4A1.5 1.5 0 0 1 9.3 20V9.2L7 5.7Z"/>',
    "share": f'<path {S} d="M12 15V3m-4 3.8L12 3l4 3.8M8 10H6.5A1.5 1.5 0 0 0 5 11.5v8A1.5 1.5 0 0 0 6.5 21h11a1.5 1.5 0 0 0 1.5-1.5v-8a1.5 1.5 0 0 0-1.5-1.5H16"/>',
    "tabs": f'<rect x="3" y="7.5" width="13.5" height="13.5" rx="2.8" {S}/><path {S} d="M7.5 7.5V5.8A2.8 2.8 0 0 1 10.3 3h7.9A2.8 2.8 0 0 1 21 5.8v7.9a2.8 2.8 0 0 1-2.8 2.8h-1.7"/>',
    "book": f'<path {S} d="M12 6.5c-2-1.7-5-2.2-8.5-2v14c3.5-.2 6.5.3 8.5 2 2-1.7 5-2.2 8.5-2v-14c-3.5-.2-6.5.3-8.5 2Zm0 0v14"/>',
    "home": f'<path {S} d="M3.5 11 12 3.5l8.5 7.5M5.5 9.5V20h4.8v-6h3.4v6h4.8V9.5"/>',
    "folder": f'<path {F} d="M2.5 6.5A2 2 0 0 1 4.5 4.5h4.6l2.2 2.4h8.2a2 2 0 0 1 2 2v8.6a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2Z"/>',
    "document": f'<path {S} d="M6 2.8h8l4.5 4.6V20a1.3 1.3 0 0 1-1.3 1.3H6A1.3 1.3 0 0 1 4.7 20V4A1.3 1.3 0 0 1 6 2.8Zm7.6.2v4.8h4.7M8.2 12.5h7.6M8.2 16.2h7.6"/>',
    "clock": f'<circle cx="12" cy="12" r="9" {S}/><path {S} d="M12 6.5V12l3.8 2.4"/>',
    "download": f'<path {S} d="M12 3.5V16m-5-4.8 5 4.8 5-4.8M4.5 20.5h15"/>',
    "star": f'<path {F} d="m12 2.6 2.8 6 6.6.8-4.9 4.5 1.3 6.5L12 17.2l-5.8 3.2 1.3-6.5-4.9-4.5 6.6-.8Z"/>',
    "star-outline": f'<path {S} stroke-width="1.8" d="m12 3.4 2.6 5.5 6 .8-4.4 4.1 1.1 6L12 16.9l-5.3 2.9 1.1-6-4.4-4.1 6-.8Z"/>',
    "trash": f'<path {S} d="M4 6.5h16M9.5 6.5V4h5v2.5M6 6.5l1 13.2A1.4 1.4 0 0 0 8.4 21h7.2a1.4 1.4 0 0 0 1.4-1.3l1-13.2M10 10.5v6.5m4-6.5v6.5"/>',
    "send": f'<path {F} d="M2.5 20.5 22 12 2.5 3.5l2.2 7L14 12l-9.3 1.5Z"/>',
    "compose": f'<path {S} d="M11 4.5H5.5A2 2 0 0 0 3.5 6.5v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V13"/><path {S} d="M9.5 14.5l.6-3.2L18.6 2.8a1.8 1.8 0 0 1 2.6 2.6L12.7 13.9Z"/>',
    "person": f'<circle cx="12" cy="8" r="4.2" {F}/><path {F} d="M3.8 21a8.2 8.2 0 0 1 16.4 0Z"/>',
    "globe": f'<circle cx="12" cy="12" r="9" {S} stroke-width="1.7"/><path {S} stroke-width="1.7" d="M3 12h18M12 3c-5 5-5 13 0 18m0-18c5 5 5 13 0 18"/>',
    "shield": f'<path {S} d="M12 2.8 4.5 5.6v6c0 4.6 3 8 7.5 9.6 4.5-1.6 7.5-5 7.5-9.6v-6Z"/>',
    "keyboard": f'<rect x="2" y="5.5" width="20" height="13" rx="2.5" {S} stroke-width="1.7"/><path {S} stroke-width="1.7" d="M6 9.5h.1M10 9.5h.1M14 9.5h.1M18 9.5h.1M6 12.5h.1M10 12.5h.1M14 12.5h.1M18 12.5h.1M8 15.5h8"/>',
    "accessibility": f'<circle cx="12" cy="4.8" r="2.1" {F}/><path {S} d="M4.5 8.5c5 1.6 10 1.6 15 0M12 9.8V14m0 0-3.4 7M12 14l3.4 7"/>',
    "cast": f'<path {S} d="M3 8.5V6.2A1.7 1.7 0 0 1 4.7 4.5h14.6A1.7 1.7 0 0 1 21 6.2v11.6a1.7 1.7 0 0 1-1.7 1.7H14"/><path {S} d="M3 12a7.5 7.5 0 0 1 7.5 7.5M3 15.8a3.7 3.7 0 0 1 3.7 3.7"/><circle cx="3.6" cy="19" r="1.2" {F}/>',
    "hotspot": f'<circle cx="12" cy="12" r="2.2" {F}/><path {S} d="{arc(12, 12, 5.5, 215, 505)}"/><path {S} d="{arc(12, 12, 9.2, 225, 495)}"/>',
    "leaf": f'<path {F} d="M20.5 3.5c.6 9-2.6 15-9.4 15.6-1.6.1-3-.3-4.2-1.1L4.6 20.8 3.2 19.4l2.5-2.6C3.3 12.3 7.4 4.3 20.5 3.500ZM8.3 15.6c3.5-1.3 6.5-4 8.3-7.8-3.2 2.7-6.1 4.8-8.3 7.800Z"/>',
    "display": f'<rect x="2.5" y="4" width="19" height="12.5" rx="2" {S}/><path {S} d="M8.5 20.500h7M12 16.500v4"/>',
    "headphones": f'<path {S} d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="13.5" width="4.5" height="7" rx="1.8" {F}/><rect x="16.5" y="13.5" width="4.5" height="7" rx="1.8" {F}/>',
    "link": f'<path {S} d="M10 14a4.5 4.5 0 0 0 6.4 0l3.1-3.100a4.5 4.5 0 0 0-6.4-6.400L12 5.600M14 10a4.5 4.5 0 0 0-6.4 0l-3.1 3.100a4.5 4.5 0 0 0 6.4 6.400L12 18.4"/>',
    "tag": f'<path {S} d="M3.5 4.500h8l9 9-7.5 7.5-9-9Z"/><circle cx="8" cy="9" r="1.5" {F}/>',
    "cloud": f'<path {F} d="M7 19.500a4.8 4.8 0 0 1-.7-9.5 6 6 0 0 1 11.6 1.200A4.2 4.2 0 0 1 17.5 19.500Z"/>',
    "desktop": f'<rect x="2.5" y="4" width="19" height="12.5" rx="2" {F}/><path {S} d="M8.5 20.500h7M12 16.500v4"/>',
    "image": f'<rect x="3" y="4.5" width="18" height="15" rx="2.5" {S}/><circle cx="8.5" cy="9.5" r="1.6" {F}/><path {S} d="m4 17 5-4.5 3.5 3 3-2.5 4.5 4"/>',
    "music": f'<path {S} d="M9 18V5.500l10-2V16"/><circle cx="6.5" cy="18" r="2.6" {F}/><circle cx="16.5" cy="16" r="2.6" {F}/>',
    "apps": f'<path {S} d="M12 3.5 4 8l8 4.500L20 8Zm-8 9 8 4.5 8-4.500M4 16.500l8 4.5 8-4.5" stroke-width="1.7"/>',
    "drive": f'<rect x="2.5" y="7" width="19" height="10" rx="2.5" {S}/><circle cx="17.5" cy="12" r="1.2" {F}/><path {S} d="M6 12h5"/>',
    "inbox": f'<path {S} d="M3.5 13.5 6 5h12l2.5 8.500v5a1.5 1.5 0 0 1-1.5 1.500H5a1.5 1.5 0 0 1-1.5-1.500Zm0 0h5a3.5 3.5 0 0 0 7 0h5"/>',
    "edit": f'<path {S} d="M4 20l1-4.500L16.5 4a2 2 0 0 1 3.5 3.500L8.5 19Z"/>',
    "split": f'<rect x="3" y="4.5" width="18" height="15" rx="2.5" {S}/><path {S} d="M12 4.500v15"/>',
    "sort": f'<path {S} d="M7 4v16m-3.5-3.500L7 20l3.5-3.500M17 20V4m-3.5 3.500L17 4l3.5 3.5"/>',
    "scissors": f'<circle cx="6" cy="6.5" r="2.7" {S}/><circle cx="6" cy="17.5" r="2.7" {S}/><path {S} d="M8.2 8.2 20 18M8.2 15.8 20 6"/>',
    "copy": f'<rect x="8" y="8" width="12.5" height="12.5" rx="2.2" {S}/><path {S} d="M5.5 16H5A1.8 1.8 0 0 1 3.5 14V5.500A2 2 0 0 1 5.5 3.500H14A1.8 1.8 0 0 1 16 5v.5"/>',
    "paste": f'<path {S} d="M9 4.500H6.500A1.5 1.5 0 0 0 5 6v13.500A1.5 1.5 0 0 0 6.5 21h11a1.5 1.5 0 0 0 1.5-1.500V6a1.5 1.5 0 0 0-1.5-1.500H15"/><rect x="9" y="2.8" width="6" height="3.6" rx="1.2" {S}/>',
    "rename": f'<rect x="2.5" y="7.5" width="19" height="9" rx="2" {S}/><path {S} d="M15 4.500v15M13 4.500h4M13 19.500h4M6 12h5"/>',
    "film": f'<rect x="3" y="4" width="18" height="16" rx="2.5" {S}/><path {S} d="M7.5 4v16M16.5 4v16M3 8.500h4.5M3 12h4.5M3 15.500h4.5M16.5 8.500H21M16.5 12H21M16.5 15.500H21"/>',
    "ubuntu": f'<circle cx="12" cy="12" r="6.1" {S} stroke-width="2.1"/><circle cx="2.9" cy="12" r="2.3" {F}/><circle cx="16.55" cy="4.12" r="2.3" {F}/><circle cx="16.55" cy="19.88" r="2.3" {F}/>',
    # Media transport and library glyphs for the music players.
    "play": f'<path {F} d="M7 4.500v15a1 1 0 0 0 1.500.900l12-7.500a1 1 0 0 0 0-1.800l-12-7.500A1 1 0 0 0 7 4.500Z"/>',
    "pause": f'<rect x="5.500" y="4" width="4.500" height="16" rx="1.200" {F}/><rect x="14" y="4" width="4.500" height="16" rx="1.200" {F}/>',
    "forward": f'<path {F} d="M2 6.200v11.600a.8.8 0 0 0 1.200.700L11.500 13.300v4.500a.8.8 0 0 0 1.200.700l9-5.800a.8.8 0 0 0 0-1.400l-9-5.800a.8.8 0 0 0-1.200.700v4.500L3.200 5.500A.8.8 0 0 0 2 6.200Z"/>',
    "backward": f'<g transform="matrix(-1 0 0 1 24 0)"><path {F} d="M2 6.200v11.600a.8.8 0 0 0 1.200.700L11.500 13.300v4.500a.8.8 0 0 0 1.200.700l9-5.800a.8.8 0 0 0 0-1.400l-9-5.800a.8.8 0 0 0-1.200.700v4.500L3.200 5.500A.8.8 0 0 0 2 6.200Z"/></g>',
    "skip-next": f'<path {F} d="M4.500 5.200v13.600a.8.8 0 0 0 1.200.700l10-6.800a.8.8 0 0 0 0-1.400l-10-6.800A.8.8 0 0 0 4.500 5.200Z"/><rect x="16.800" y="4.500" width="2.700" height="15" rx="1" {F}/>',
    "skip-previous": f'<g transform="matrix(-1 0 0 1 24 0)"><path {F} d="M4.500 5.200v13.600a.8.8 0 0 0 1.200.700l10-6.800a.8.8 0 0 0 0-1.400l-10-6.800A.8.8 0 0 0 4.500 5.200Z"/><rect x="16.800" y="4.500" width="2.700" height="15" rx="1" {F}/></g>',
    "shuffle": f'<path {S} d="M3 7h3.500c2 0 3.200 1 4.300 2.700l2.400 4.600C14.300 16 15.500 17 17.500 17H21M18 14l3 3-3 3M3 17h3.500c1.300 0 2.200-.4 3-1.200M13.500 8.200c.8-.8 1.700-1.200 3-1.200H21M18 4l3 3-3 3"/>',
    "repeat": f'<path {S} d="M4 11V9a3 3 0 0 1 3-3h13M17 3l3 3-3 3M20 13v2a3 3 0 0 1-3 3H4M7 21l-3-3 3-3"/>',
    "repeat-one": f'<path {S} d="M4 11V9a3 3 0 0 1 3-3h13M17 3l3 3-3 3M20 13v2a3 3 0 0 1-3 3H4M7 21l-3-3 3-3"/><path {S} stroke-width="1.800" d="M11 10.600 12.500 9.500v5.500"/>',
    "heart": f'<path {S} d="M12 20s-7.500-4.600-9-9.300C2 7.400 4 4.500 7.200 4.500c2 0 3.600 1.100 4.800 3 1.200-1.900 2.800-3 4.800-3 3.200 0 5.200 2.900 4.200 6.200C19.500 15.400 12 20 12 20Z"/>',
    "heart-fill": f'<path {F} d="M12 20.500s-8-4.800-9.600-9.700C1.300 7.200 3.600 4 7.100 4c2.100 0 3.800 1.100 4.900 2.900C13.100 5.100 14.800 4 16.900 4c3.500 0 5.800 3.200 4.700 6.800C20 15.700 12 20.500 12 20.500Z"/>',
    "queue": f'<path {S} d="M4 6h16M4 12h10M4 18h8"/><path {F} d="M15.500 14v8l6-4Z"/>',
    "radio": f'<circle cx="12" cy="12" r="2.200" {F}/><path {S} d="M8.100 15.900a5.500 5.500 0 0 1 0-7.800M15.900 8.100a5.500 5.500 0 0 1 0 7.800M5.200 18.800a9.600 9.600 0 0 1 0-13.600M18.800 5.200a9.600 9.600 0 0 1 0 13.600"/>',
    "library": f'<rect x="3" y="4" width="3.200" height="16" rx="1" {S}/><rect x="8.800" y="4" width="3.200" height="16" rx="1" {S}/><path {S} d="m14.500 5.300 3-.9 4 15.200-3 .9Z"/>',
    "compass": f'<circle cx="12" cy="12" r="9" {S}/><path {F} d="m16 8-2.300 5.700L8 16l2.300-5.700Z"/>',
    "thumb-up": f'<path {S} d="M7 10.500V20H4.500a1 1 0 0 1-1-1v-7.500a1 1 0 0 1 1-1Zm0 0 4-7a2 2 0 0 1 2.800 2.200L13 9.500h5.600a2 2 0 0 1 2 2.300l-1.200 6.500a2 2 0 0 1-2 1.700H7"/>',
    "thumb-up-fill": f'<path {F} d="M3 11.500A1.500 1.500 0 0 1 4.500 10H7v10.500H4.500A1.500 1.500 0 0 1 3 19Zm5.500-1.200 3.600-6.400a2.200 2.200 0 0 1 3.900 1.800l-.8 3.300h4.100a2.200 2.200 0 0 1 2.200 2.600l-1.200 6.800a2.200 2.200 0 0 1-2.200 1.800H8.500Z"/>',
    # Image-editor tools.
    "pencil": f'<path {S} d="M15.5 4.5l4 4L8 20H4v-4ZM13 7l4 4"/>',
    "brush": f'<path {S} d="M20.5 3.5 11 13"/><path {F} d="M9.8 12.2c1.4 0 2.6 1.2 2.4 2.7-.3 2.9-2.9 5.3-8.7 5.600 1.6-1.300 1.300-3.500 2.100-5.500.6-1.700 2.300-2.800 4.200-2.800Z"/>',
    "bucket": f'<path {S} d="M11 3.5 19.5 12l-7 7L4 10.5Zm-7 7h15.5"/><path {F} d="M20.5 15.5c1 1.500 1.500 2.500 1.500 3.300a1.500 1.500 0 0 1-3 0c0-.8.500-1.800 1.500-3.300Z"/>',
    "eraser": f'<path {S} d="M8.5 20H20M4.4 14.600 14.600 4.400a1.5 1.5 0 0 1 2.100 0l3 3a1.5 1.5 0 0 1 0 2.100L10 19.300a2.500 2.500 0 0 1-3.500 0L4.400 16.700a1.5 1.5 0 0 1 0-2.100ZM9.500 9.500l5 5"/>',
    "eyedropper": f'<path {S} d="m13.500 6.500 4 4M15.500 4.500a2.100 2.100 0 0 1 3 0l1 1a2.100 2.100 0 0 1 0 3l-2 2-4-4ZM13.500 8.500 5 17v2.500h2.500l8.500-8.500"/>',
    "zoom-in": f'<circle cx="10.5" cy="10.5" r="6.5" {S}/><path {S} d="m15.5 15.5 5.5 5.5M7.500 10.500h6M10.500 7.500v6"/>',
    "zoom-out": f'<circle cx="10.5" cy="10.5" r="6.5" {S}/><path {S} d="m15.5 15.5 5.5 5.5M7.500 10.500h6"/>',
    "text-tool": f'<path {S} d="M5 6.500V4h14v2.500M12 4v16M9 20h6"/>',
    "select-rect": f'<path {S} stroke-dasharray="2.600 2.400" d="M4 4h16v16H4Z"/>',
    "select-ellipse": f'<ellipse cx="12" cy="12" rx="9" ry="7" {S} stroke-dasharray="2.600 2.400"/>',
    "lasso": f'<path {S} d="M5.500 14.500C3 12.500 3.500 7.500 8.500 5.500c5-2 11 .5 11 4.500 0 4-6 5.500-10 4.500"/><path {S} d="M9.500 14.500c-1.500 0-2.500 1-2.500 2.200 0 1.300 1.500 2 2.500 1.300M8 18c-.5 1.500-1.500 2.500-3 2.500"/>',
    "wand": f'<path {S} d="M4 20 15 9m-2-2 4 4"/><path {F} d="m18 2 .8 2.200L21 5l-2.200.8L18 8l-.8-2.200L15 5l2.200-.8ZM8 3l.5 1.500L10 5l-1.500.5L8 7l-.5-1.500L6 5l1.500-.5ZM19.500 12l.5 1.500 1.500.5-1.500.5-.5 1.500-.5-1.500-1.500-.5 1.500-.5Z"/>',
    "crop": f'<path {S} d="M7 3v14h14M3 7h14v14"/>',
    "rotate-cw": f'<path {S} d="M19.500 12a7.500 7.500 0 1 1-2.200-5.300M19.500 4v4.500H15"/>',
    "rotate-ccw": f'<path {S} d="M4.500 12a7.500 7.500 0 1 0 2.200-5.300M4.500 4v4.500H9"/>',
    "flip-h": f'<path {S} d="M12 3v18"/><path {F} d="M9.500 6.500v11L3 17.500Z"/><path {S} d="M14.500 6.500v11l6.500 0Z"/>',
    "flip-v": f'<path {S} d="M3 12h18"/><path {F} d="M6.500 9.500h11L17.500 3Z"/><path {S} d="M6.500 14.500h11l0 6.500Z"/>',
    "resize": f'<rect x="3" y="11" width="10" height="10" rx="1.500" {S}/><path {S} d="M13.500 10.500 20.500 3.500M15 3.500h5.500V9"/>',
    "shapes": f'<rect x="3" y="11" width="9" height="9" rx="1" {S}/><circle cx="15.500" cy="8.500" r="5" {S}/>',
    "line-tool": f'<path {S} d="M4.500 19.500 19.500 4.500"/><circle cx="4.500" cy="19.500" r="1.800" {F}/><circle cx="19.500" cy="4.500" r="1.800" {F}/>',
    "layers": f'<path {S} d="M12 3.500 21 8.500l-9 5-9-5ZM3 12.500l9 5 9-5M3 16.500l9 5 9-5"/>',
    "undo": f'<path {S} d="M9 14.500 4 9.500l5-5M4 9.500h10a6 6 0 0 1 0 12h-3"/>',
    "redo": f'<path {S} d="m15 14.500 5-5-5-5M20 9.500H10a6 6 0 0 0 0 12h3"/>',
    "move": f'<path {S} d="M12 3v18M3 12h18M9.500 5.500 12 3l2.500 2.500M9.500 18.500 12 21l2.500-2.500M5.500 9.500 3 12l2.500 2.500M18.500 9.500 21 12l-2.500 2.500"/>',
    "hand": f'<path {S} d="M8 13V5.500a1.500 1.500 0 0 1 3 0V11m0-6.500a1.500 1.500 0 0 1 3 0V11m0-5a1.500 1.500 0 0 1 3 0v5.500m0-3a1.500 1.500 0 0 1 3 0V15a6.500 6.500 0 0 1-6.500 6.500h-1c-2 0-3.500-1-4.700-2.600L4 14.500a1.500 1.500 0 0 1 2.300-1.900L8 14.500"/>',
    "sliders": f'<path {S} d="M4 7h9m4 0h3M4 17h3m4 0h9"/><circle cx="15" cy="7" r="2.200" {S}/><circle cx="9" cy="17" r="2.200" {S}/>',
    "dial": f'<circle cx="12" cy="12" r="8.500" {S}/><path {S} d="M12 12V6.500"/><circle cx="12" cy="12" r="1.600" {F}/>',
    "filters": f'<circle cx="12" cy="8" r="5" {S}/><circle cx="8.500" cy="14.500" r="5" {S}/><circle cx="15.500" cy="14.500" r="5" {S}/>',
    "marker": f'<path {S} d="M14.500 4.500l5 5-8 8H7.500l-1 1H3.500l2-2V13Zm-7 7 5 5"/>',
    "highlighter": f'<path {S} d="M15 3.500 20.500 9 13 16.500 7.500 11ZM7.500 11 5 13.500v2.500l2.500 2.500H10l2.500-2.500"/><path {F} d="M3 21h9v-1.500H3Z"/>',
    "ruler": f'<path {S} d="M3.500 16.500 16.500 3.500l4 4-13 13ZM7 13l2 2m1-5 2 2m1-5 2 2"/>',
    "palette": f'<path {S} d="M12 3.500a8.500 8.500 0 1 0 0 17c1.200 0 1.800-.8 1.500-1.800-.4-1.200.3-2.200 1.600-2.200H17a3.500 3.500 0 0 0 3.500-3.500c0-5.200-3.800-9.500-8.500-9.500Z"/><circle cx="7.500" cy="11" r="1.300" {F}/><circle cx="10" cy="7" r="1.300" {F}/><circle cx="14.500" cy="7.500" r="1.300" {F}/>',
    "eye": f'<path {S} d="M2.500 12S6 5.500 12 5.500 21.500 12 21.500 12 18 18.500 12 18.500 2.500 12 2.500 12Z"/><circle cx="12" cy="12" r="3" {S}/>',
    "eye-off": f'<path {S} d="M4 4l16 16M9.900 5.800A9 9 0 0 1 12 5.500c6 0 9.500 6.500 9.500 6.500a17 17 0 0 1-2.600 3.400M6.500 7.600C4 9.300 2.500 12 2.500 12S6 18.500 12 18.500c1.600 0 3-.4 4.300-1.100"/>',
    "magic": f'<path {F} d="M11 2.500l1.800 5 5 1.800-5 1.800-1.800 5-1.800-5-5-1.800 5-1.800ZM18.500 14l.9 2.400 2.400.9-2.400.9-.9 2.400-.9-2.400-2.400-.9 2.400-.9Z"/>',
    "minus": f'<path {S} d="M5 12h14"/>',
    "contrast": f'<circle cx="12" cy="12" r="8.500" {S}/><path {F} d="M12 3.500a8.500 8.500 0 0 1 0 17Z"/>',
    "drop": f'<path {S} d="M12 3.500s6 6.500 6 10.500a6 6 0 0 1-12 0c0-4 6-10.500 6-10.500Z"/>',
    "thermometer": f'<path {S} d="M10 13.500V5a2 2 0 0 1 4 0v8.500a4 4 0 1 1-4 0Z"/><circle cx="12" cy="17" r="1.800" {F}/>',
    "stamp": f'<path {S} d="M5 21h14M6 17.500h12v-3H6ZM9.500 14.500 10 10a3 3 0 1 1 4 0l.5 4.500"/>',
}


def dedupe(tag):
    """A later explicit stroke-width overrides the shared stroke style's default."""
    text = tag.group(0)
    if text.count("stroke-width=") > 1:
        text = re.sub(r'stroke-width="[^"]*" ?', "", text, count=1)
    return text


def main():
    OUT.mkdir(exist_ok=True)
    names = sorted(SYMBOLS)
    for name in names:
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 24 24">{re.sub(r"<[^>]+>", dedupe, SYMBOLS[name])}</svg>'
        (OUT / f"{name}.svg").write_text(svg)
        png = cairosvg.svg2png(bytestring=svg.encode(), output_width=96, output_height=96)
        rgba = Image.open(io.BytesIO(png)).convert("RGBA")
        alpha = rgba.getchannel("A")
        Image.merge("LA", (Image.new("L", alpha.size, 255), alpha)).save(OUT / f"{name}.png", optimize=True)
    with open(HERE.parent / "src" / "symbols.rs", "w") as out:
        out.write("// Generated by assets/generate-symbols.py; do not edit.\n")
        out.write("pub const SYMBOLS: &[(&str, &[u8])] = &[\n")
        for name in names:
            out.write(f'    ("symbol/{name}", include_bytes!("../assets/symbols/{name}.png")),\n')
        out.write("];\n")
    print(len(names), "symbols", sum((OUT / f"{n}.png").stat().st_size for n in names), "bytes")


if __name__ == "__main__":
    main()
