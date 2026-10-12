# fffFinder helpers for Nushell.
#
# Needs the `nu_plugin_fff` plugin, which provides `fff`, `fff grep`,
# `fff pick`, `fff gui`, and `fff status`:
#
#     plugin add ~/.cargo/bin/nu_plugin_fff
#     plugin use fff
#
# Then load these helpers, for example from config.nu:
#
#     use ~/src/fffFinder/nu/fff.nu *
#     fff keybindings      # Ctrl+T inserts picked paths, Alt+C picks a folder to cd into

# Pick files in the fffFinder window and insert them at the cursor as NUON.
export def "fff insert" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick --multiple ...$query
    if ($picked | is-empty) { return }
    let text = $picked | each { to nuon } | str join ' '
    commandline edit --insert $text
}

# Pick a folder (or a file's folder) in fffFinder and cd into it.
export def --env "fff cd" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick ...$query
    if $picked == null { return }
    let dir = if ($picked | path type) == 'dir' { $picked } else { $picked | path dirname }
    cd $dir
}

# Pick a file in fffFinder and open it in $env.EDITOR (or $env.VISUAL).
export def "fff edit" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick ...$query
    if $picked == null { return }
    let editor = $env.VISUAL? | default $env.EDITOR? | default (if $nu.os-info.name == 'windows' { 'notepad' } else { 'vi' })
    run-external $editor $picked
}

# Fuzzy-find files and open the best match as structured data.
export def "fff open" [
    ...query: string  # Search words, as for `fff`
]: nothing -> any {
    let hit = fff --kind document --limit 1 ...$query | get 0?.path
    if $hit == null {
        error make {msg: $"no document matches '($query | str join ' ')'"}
    }
    open $hit
}

# Add key bindings: Ctrl+T inserts picked paths, Alt+C picks a folder to cd into.
export def --env "fff keybindings" [
    --insert-key: string = 't'  # Key used with Ctrl to insert picked paths
    --cd-key: string = 'c'      # Key used with Alt to cd into a picked folder
]: nothing -> nothing {
    let modes = [emacs vi_normal vi_insert]
    $env.config.keybindings = $env.config.keybindings
        | where name not-in [fff_insert fff_cd]
        | append [
            {
                name: fff_insert
                modifier: control
                keycode: $"char_($insert_key)"
                mode: $modes
                event: {send: executehostcommand, cmd: 'fff insert'}
            }
            {
                name: fff_cd
                modifier: alt
                keycode: $"char_($cd_key)"
                mode: $modes
                event: {send: executehostcommand, cmd: 'fff cd'}
            }
        ]
}
