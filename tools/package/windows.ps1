# A Windows release of the game app: dist\LovePlus+-VERSION-windows-x64.zip with LovePlus+.exe, the translation and
# the notices. Nothing from the game is in it: the app reads the player's ROM on first run.
# Signing: with $env:SIGN_CERT (a .pfx) and $env:SIGN_PASSWORD set, the program is signed with signtool (Windows SDK);
# unsigned programs show a SmartScreen warning on first run.
$ErrorActionPreference = "Stop"
$Root = Resolve-Path "$PSScriptRoot\..\.."
$Dist = "$Root\dist"
$Version = (Select-String -Path "$Root\engine\Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
$Delta = "$Dist\translation-en.lpdelta"
if (!(Test-Path $Delta)) { throw "no $Delta: python tools\lp_delta.py build content patch dist\translation-en.lpdelta" }
Push-Location "$Root\engine"
cargo build --release --bin game
Pop-Location
$Target = (rustc -vV | Select-String '^host: (.*)').Matches.Groups[1].Value
$Dir = "$Root\engine\target\LovePlus+-$Version"
if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
New-Item -ItemType Directory $Dir | Out-Null
Copy-Item "$Root\engine\target\release\game.exe" "$Dir\LovePlus+.exe"
Copy-Item $Delta $Dir
python "$Root\tools\package\notices.py" "$Dir\THIRD_PARTY_NOTICES.txt" --target $Target
if ($env:SIGN_CERT) {
    signtool sign /f $env:SIGN_CERT /p $env:SIGN_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 "$Dir\LovePlus+.exe"
}
$Out = "$Dist\LovePlus+-$Version-windows-x64.zip"
if (Test-Path $Out) { Remove-Item $Out }
Compress-Archive -Path "$Dir\*" -DestinationPath $Out
Write-Output $Out
