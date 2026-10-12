# Shared helpers for the fffFinder build scripts.

const SCRIPTS_DIR = path self | path dirname

# Repository root (the parent of scripts/).
export def repo-root []: nothing -> path {
    $SCRIPTS_DIR | path dirname
}

# Executable suffix for this platform.
export def exe-suffix []: nothing -> string {
    if $nu.os-info.name == 'windows' { '.exe' } else { '' }
}

# Build the workspace; returns the target directory holding the binaries.
export def build [--debug]: nothing -> path {
    let root = repo-root
    let args = if $debug { [build --workspace] } else { [build --release --workspace] }
    cd $root
    ^cargo ...$args
    $root | path join target (if $debug { 'debug' } else { 'release' })
}

# Version from the workspace manifest.
export def version []: nothing -> string {
    open (repo-root | path join Cargo.toml) | get workspace.package.version
}
