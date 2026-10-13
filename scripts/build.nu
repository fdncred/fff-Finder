#!/usr/bin/env nu
# Build fff-Finder, and optionally test and lint it.

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
    let binary = $target | path join $'fff-Finder(exe-suffix)'
    {binary: $binary, size: (ls $binary | get 0.size)}
}
