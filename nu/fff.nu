# fffinder commands for Nushell.
#
#     use /path/to/fffinder/nu/fff.nu *
#     fff keybindings      # Ctrl+T inserts picked paths, Alt+C picks a folder to cd into
#
# `fff find`, `fff search`, and the rest ask the running fffinder window,
# which keeps its index live, so nothing is scanned in the shell. The
# protocol is described in docs/protocol.md.

const MODULE_DIR = path self | path dirname

# $env.FFFINDER_<name>, or the same variable under the earlier name
# (FFF_FINDER_), the way the app reads it.
def app-env [name: string]: nothing -> any {
    let value = ['FFFINDER_' 'FFF_FINDER_']
        | each {|prefix| $env | get --optional $'($prefix)($name)' }
        | get 0?
    if ($value | is-empty) { null } else { $value }
}

# fffinder's data folder, worked out the way the app does.
def data-dir []: nothing -> path {
    let dir = app-env DATA_DIR
    if $dir != null {
        return ($dir | path expand)
    }
    match $nu.os-info.name {
        'macos' => ($nu.home-dir | path join Library 'Application Support' fffinder)
        'windows' => ($env.LOCALAPPDATA | path join fffinder)
        _ => ($env.XDG_DATA_HOME? | default ($nu.home-dir | path join .local share) | path join fffinder)
    }
}

# How to reach the running window, from run/server.json.
def server-info []: nothing -> record {
    let info = data-dir | path join run server.json
    if not ($info | path exists) {
        error make {msg: 'no fffinder window is running', help: 'start one with `fff gui`'}
    }
    open $info
}

# Send one request to the running window and return its JSON answer.
def request [
    method: string      # GET or POST
    endpoint: string    # For example /v1/find
    query: list = []    # Query parameters as {key, value} records; keys may repeat
    body?: any          # JSON body for POST
]: nothing -> any {
    let server = server-info
    let qs = if ($query | is-empty) { '' } else { '?' + ($query | url build-query) }
    let url = ($server.url? | default 'http://localhost') + $endpoint + $qs
    let headers = if $server.token? != null { {Authorization: $'Bearer ($server.token)'} } else { {} }
    let socket = $server.socket?
    let response = try {
        if $method == 'GET' {
            if $socket != null {
                http get --full --allow-errors --unix-socket $socket --headers $headers $url
            } else {
                http get --full --allow-errors --headers $headers $url
            }
        } else if $socket != null {
            http post --full --allow-errors --unix-socket $socket --headers $headers --content-type application/json $url ($body | to json)
        } else {
            http post --full --allow-errors --headers $headers --content-type application/json $url ($body | to json)
        }
    } catch {|err|
        error make {msg: $'fffinder is not answering: ($err.msg)', help: 'start it with `fff gui`'}
    }
    if $response.status != 200 {
        let message = try { $response.body.error } catch { $'request failed with status ($response.status)' }
        error make {msg: $message}
    }
    $response.body
}

# Start fffinder in the background if no window is answering yet.
def ensure-running []: nothing -> nothing {
    let answering = {|| try { request GET /v1/status | ignore; true } catch { false } }
    if (do $answering) { return }
    run-external (app-binary) '--detach'
    let started = 1..80 | each { $in } | any {|_| sleep 250ms; do $answering }
    if not $started {
        error make {msg: 'fffinder did not start within 20 seconds'}
    }
}

# Unix seconds to a date, keeping nothing as nothing.
def seconds-to-date []: any -> any {
    if $in == null { null } else { $in * 1_000_000_000 | into datetime }
}

def query-params [pairs: record]: nothing -> list {
    $pairs | transpose key value | where value != null
}

# The fffinder program: $env.FFFINDER_BIN, PATH, the app bundle, or a
# build in this repository ($env.CARGO_TARGET_DIR, then target/).
def app-binary []: nothing -> path {
    let name = if $nu.os-info.name == 'windows' { 'fffinder.exe' } else { 'fffinder' }
    let repo = $MODULE_DIR | path dirname
    # A relative CARGO_TARGET_DIR is taken from the repository root.
    let builds = [$env.CARGO_TARGET_DIR? 'target']
        | compact --empty
        | each {|dir| $repo | path join $dir }
        | each {|dir| [($dir | path join release $name) ($dir | path join debug $name)] }
        | flatten
    let candidates = [
        (app-env BIN)
        (which $name | get 0?.path)
        '/Applications/fffinder.app/Contents/MacOS/fffinder'
        ($nu.home-dir | path join Applications fffinder.app Contents MacOS fffinder)
        ...$builds
    ]
    let found = $candidates | compact | where {|p| ($p | path exists) and ($p | path type) == 'file' } | get 0?
    if $found == null {
        error make {msg: 'fffinder was not found; set $env.FFFINDER_BIN or run scripts/install.nu'}
    }
    $found
}

