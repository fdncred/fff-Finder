#!/usr/bin/env nu
# Build fffFinder and nu_plugin_fff, and run the test suite.

use common.nu *

def main [
    --debug   # Build without optimizations
    --test    # Also run `cargo test --workspace`
    --lint    # Also run clippy with warnings as errors
] {
    let target = build --debug=$debug
    cd (repo-root)
    if $lint { ^cargo clippy --workspace --all-targets -- -D warnings }
    if $test { ^cargo test --workspace }
    let suffix = exe-suffix
    [fffFinder nu_plugin_fff]
    | each {|bin| $target | path join $'($bin)($suffix)' }
    | wrap binary
    | insert size {|row| ls $row.binary | get 0.size }
}
