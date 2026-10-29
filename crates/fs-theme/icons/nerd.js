.pragma library

// Hand-mapped Nerd Font codepoints for the `nerd` icon set (theme.icons),
// the same Material Design Icons range shell/Menu/providers.js already
// draws bar/menu glyphs from (verified against nerd-fonts' own
// glyphnames.json, the source that patches JetBrainsMono Nerd Font). Only
// the names this milestone's surfaces need; M42+ tasks append more.
//
// "plug-zap" has no MDI glyph for a charging/live plug (checked every
// "plug"/"power_plug"/"ev_plug_*" entry in glyphnames.json): mapped to
// circle-help rather than guessing a wrong pairing.
var ICONS = {
    "wifi": "\u{F05A9}",
    "wifi-off": "\u{F05AA}",
    "bluetooth": "\u{F00AF}",
    "bluetooth-connected": "\u{F00B1}",
    "bluetooth-off": "\u{F00B2}",
    "volume": "\u{F0580}",
    "volume-1": "\u{F057F}",
    "volume-2": "\u{F057E}",
    "volume-x": "\u{F075F}",
    "mic": "\u{F036C}",
    "mic-off": "\u{F036D}",
    // MDI's own md-battery is the full one, so "battery" and "battery-full"
    // share it; lucide draws them apart (empty outline vs full).
    "battery": "\u{F0079}",
    "battery-charging": "\u{F0085}",
    "battery-full": "\u{F0079}",
    "battery-low": "\u{F007B}",
    "battery-medium": "\u{F007E}",
    "battery-warning": "\u{F0083}",
    "sun": "\u{F0599}",
    "moon": "\u{F0594}",
    // md-weather_night is already "moon"; the bare crescent keeps them apart.
    "moon-star": "\u{F0F65}",
    "bell": "\u{F009A}",
    "bell-off": "\u{F009B}",
    "clock": "\u{F0150}",
    "calendar": "\u{F00EE}",
    "search": "\u{F0349}",
    "command": "\u{F0633}",
    "x": "\u{F0156}",
    "check": "\u{F012C}",
    "lock": "\u{F033E}",
    "lock-open": "\u{F033F}",
    "refresh-cw": "\u{F0453}",
    "settings": "\u{F0493}",
    "power": "\u{F0425}",
    "monitor": "\u{F0379}",
    "monitor-off": "\u{F0D90}",
    "cpu": "\u{F0EE0}",
    "memory-stick": "\u{F035B}",
    "hard-drive": "\u{F02CA}",
    "thermometer": "\u{F050F}",
    "activity": "\u{F0430}",
    "gauge": "\u{F029A}",
    "download": "\u{F01DA}",
    "upload": "\u{F0552}",
    "arrow-up": "\u{F005D}",
    "arrow-down": "\u{F0045}",
    "arrow-left": "\u{F004D}",
    "arrow-right": "\u{F0054}",
    "chevron-up": "\u{F0143}",
    "chevron-down": "\u{F0140}",
    "chevron-left": "\u{F0141}",
    "chevron-right": "\u{F0142}",
    "circle-help": "\u{F02D7}",
    "circle-alert": "\u{F0028}",
    "triangle-alert": "\u{F0026}",
    "info": "\u{F02FC}",
    "play": "\u{F040A}",
    "pause": "\u{F03E4}",
    "skip-back": "\u{F04AE}",
    "skip-forward": "\u{F04AD}",
    "shuffle": "\u{F049D}",
    "repeat": "\u{F0456}",
    "repeat-1": "\u{F0458}",
    "music": "\u{F075A}",
    "radio": "\u{F0439}",           // md-radio
    "image": "\u{F02E9}",
    "camera": "\u{F0100}",
    "video": "\u{F0567}",
    "clipboard": "\u{F0147}",
    "keyboard": "\u{F030C}",
    "terminal": "\u{F018D}",
    "git-branch": "\u{F062C}",
    "package": "\u{F03D3}",
    // md-package_up is MDI's own "there is an update" box; lucide
    // draws the same idea as a package with a plus.
    "package-plus": "\u{F03D5}",
    "cloud": "\u{F015F}",
    "cloudy": "\u{F0590}",
    "cloud-fog": "\u{F0591}",
    // MDI draws no drizzle of its own, so the lightest rain it has stands in
    // and shares md-weather_rainy with "cloud-rain"; lucide draws them apart.
    "cloud-drizzle": "\u{F0597}",
    "cloud-rain": "\u{F0597}",
    "cloud-rain-wind": "\u{F0596}",
    "cloud-hail": "\u{F0592}",
    "cloud-snow": "\u{F0598}",
    "cloud-lightning": "\u{F0593}",
    "cloud-sun": "\u{F0595}",
    "cloud-moon": "\u{F0F31}",
    "zap": "\u{F0241}",
    "plus": "\u{F0415}",
    "minus": "\u{F0374}",
    "trash": "\u{F0A79}",
    "pencil": "\u{F03EB}",
    "external-link": "\u{F03CC}",
    "ellipsis": "\u{F01D8}",
    "menu": "\u{F035C}",
    "grid-2x2": "\u{F0570}",
    "list": "\u{F0279}",
    "star": "\u{F04CE}",
    "heart": "\u{F02D1}",
    "globe": "\u{F01E7}",
    "network": "\u{F0317}",
    "map-pin": "\u{F034E}",
    "user": "\u{F0004}",
    "users": "\u{F0849}",
    "home": "\u{F02DC}",
    "folder": "\u{F024B}",
    "file": "\u{F0214}",
    "save": "\u{F0193}",
    "copy": "\u{F018F}",
    "share-2": "\u{F0497}",
    "send": "\u{F048A}",
    "mail": "\u{F01EE}",
    "message-square": "\u{F0369}",
    "phone": "\u{F03F2}",
    "headphones": "\u{F02CB}",
    "gamepad-2": "\u{F0297}",
    "printer": "\u{F042A}",
    "usb": "\u{F0553}",
    "plug": "\u{F06A5}",
    "plug-zap": "\u{F02D7}",

    // M43: the launcher's route icons (shell/Menu/icons.js). Every
    // codepoint below is the one the same route already carried as a raw
    // glyph in default-menu.jsonc or providers.js, so the nerd set draws
    // exactly what it drew before the names went in.
    "layout-grid": "\u{F003B}",      // md-apps
    "calculator": "\u{F00EC}",       // md-calculator
    "smile": "\u{F0C68}",            // md-emoticon
    "snowflake": "\u{F313}",         // linux-nixos
    "inbox": "\u{F1294}",            // md-tray
    "gpu": "\u{F08AE}",              // md-expansion_card
    "log-out": "\u{F0343}",          // md-logout
    "alarm-clock": "\u{F088C}",      // md-reminder
    "timer": "\u{F051B}",            // md-timer_outline
    "history": "\u{F02DA}",          // md-history
    "toggle-left": "\u{F0521}",      // md-toggle_switch
    "lightbulb": "\u{F1A4C}",        // md-lightbulb_night
    "coffee": "\u{F0176}",           // md-coffee
    "scan-text": "\u{F113A}",        // md-ocr
    "pipette": "\u{F020B}",          // md-eyedropper_variant
    "square": "\u{F04DB}",           // md-stop
    "film": "\u{F0D78}",             // md-file_gif_box
    "crop": "\u{F0489}",             // md-selection
    "puzzle": "\u{F0431}",           // md-puzzle
    "check-check": "\u{F012D}",      // md-check_all
    "palette": "\u{F0301}",          // md-invert_colors
    "arrow-left-right": "\u{F04E1}", // md-swap_horizontal
    "laptop": "\u{F0322}",           // md-laptop
    "git-fork": "\u{F00FB}",         // md-call_split
    "bot": "\u{F16A3}",              // md-robot_excited
    "fingerprint": "\u{F0237}",
    "circle-dot": "\u{F0EC2}",       // md-record_circle
    "app-window": "\u{F05AF}",       // md-window_maximize

    // M49: the AirPods listening modes. MDI has no dedicated
    // transparency glyph, so the plain hearing ear stands for it and
    // the struck-through one for noise cancellation, which is the
    // pairing lucide draws with ear/ear-off.
    "circle-off": "\u{F073A}",       // md-cancel
    "ear": "\u{F07C5}",              // md-ear_hearing
    "ear-off": "\u{F0A45}",          // md-ear_hearing_off
    "audio-waveform": "\u{F147D}",   // md-waveform

    // M74: the Spaces cell's agent badges.
    "loader-circle": "\u{F0772}",    // md-loading
    "circle-check": "\u{F05E0}",     // md-check_circle

    // M75: the iPhone mirror's per-app marks (Iphone/model.js appIcon).
    "smartphone": "\u{F011C}",       // md-cellphone
    "message-circle": "\u{F0361}",   // md-message
    "phone-incoming": "\u{F03F7}",   // md-phone_incoming
    "phone-missed": "\u{F03FA}",     // md-phone_missed
    "voicemail": "\u{F057D}",        // md-voicemail
    "hash": "\u{F0423}",             // md-pound
    "heart-pulse": "\u{F05F6}",      // md-heart_pulse
    "map": "\u{F034D}",              // md-map
    "newspaper": "\u{F0395}",        // md-newspaper
    "wallet": "\u{F0584}",           // md-wallet
    "list-todo": "\u{F0756}"         // md-format_list_checks
};

var FAMILY = "";
