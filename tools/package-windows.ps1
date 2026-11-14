param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,

    [Parameter(Mandatory = $true)]
    [string]$Modules,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [string]$ArchivePath = ''
)

$ErrorActionPreference = 'Stop'

$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$modulesPath = (Resolve-Path -LiteralPath $Modules).Path
if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
    throw "Viewer executable not found: $executablePath"
}
if (-not (Test-Path -LiteralPath (Join-Path $modulesPath 'Walker/module.ini') -PathType Leaf)) {
    throw "Walker module manifest not found under: $modulesPath"
}

$outputPath = [System.IO.Path]::GetFullPath($OutputDirectory)
$packageRoot = Join-Path $outputPath 'Perfect World Explorer'
if (Test-Path -LiteralPath $packageRoot) {
    Remove-Item -LiteralPath $packageRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $packageRoot | Out-Null

$shippedExecutable = Join-Path $packageRoot 'Perfect-World-Explorer.exe'
Copy-Item -LiteralPath $executablePath -Destination $shippedExecutable
$shippedModules = Join-Path $packageRoot 'Modules'
Copy-Item -LiteralPath $modulesPath -Destination $shippedModules -Recurse

# Both the project's GPL and the AMD FSR 1 attribution must accompany binary
# distributions. The latter does not relicense the rest of the project.
$noticePaths = @('LICENSE', 'THIRD_PARTY_NOTICES.md')
foreach ($notice in $noticePaths) {
    $source = Join-Path (Join-Path $PSScriptRoot '..') $notice
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Distribution notice missing: $source"
    }
    Copy-Item -LiteralPath $source -Destination (Join-Path $packageRoot $notice)
}

$walkerManifest = Join-Path $shippedModules 'Walker/module.ini'
if (-not (Test-Path -LiteralPath $walkerManifest -PathType Leaf)) {
    throw "Staging did not produce Modules/Walker/module.ini in $packageRoot"
}

# Keep checksums with the portable folder. No development/survey executables
# are inputs to this script, so only the viewer and installed module packages
# can enter the distribution.
$files = @((Get-Item -LiteralPath $shippedExecutable)) +
    @((Get-Item -LiteralPath (Join-Path $packageRoot 'LICENSE'))) +
    @((Get-Item -LiteralPath (Join-Path $packageRoot 'THIRD_PARTY_NOTICES.md'))) +
    @((Get-ChildItem -LiteralPath $shippedModules -Recurse -File | Sort-Object FullName))
$checksumLines = @(
    foreach ($file in $files) {
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $relative = [System.IO.Path]::GetRelativePath($packageRoot, $file.FullName).Replace('\', '/')
        "$hash  $relative"
    }
)
[System.IO.File]::WriteAllLines(
    (Join-Path $packageRoot 'SHA256SUMS.txt'),
    [string[]]$checksumLines,
    [System.Text.Encoding]::ASCII
)

if (-not [string]::IsNullOrWhiteSpace($ArchivePath)) {
    $archiveFullPath = [System.IO.Path]::GetFullPath($ArchivePath)
    $archiveDirectory = Split-Path -Parent $archiveFullPath
    New-Item -ItemType Directory -Force -Path $archiveDirectory | Out-Null
    if (Test-Path -LiteralPath $archiveFullPath) {
        Remove-Item -LiteralPath $archiveFullPath -Force
    }

    # No trailing wildcard: Compress-Archive retains the top-level
    # "Perfect World Explorer" directory in the release ZIP.
    Compress-Archive -LiteralPath $packageRoot -DestinationPath $archiveFullPath -CompressionLevel Optimal

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($archiveFullPath)
    try {
        $entries = @($archive.Entries | ForEach-Object { $_.FullName.Replace('\', '/') })
        foreach ($required in @(
            'Perfect World Explorer/Perfect-World-Explorer.exe',
            'Perfect World Explorer/Modules/Walker/module.ini',
            'Perfect World Explorer/Modules/Populator/module.ini',
            'Perfect World Explorer/LICENSE',
            'Perfect World Explorer/THIRD_PARTY_NOTICES.md',
            'Perfect World Explorer/SHA256SUMS.txt'
        )) {
            if ($entries -notcontains $required) {
                throw "Release archive is missing required entry: $required"
            }
        }
        if ($entries | Where-Object { $_ -match '(?i)bmd.*survey|survey.*bmd' }) {
            throw 'A BMD survey executable was unexpectedly included in the release archive.'
        }
    }
    finally {
        $archive.Dispose()
    }

    $archiveHash = (Get-FileHash -LiteralPath $archiveFullPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $archiveName = [System.IO.Path]::GetFileName($archiveFullPath)
    [System.IO.File]::WriteAllLines(
        (Join-Path $archiveDirectory 'SHA256SUMS.txt'),
        [string[]]@("$archiveHash  $archiveName"),
        [System.Text.Encoding]::ASCII
    )
}

Write-Host "Staged portable viewer at $packageRoot"
