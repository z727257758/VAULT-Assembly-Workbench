$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$archive = Join-Path $workspace 'target/OpenSCAD-2021.01-x86-64.zip'
New-Item -ItemType Directory -Force (Join-Path $workspace 'target') | Out-Null
Invoke-WebRequest -Uri 'https://files.openscad.org/OpenSCAD-2021.01-x86-64.zip' -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne 'FB0CAABF5BBC89F8F2F80C10B79AE64D697AAFF6EFD58B2756F5D6270EDB7BA7') {throw 'OpenSCAD archive checksum mismatch'}
Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $workspace 'tools/openscad') -Force
Write-Output 'Gridfinity generator ready (OpenSCAD 2021.01 portable).'
