.pragma library

// Real distro logos, from the font that carries them (owner, 2026-08-26:
// "make it the actual logo, not a fake snowflake one. Find a library for the
// most mayor distros"). The library is font-logos, which Nerd Fonts embeds
// as its Linux range; the release tarball ships Symbols Nerd Font beside
// lucide.ttf on FS_RS_FONT_DIRS (dev/tarball.sh), so the glyphs resolve on
// any install without the user having an icon theme with distributor logos
// in it.
//
// Deliberately NOT part of Theme/icons.js's set switching. Every other icon
// in the shell follows `theme.icons`, but a distro logo is a mark, not an
// icon: under `lucide` the name "snowflake" resolved to Lucide's generic
// weather snowflake, which is how the launcher ended up wearing a fake
// NixOS logo. This table always draws the real one.
//
// Codepoints extracted from SymbolsNerdFont-Regular.ttf 3.5.0 rather than
// transcribed. Keys are os-release ID values, glyph names are the font's own.

var FAMILY = "Symbols Nerd Font";

// The generic Tux, for a Linux the table does not name.
var FALLBACK = "\u{F31A}";

var LOGOS = {
    "almalinux": "\u{F31D}",             // linux-almalinux
    "alpine": "\u{F300}",                // linux-alpine
    "aosc": "\u{F301}",                  // linux-aosc
    "arch": "\u{F303}",                  // linux-archlinux
    "archarm": "\u{F303}",               // linux-archlinux
    "archcraft": "\u{F345}",             // linux-archcraft
    "archlabs": "\u{F31E}",              // linux-archlabs
    "arcolinux": "\u{F346}",             // linux-arcolinux
    "artix": "\u{F31F}",                 // linux-artix
    "biglinux": "\u{F347}",              // linux-biglinux
    "cachyos": "\u{F385}",               // linux-cachyos
    "centos": "\u{F304}",                // linux-centos
    "coreos": "\u{F305}",                // linux-coreos
    "crystal": "\u{F348}",               // linux-crystal
    "debian": "\u{F306}",                // linux-debian
    "deepin": "\u{F321}",                // linux-deepin
    "devuan": "\u{F307}",                // linux-devuan
    "elementary": "\u{F309}",            // linux-elementary
    "endeavouros": "\u{F322}",           // linux-endeavour
    "fedora": "\u{F30A}",                // linux-fedora
    "fedora-coreos": "\u{F305}",         // linux-coreos
    "freebsd": "\u{F30C}",               // linux-freebsd
    "funtoo": "\u{F30D}",                // linux-gentoo
    "garuda": "\u{F337}",                // linux-garuda
    "gentoo": "\u{F30D}",                // linux-gentoo
    "guix": "\u{F325}",                  // linux-gnu_guix
    "hyperbola": "\u{F33A}",             // linux-hyperbola
    "illumos": "\u{F326}",               // linux-illumos
    "kali": "\u{F327}",                  // linux-kali_linux
    "kubuntu": "\u{F333}",               // linux-kubuntu
    "linux": "\u{F31A}",                 // linux-tux
    "linuxmint": "\u{F30E}",             // linux-linuxmint
    "locos": "\u{F349}",                 // linux-locos
    "lxle": "\u{F33E}",                  // linux-lxle
    "mageia": "\u{F310}",                // linux-mageia
    "mandriva": "\u{F311}",              // linux-mandriva
    "manjaro": "\u{F312}",               // linux-manjaro
    "manjaro-arm": "\u{F312}",           // linux-manjaro
    "mx": "\u{F33F}",                    // linux-mxlinux
    "neon": "\u{F331}",                  // linux-kde_neon
    "nixos": "\u{F313}",                 // linux-nixos
    "nobara": "\u{F380}",                // linux-nobara
    "openbsd": "\u{F328}",               // linux-openbsd
    "opensuse": "\u{F314}",              // linux-opensuse
    "opensuse-leap": "\u{F37E}",         // linux-leap
    "opensuse-tumbleweed": "\u{F314}",   // linux-opensuse, the gecko
    "openwrt": "\u{F382}",               // linux-openwrt
    "parabola": "\u{F340}",              // linux-parabola
    "parrot": "\u{F329}",                // linux-parrot
    "pop": "\u{F32A}",                   // linux-pop_os
    "postmarketos": "\u{F374}",          // linux-postmarketos
    "puppy": "\u{F341}",                 // linux-puppy
    "qubes": "\u{F342}",                 // linux-qubesos
    "raspbian": "\u{F315}",              // linux-raspberry_pi
    "rhel": "\u{F316}",                  // linux-redhat
    "rocky": "\u{F32B}",                 // linux-rocky_linux
    "sabayon": "\u{F317}",               // linux-sabayon
    "slackware": "\u{F318}",             // linux-slackware
    "sles": "\u{F314}",                  // linux-opensuse
    "solus": "\u{F32D}",                 // linux-solus
    "tails": "\u{F343}",                 // linux-tails
    "trisquel": "\u{F344}",              // linux-trisquel
    "ubuntu": "\u{F31B}",                // linux-ubuntu
    "void": "\u{F32E}",                  // linux-void
    "xerolinux": "\u{F34A}",             // linux-xerolinux
    "zorin": "\u{F32F}"                 // linux-zorin
};

function glyph(id) {
    var key = String(id || "").trim().toLowerCase();
    return LOGOS[key] || "";
}

// Any Linux at all is better than the shell's own mark when the caller asked
// for the distro logo and the table has no entry for this one.
function glyphOrTux(id) {
    return glyph(id) || FALLBACK;
}
