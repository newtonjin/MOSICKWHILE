$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$msgFile = Join-Path ".git" "COMMIT_EDITMSG_CLEAN"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllLines($msgFile, @($args[0], "", $args[1]), $utf8)
git add -A
$tree = (git write-tree).Trim()
$parent = (git rev-parse HEAD).Trim()
$new = (& git.exe commit-tree $tree -p $parent "-F" $msgFile).Trim()
git reset --hard $new
git log -1 --format='%B'
