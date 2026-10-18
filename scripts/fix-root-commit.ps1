$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$msgFile = Join-Path ".git" "COMMIT_EDITMSG_CLEAN"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllLines(
  (Resolve-Path ".git").Path + "\COMMIT_EDITMSG_CLEAN",
  @(
    "Initial public release of MOSICKWHILE.",
    "",
    "Windows anti-motion-sickness overlay with pupil FOV, gaming mode, and Present bridge. Source only; binaries ship via GitHub Releases."
  ),
  $utf8
)

git add -A
$tree = (git write-tree).Trim()
$new = (& git.exe commit-tree $tree "-F" $msgFile).Trim()
git reset --hard $new
git log -1 --format=full
Write-Host "OK $new"
