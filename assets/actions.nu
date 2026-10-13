# fff-Finder actions, written in Nushell.
#
# Every command exported here appears when you right-click items, under
# "Nushell Actions", and in the toolbar's Nushell menu. It receives the
# selected items as a table on $in, with these columns:
#
#   name, path, type ("file" or "dir"), kind, size (filesize), modified (datetime)
#
# What it returns shows in the Nushell panel. The comment above a command is
# its tooltip. fff-Finder reloads this file when you save it.

# Count the lines in each selected text file.
export def "count lines" []: table -> table {
    $in
    | where type == file
    | each {|f|
        # A NUL byte near the start means a binary file, which has no lines.
        try {
            if (open --raw $f.path | first 8192 | bytes index-of 0x[00]) == -1 {
                {name: $f.name, lines: (open --raw $f.path | lines | length)}
            }
        }
    }
    | sort-by lines --reverse
}

# Add up how many files and folders are selected and how big they are.
export def "total size" []: table -> record {
    let items = $in
    let files = $items | where type == file
    let folders = $items | where type == dir
    {
        files: ($files | length)
        size: ($files | get size | compact | prepend 0b | math sum)
        folders: ($folders | length)
        "folders size": ($folders | each {|d| try { du $d.path | get 0.apparent } } | prepend 0b | math sum)
    }
}

# Markdown links to the selected items, ready to paste into notes.
export def "markdown links" []: table -> string {
    $in
    | each {|f|
        let text = $f.name | str replace --all --regex '([\[\]\\])' '\$1'
        # Percent-encode what a URL or a Markdown link can't hold as is.
        let url = $f.path
            | str replace --all '\' '/'
            | split chars
            | each {|c| if $c =~ '^[A-Za-z0-9/._~:-]$' { $c } else { $c | url encode --all } }
            | str join
        # Windows paths start with a drive letter: file:///C:/...
        let url = if ($url | str starts-with '/') { $url } else { $"/($url)" }
        $"- [($text)]\(file://($url))"
    }
    | str join (char nl)
}

# Find files with the same contents among the selection and folders in it.
export def "find duplicates" []: table -> table {
    $in
    | each {|item|
        if $item.type == dir {
            # Listing from inside the folder keeps [ ] * ? in its name literal.
            do { cd $item.path; glob '**/*' --no-dir --no-symlink }
        } else {
            [$item.path]
        }
    }
    | flatten
    | uniq
    | each {|p| try { {path: $p, size: (ls $p | get 0.size)} } }
    | where size > 0b
    | group-by {|f| $f.size | into int | into string }
    | values
    | where ($it | length) > 1
    | flatten
    | each {|f| try { $f | insert sha256 (open --raw $f.path | hash sha256) } }
    | group-by sha256
    | values
    | where ($it | length) > 1
    | flatten
    | sort-by size --reverse
    | select sha256 size path
}

# Pack the selected items into a .tar.gz archive next to them.
export def "archive" []: table -> record {
    let items = $in
    if ($items | is-empty) { error make {msg: 'select something to archive'} }
    let dir = $items | first | get path | path dirname
    let stamp = date now | format date '%Y-%m-%d %H.%M.%S'
    let archive = $dir | path join $"Archive ($stamp).tar.gz"
    # -C before each item lets the selection span folders; ./ keeps a name
    # that starts with - from reading as an option.
    let args = $items | each {|i| ['-C' ($i.path | path dirname) $"./($i.path | path basename)"] } | flatten
    # COPYFILE_DISABLE keeps macOS from adding ._ files for extended attributes.
    let result = with-env {COPYFILE_DISABLE: '1'} { ^tar -czf $archive ...$args | complete }
    if $result.exit_code != 0 {
        rm --force $archive
        error make {msg: ($result.stderr | str trim)}
    }
    {archive: $archive, items: ($items | length), size: (ls $archive | get 0.size)}
}
