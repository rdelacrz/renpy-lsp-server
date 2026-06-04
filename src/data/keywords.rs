//! Static keyword and property lists for Ren'Py completion.
//!
//! These lists power contextual auto-completion for Ren'Py statements, screen-language
//! displayables, ATL animation properties, and style properties.

/// Ren'Py statement/flow keywords for completion.
pub fn renpy_keywords() -> &'static [&'static str] {
    &[
        "define", "default", "image", "init", "jump", "call", "return", "label",
        "menu", "pass", "pause", "python", "scene", "screen", "show", "hide",
        "with", "transform", "style", "translate", "if", "elif", "else", "while",
        "for", "camera", "play", "stop", "queue", "voice", "window", "nvl",
        "use", "has", "add", "timer", "key", "on", "at", "as", "behind",
        "onlayer", "zorder", "expression", "contains", "parallel", "block",
        "choice", "repeat", "function", "event", "animation",
    ]
}

/// Screen language displayable names.
pub fn screen_displayables() -> &'static [&'static str] {
    &[
        "null", "text", "hbox", "vbox", "fixed", "grid", "frame", "window",
        "side", "button", "textbutton", "imagebutton", "bar", "vbar", "hotbar",
        "viewport", "vpgrid", "imagemap", "hotspot", "drag", "draggroup",
        "timer", "input", "key", "mousearea", "add", "transform", "use", "has",
        "showif", "nearrect", "areapicker",
    ]
}

/// ATL property names.
pub fn atl_properties() -> &'static [&'static str] {
    &[
        "alpha", "align", "pos", "anchor", "xpos", "ypos", "xanchor", "yanchor",
        "xalign", "yalign", "rotate", "zoom", "xzoom", "yzoom", "crop", "size",
        "xsize", "ysize", "matrixcolor", "blur", "delay", "fps", "xoffset",
        "yoffset", "offset", "rotate_pad", "transform_anchor", "subpixel",
        "xtile", "ytile", "xpan", "ypan", "mesh", "shader",
    ]
}

/// Style property names.
pub fn style_properties() -> &'static [&'static str] {
    &[
        "background", "foreground", "color", "font", "size", "bold", "italic",
        "underline", "strikethrough", "kerning", "drop_shadow", "drop_shadow_color",
        "outlines", "text_align", "layout", "language", "antialias", "min_width",
        "xminimum", "yminimum", "xmaximum", "ymaximum", "xfill", "yfill",
        "spacing", "first_spacing", "xpadding", "ypadding", "left_padding",
        "right_padding", "top_padding", "bottom_padding", "left_margin",
        "right_margin", "top_margin", "bottom_margin", "xmargin", "ymargin",
    ]
}
