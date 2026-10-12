#!/usr/bin/env nu
# Build, install, and register fffFinder and its Nushell plugin.
#
#   nu scripts/install.nu                 # release build into ~/.cargo/bin
#   nu scripts/install.nu --dest ~/bin    # somewhere else on PATH
#
# Afterwards run `plugin use fff` (or restart nu), and optionally add
# `use <repo>/nu/fff.nu *` and `fff keybindings` to config.nu.

use common.nu *

def main [
    --dest: path        # Install directory (default: ~/.cargo/bin)
    --debug             # Install debug builds
    --no-register       # Skip `plugin add`
] {
    let target = build --debug=$debug
    let dest = $dest | default ($nu.home-dir | path join .cargo bin) | path expand
    mkdir $dest
    let suffix = exe-suffix
    let installed = [fffFinder nu_plugin_fff] | each {|bin|
        let file = $'($bin)($suffix)'
        cp --force ($target | path join $file) $dest
        $dest | path join $file
    }
    if not $no_register {
        plugin add ($dest | path join $'nu_plugin_fff($suffix)')
        print $"(ansi green)Registered nu_plugin_fff.(ansi reset) Run `plugin use fff` or restart nu."
    }
    let module = repo-root | path join nu fff.nu
    print $"Helpers \(fff insert, fff cd, key bindings\): add `use ($module) *` and `fff keybindings` to config.nu."
    $installed
}
