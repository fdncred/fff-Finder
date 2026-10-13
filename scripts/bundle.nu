#!/usr/bin/env nu
# Package fff-Finder for the current platform into dist/.
#
#   macOS:   dist/fff-Finder.app (fff.nu inside Contents/Resources)
#   Linux:   dist/fff-Finder-<version>-linux-<arch>/ with a .desktop file
#            (--install copies it into ~/.local)
#   Windows: dist/fff-Finder-<version>-windows-<arch>.zip

use common.nu *

def main [
    --install   # Linux: install the desktop entry and icon for this user
    --skip-build # Package existing release binaries
] {
    let root = repo-root
    let target = if $skip_build { target-dir | path join release } else { build }
    let dist = $root | path join dist
    mkdir $dist
    match $nu.os-info.name {
        'macos' => (bundle-macos $root $target $dist)
        'windows' => (bundle-windows $root $target $dist)
        _ => (bundle-linux $root $target $dist --install=$install)
    }
}

def bundle-macos [root: path, target: path, dist: path]: nothing -> path {
    let app = $dist | path join fff-Finder.app
    rm --recursive --force $app
    let contents = $app | path join Contents
    mkdir ($contents | path join MacOS) ($contents | path join Resources)
    cp ($target | path join fff-Finder) ($contents | path join MacOS)
    cp ($root | path join nu fff.nu) ($contents | path join Resources)
    make-icns ($root | path join assets app-icon.svg) ($contents | path join Resources fff-Finder.icns)
    info-plist (version) | save --force ($contents | path join Info.plist)
    # Ad-hoc signature so Gatekeeper and Full Disk Access can track the app.
    ^codesign --force --deep --sign - $app
    print $"Built ($app). Drag it to /Applications; grant Full Disk Access to search protected folders."
    $app
}

# Render the SVG with Quick Look, then build an iconset with sips and iconutil.
def make-icns [svg: path, out: path] {
    let work = mktemp --directory
    let png = render-svg $svg $work
    let iconset = $work | path join fff-Finder.iconset
    mkdir $iconset
    for size in [16 32 128 256 512] {
        ^sips -z $size $size $png --out ($iconset | path join $'icon_($size)x($size).png') | ignore
        let double = $size * 2
        ^sips -z $double $double $png --out ($iconset | path join $'icon_($size)x($size)@2x.png') | ignore
    }
    ^iconutil --convert icns --output $out $iconset
    rm --recursive --force $work
}

def info-plist [version: string]: nothing -> string {
    $'<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>fff-Finder</string>
  <key>CFBundleDisplayName</key><string>fff-Finder</string>
  <key>CFBundleIdentifier</key><string>com.nushell.fff-Finder</string>
  <key>CFBundleExecutable</key><string>fff-Finder</string>
  <key>CFBundleIconFile</key><string>fff-Finder</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>($version)</string>
  <key>CFBundleVersion</key><string>($version)</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>fff-Finder asks your terminal, such as Ghostty, to open a window running Nushell in a folder.</string>
</dict>
</plist>
'
}

def bundle-linux [root: path, target: path, dist: path, --install]: nothing -> path {
    let name = $'fff-Finder-(version)-linux-($nu.os-info.arch)'
    let out = $dist | path join $name
    rm --recursive --force $out
    mkdir $out
    cp ($target | path join fff-Finder) ($root | path join nu fff.nu) $out
    cp ($root | path join assets app-icon.svg) ($out | path join fff-Finder.svg)
    let desktop = [
        '[Desktop Entry]'
        'Type=Application'
        'Name=fff-Finder'
        'Comment=Fast fuzzy file search'
        'Exec=fff-Finder %f'
        'Icon=fff-Finder'
        'Terminal=false'
        'Categories=Utility;FileTools;FileManager;'
        'Keywords=find;search;files;fuzzy;'
        'StartupWMClass=fff-Finder'
    ] | str join (char nl)
    $desktop | save --force ($out | path join fff-Finder.desktop)
    if $install {
        let share = $nu.home-dir | path join .local share
        let bin = $nu.home-dir | path join .local bin
        mkdir ($share | path join applications) ($share | path join icons hicolor scalable apps) $bin
        cp ($out | path join fff-Finder) $bin
        cp ($out | path join fff-Finder.svg) ($share | path join icons hicolor scalable apps)
        cp ($out | path join fff-Finder.desktop) ($share | path join applications)
        print $"Installed to ($bin) and the desktop menu."
    }
    $out
}

def bundle-windows [root: path, target: path, dist: path]: nothing -> path {
    let name = $'fff-Finder-(version)-windows-($nu.os-info.arch)'
    let out = $dist | path join $name
    rm --recursive --force $out
    mkdir $out
    cp ($target | path join fff-Finder.exe) ($root | path join nu fff.nu) $out
    cp ($root | path join README.md) $out
    let zip = $dist | path join $'($name).zip'
    rm --force $zip
    cd $dist
    ^tar -a -c -f $zip $name
    $zip
}