# Choose files in the fffinder window and return their paths.
#
# Return (or double-click) chooses and Escape cancels, which returns nothing.
@example "Pick a CSV file and open it as a table" { open (fff pick *.csv) }
@example "Pick several files and list them" { fff pick --multiple | each { ls -l $in } }
export def "fff pick" [
    ...query: string    # Initial search text
    --root (-r): path   # Folder to open instead of the current one
    --multiple (-m)     # Allow choosing several items; always returns a list
]: nothing -> any {
    let root = $root | default $env.PWD | path expand
    let args = (
        [--pick $root]
        | append (if $multiple { [--multiple] } else { [] })
        | append (if ($query | is-empty) { [] } else { [--query ($query | str join ' ')] })
    )
    let result = run-external (app-binary) ...$args | complete
    if $result.exit_code != 0 {
        return (if $multiple { [] } else { null })
    }
    let paths = $result.stdout | from json
    if $multiple { $paths } else { $paths | get 0? }
}

# Open the fffinder window at a folder, optionally searching.
@example "Open the window at the current folder" { fff gui }
@example "Search for READMEs under ~/src" { fff gui ~/src --query readme }
export def "fff gui" [
    path?: path          # Folder to open (default: the current folder)
    --query (-q): string # Start a search
]: nothing -> nothing {
    let dir = $path | default $env.PWD | path expand
    let args = [--detach $dir] | append (if $query == null { [] } else { [--query $query] })
    run-external (app-binary) ...$args
}

# Find files and folders by name in fffinder's live index, ranked the way
# the window ranks them.
@example "Find a file by name" { fff find main rs }
@example "Documents only, best match opened as data" { fff find budget --kind document | first | open $in.path }
@example "Only under the current folder" { fff find readme --here }
export def "fff find" [
    ...query: string     # Search words, with fffinder's search syntax
    --files (-f)         # Files only
    --dirs (-d)          # Folders only
    --kind (-k): string  # document, image, movie, audio, code, archive, or folder
    --scope (-s): path   # Only inside this folder
    --here               # Only inside the current folder
    --limit (-l): int = 100  # Maximum rows
]: nothing -> table {
    let type = if $files { 'file' } else if $dirs { 'dir' } else { 'all' }
    let scope = if $here { $env.PWD } else if $scope != null { $scope | path expand } else { null }
    let params = query-params {
        query: ($query | str join ' ')
        type: $type
        kind: $kind
        scope: $scope
        limit: $limit
    }
    request GET /v1/find $params | each {|row|
        {
            name: ($row.path | path basename)
            type: $row.type
            size: (if $row.size? == null { null } else { $row.size | into filesize })
            modified: ($row.modified? | seconds-to-date)
            git: $row.git?
            path: $row.path
            score: $row.score
        }
    }
}

# List every indexed file, or the ones whose names match `query`.
export def "fff files" [
    query?: string       # Fuzzy filter
    --limit (-l): int    # Maximum rows
]: nothing -> table {
    request GET /v1/files (query-params {query: $query, limit: $limit}) | each {|row|
        {
            name: ($row.path | path basename)
            size: ($row.size | into filesize)
            modified: ($row.modified | seconds-to-date)
            path: $row.path
            root: $row.root
        }
    }
}

# List every indexed folder, or the ones whose names match `query`.
export def "fff dirs" [
    query?: string       # Fuzzy filter
    --limit (-l): int    # Maximum rows
]: nothing -> table {
    request GET /v1/dirs (query-params {query: $query, limit: $limit}) | each {|row|
        {name: ($row.path | path basename), path: $row.path, root: $row.root}
    }
}

# Search file contents in fffinder's index.
@example "Plain text" { fff search TODO }
@example "A regex, with a line of context" { fff search --regex 'fn \w+_test' --context 1 }
@example "Group matches by file" { fff search FIXME | group-by path }
export def "fff search" [
    ...pattern: string     # Text to find; a line matching any pattern counts
    --regex (-r)           # Patterns are regular expressions
    --fuzzy (-f)           # Tolerate typos and gaps
    --context (-C): int    # Lines of context before and after
    --before (-B): int     # Lines of context before
    --after (-A): int      # Lines of context after
    --scope (-s): path     # Only inside this folder
    --here                 # Only inside the current folder
    --limit (-l): int = 200  # Maximum matches
]: nothing -> table {
    if ($pattern | is-empty) { error make {msg: 'give at least one pattern'} }
    let mode = if $regex { 'regex' } else if $fuzzy { 'fuzzy' } else { 'plain' }
    let scope = if $here { $env.PWD } else if $scope != null { $scope | path expand } else { null }
    let params = (
        $pattern | each {|p| {key: pattern, value: $p} }
        | append (query-params {
            mode: $mode
            before: ($before | default $context)
            after: ($after | default $context)
            scope: $scope
            limit: $limit
        })
    )
    request GET /v1/search $params | each {|row|
        let base = {
            path: $row.path
            line_number: $row.line_number
            column: $row.column
            line: $row.line
        }
        if ($row.context_before | is-empty) and ($row.context_after | is-empty) {
            $base
        } else {
            $base | insert context ($row.context_before ++ [$row.line] ++ $row.context_after)
        }
    }
}

