$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$msgFile = Join-Path ".git" "COMMIT_EDITMSG_CLEAN"
$utf8 = New-Object System.Text.UTF8Encoding $false
$title = $args[0]
$body = if ($args.Count -gt 1) { $args[1] } else { "" }
$lines = if ($body) { @($title, "", $body) } else { @($title) }
[System.IO.File]::WriteAllLines($msgFile, $lines, $utf8)
git add -A
$tree = (git write-tree).Trim()
$parent = (git rev-parse HEAD).Trim()
$new = (& git.exe commit-tree $tree -p $parent "-F" $msgFile).Trim()
git reset --hard $new
git log -1 --format='%B'
