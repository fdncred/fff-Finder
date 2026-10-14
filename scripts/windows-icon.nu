#!/usr/bin/env nu
# Render assets/app-icon.svg into the icon the Windows build embeds,
# assets/windows/fffinder.ico. Runs on macOS (Quick Look and sips); run it
# after changing the SVG and commit the result.

use common.nu *

const SIZES = [16 20 24 32 40 48 64 96 128 256]

def main [] {
    let root = repo-root
    let work = mktemp --directory
    let png = render-svg ($root | path join assets app-icon.svg) $work
    let images = $SIZES | each {|size|
        let out = $work | path join $'($size).png'
        ^sips -z $size $size $png --out $out | ignore
        {size: $size, data: (open --raw $out)}
    }
    let ico = $root | path join assets windows fffinder.ico
    pack-ico $images | save --force $ico
    rm --recursive --force $work
    $ico
}

# An ICO file holding PNG images, which Windows Vista and later read.
def pack-ico [images: list<record<size: int, data: binary>>]: nothing -> binary {
    let count = $images | length
    mut offset = 6 + 16 * $count
    mut entries = []
    for image in $images {
        let length = $image.data | bytes length
        # Width and height are one byte each, with 0 meaning 256.
        let side = if $image.size >= 256 { 0 } else { $image.size }
        $entries = $entries | append (bytes build
            (le $side 1) (le $side 1) 0x[00 00]
            (le 1 2) (le 32 2) (le $length 4) (le $offset 4))
        $offset = $offset + $length
    }
    bytes build 0x[00 00] (le 1 2) (le $count 2) ...$entries ...($images | get data)
}

# `value` as `size` little-endian bytes.
def le [value: int, size: int]: nothing -> binary {
    $value | into binary --endian little | bytes at 0..<$size
}