# The running window's index: locations, file counts, and whether it is scanning.
export def "fff status" []: nothing -> record {
    let status = request GET /v1/status
    $status | update roots {|s| $s.roots | update path {|r| $r.path } }
}

# Stream file changes from fffinder's index until --timeout or --max-events.
@example "The next change to a Rust file" { fff watch '**/*.rs' --max-events 1 }
export def "fff watch" [
    pattern?: string           # Glob, path, or folder; empty watches everything
    --ignore (-i): list<string> # Globs or path prefixes to skip
    --timeout (-t): duration   # Stop after this long
    --max-events (-n): int     # Stop after this many changes
]: nothing -> table {
    if $timeout == null and $max_events == null {
        error make {msg: 'give --timeout or --max-events, so the command ends'}
    }
    let params = (
        $ignore | default [] | each {|i| {key: ignore, value: $i} }
        | append (query-params {
            pattern: $pattern
            timeout_ms: (if $timeout == null { null } else { $timeout // 1ms })
            max_events: $max_events
        })
    )
    let server = server-info
    let qs = '?' + ($params | url build-query)
    let url = ($server.url? | default 'http://localhost') + '/v1/watch' + $qs
    let headers = if $server.token? != null { {Authorization: $'Bearer ($server.token)'} } else { {} }
    let body = if $server.socket? != null {
        http get --raw --unix-socket $server.socket --headers $headers $url
    } else {
        http get --raw --headers $headers $url
    }
    $body | lines | where $it != '' | each { from json }
}

# What is selected in the front fffinder window (or, with --all, everything it shows).
export def "fff selection" [
    --all (-a)  # Every item shown instead of only the selection
]: nothing -> table {
    let params = if $all { [{key: all, value: true}] } else { [] }
    request GET /v1/selection $params | get items | each {|row|
        {
            name: $row.name
            type: $row.type
            size: (if $row.size? == null { null } else { $row.size | into filesize })
            modified: ($row.modified? | seconds-to-date)
            path: $row.path
        }
    }
}

# Show files in the fffinder window: paths, `ls` or `glob` output, or any
# table with a path or name column. Starts fffinder if it isn't running.
@example "Browse big images as icons" { glob **/*.{png,jpg} | where { (ls $in | get 0.size) > 5mb } | fff show --title 'Big images' }
@example "Recently changed files" { ls **/* | where modified > ((date now) - 1day) | fff show }
export def "fff show" [
    --title (-t): string  # Window title for the results
]: any -> record {
    let paths = $in | each {|item|
        if ($item | describe) == 'string' { $item } else { $item.path? | default $item.name? }
    } | compact | each { path expand }
    if ($paths | is-empty) { error make {msg: 'nothing to show; pipe in paths or a table with a path or name column'} }
    ensure-running
    request POST /v1/show [] {paths: $paths, title: ($title | default 'From Nushell')}
}

# Fuzzy-find documents and open the best match as structured data.
export def "fff open" [
    ...query: string  # Search words, as for `fff find`
]: nothing -> any {
    let hit = fff find --kind document --limit 1 ...$query | get 0?.path
    if $hit == null {
        error make {msg: $"no document matches '($query | str join ' ')'"}
    }
    open $hit
}

# Pick files in fffinder and insert them at the cursor as NUON.
export def "fff insert" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick --multiple ...$query
    if ($picked | is-empty) { return }
    let text = $picked | each { to nuon } | str join ' '
    commandline edit --insert $text
}

# Pick a folder (or a file's folder) in fffinder and cd into it.
export def --env "fff cd" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick ...$query
    if $picked == null { return }
    let dir = if ($picked | path type) == 'dir' { $picked } else { $picked | path dirname }
    cd $dir
}

# Pick a file in fffinder and open it in $env.config.buffer_editor,
# $env.VISUAL, or $env.EDITOR.
export def "fff edit" [
    ...query: string  # Initial search text
]: nothing -> nothing {
    let picked = fff pick ...$query
    if $picked == null { return }
    let fallback = if $nu.os-info.name == 'windows' { 'notepad' } else { 'vi' }
    let editor = $env.config.buffer_editor? | default $env.VISUAL? | default $env.EDITOR? | default $fallback
    let words = if ($editor | describe) =~ '^list' { $editor } else { [$editor] }
    if ($words | is-empty) { error make {msg: 'buffer_editor is an empty list'} }
    run-external ($words | first) ...($words | skip 1) $picked
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
