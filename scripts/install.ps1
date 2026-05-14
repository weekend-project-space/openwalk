[CmdletBinding()]
param(
    [string]$Version = $(if ($env:OPENWALK_VERSION) { $env:OPENWALK_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:OPENWALK_INSTALL_DIR) { $env:OPENWALK_INSTALL_DIR } else { Join-Path $HOME ".openwalk\bin" }),
    [string]$BaseUrl = $(if ($env:OPENWALK_RELEASE_BASE_URL) { $env:OPENWALK_RELEASE_BASE_URL } else { "https://github.com/weekend-project-space/openwalk/releases" }),
    [switch]$NoPath
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

try {
    if ([System.Enum]::GetNames([Net.SecurityProtocolType]) -contains "Tls12") {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    }
} catch {
}

function Write-Log {
    param([string]$Message)
    Write-Host "[openwalk-install] $Message"
}

function Fail {
    param([string]$Message)
    throw "[openwalk-install] error: $Message"
}

function Download-File {
    param(
        [string]$Url,
        [string]$OutFile,
        [string[]]$ExpectedAssets = @()
    )

    try {
        Invoke-WebRequest -Uri $Url -OutFile $OutFile
        return
    } catch {
        $statusCode = $null

        try {
            if ($_.Exception.Response -and $_.Exception.Response.StatusCode) {
                $statusCode = [int]$_.Exception.Response.StatusCode
            }
        } catch {
        }

        if ($statusCode -eq 404) {
            $hintLines = @(
                "[openwalk-install] error: no published release asset was found for:",
                "[openwalk-install] error:   $Url",
                "[openwalk-install] hint: publish a GitHub Release first"
            )

            if ($ExpectedAssets.Count -gt 0) {
                $hintLines += "[openwalk-install] hint: expected assets:"
                foreach ($asset in $ExpectedAssets) {
                    $hintLines += "[openwalk-install] hint:   $asset"
                }
            }

            $hintLines += "[openwalk-install] hint: or build locally with: cargo build --release"
            throw ($hintLines -join [Environment]::NewLine)
        }

        Fail "failed to download $Url. $($_.Exception.Message)"
    }
}

function Get-ReleasePath {
    param([string]$RequestedVersion)

    if ($RequestedVersion -eq "latest") {
        return "download/latest"
    }

    return "download/$RequestedVersion"
}

function Get-TargetTriple {
    $arch = $null

    try {
        $runtimeInformation = [System.Type]::GetType("System.Runtime.InteropServices.RuntimeInformation")
        if ($runtimeInformation) {
            $archProperty = $runtimeInformation.GetProperty("OSArchitecture", [System.Reflection.BindingFlags]::Public -bor [System.Reflection.BindingFlags]::Static)
            if ($archProperty) {
                $arch = $archProperty.GetValue($null, @()).ToString()
            }
        }
    } catch {
    }

    if (-not $arch) {
        $processorArchitecture = [Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITECTURE")
        $processorArchitectureW6432 = [Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITEW6432")
        $detectedArchitecture = if (-not [string]::IsNullOrWhiteSpace($processorArchitectureW6432)) {
            $processorArchitectureW6432
        } else {
            $processorArchitecture
        }

        if ($null -eq $detectedArchitecture) {
            $detectedArchitecture = ""
        }

        switch ($detectedArchitecture.ToUpperInvariant()) {
            "AMD64" { $arch = "X64" }
            "X86" {
                if ([Environment]::Is64BitOperatingSystem) {
                    $arch = "X64"
                } else {
                    Fail "unsupported Windows architecture: x86"
                }
            }
            "ARM64" { $arch = "Arm64" }
            default { Fail "unsupported Windows architecture: $detectedArchitecture" }
        }
    }

    switch ($arch) {
        "X64" { return "x86_64-pc-windows-msvc" }
        "Arm64" { return "aarch64-pc-windows-msvc" }
        default { Fail "unsupported Windows architecture: $arch" }
    }
}

function Normalize-PathEntry {
    param([string]$Entry)

    $expanded = [Environment]::ExpandEnvironmentVariables($Entry)
    return [System.IO.Path]::GetFullPath($expanded).TrimEnd('\').ToLowerInvariant()
}

function Ensure-UserPath {
    param([string]$Directory)

    $normalizedTarget = Normalize-PathEntry -Entry $Directory
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    $processEntries = @()

    if (-not [string]::IsNullOrWhiteSpace($userPath)) {
        $entries = $userPath.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if (-not [string]::IsNullOrWhiteSpace($env:Path)) {
        $processEntries = $env:Path.Split(';', [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    foreach ($entry in $entries) {
        try {
            $normalizedEntry = Normalize-PathEntry -Entry $entry
            if ($normalizedEntry -eq $normalizedTarget) {
                $hasProcessPath = $false
                foreach ($processEntry in $processEntries) {
                    try {
                        if ((Normalize-PathEntry -Entry $processEntry) -eq $normalizedTarget) {
                            $hasProcessPath = $true
                            break
                        }
                    } catch {
                        continue
                    }
                }

                if (-not $hasProcessPath) {
                    $env:Path = "$Directory;$env:Path"
                }
                Write-Log "PATH already contains $Directory"
                return
            }
        } catch {
            continue
        }
    }

    $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
        $Directory
    } else {
        "$userPath;$Directory"
    }

    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")

    $hasProcessPath = $false
    foreach ($processEntry in $processEntries) {
        try {
            if ((Normalize-PathEntry -Entry $processEntry) -eq $normalizedTarget) {
                $hasProcessPath = $true
                break
            }
        } catch {
            continue
        }
    }

    if (-not $hasProcessPath) {
        $env:Path = "$Directory;$env:Path"
    }

    Write-Log "added $Directory to user PATH"
}

if (-not $env:OS -or $env:OS -ne "Windows_NT") {
    Fail "this installer is intended for Windows"
}

$targetTriple = Get-TargetTriple
$assetName = "openwalk-$targetTriple.zip"
$checksumName = "openwalk-checksums.txt"
$releasePath = Get-ReleasePath -RequestedVersion $Version
$assetUrl = "$BaseUrl/$releasePath/$assetName"
$checksumUrl = "$BaseUrl/$releasePath/$checksumName"

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("openwalk-install-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    $archivePath = Join-Path $tempRoot $assetName
    $checksumPath = Join-Path $tempRoot $checksumName

    Write-Log "downloading $assetUrl"
    Download-File -Url $assetUrl -OutFile $archivePath -ExpectedAssets @($assetName, $checksumName)

    Write-Log "downloading $checksumUrl"
    Download-File -Url $checksumUrl -OutFile $checksumPath -ExpectedAssets @($assetName, $checksumName)

    $expectedChecksum = $null
    foreach ($line in Get-Content -Path $checksumPath) {
        if ($line -match '^\s*([0-9a-fA-F]+)\s+\*?(.+?)\s*$') {
            $name = $Matches[2]
            if ($name -eq $assetName) {
                $expectedChecksum = $Matches[1].ToLowerInvariant()
                break
            }
        }
    }

    if (-not $expectedChecksum) {
        Fail "could not find checksum for $assetName in $checksumName"
    }

    $actualChecksum = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualChecksum -ne $expectedChecksum) {
        Fail "checksum mismatch for $assetName"
    }

    Write-Log "extracting archive"
    Expand-Archive -Path $archivePath -DestinationPath $tempRoot -Force

    $binary = Get-ChildItem -Path $tempRoot -Recurse -File -Filter "openwalk.exe" | Select-Object -First 1
    if (-not $binary) {
        Fail "failed to locate extracted openwalk.exe"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    $installPath = Join-Path $InstallDir "openwalk.exe"
    $tempInstallPath = Join-Path $InstallDir ".openwalk.tmp.$PID.exe"

    Copy-Item -Path $binary.FullName -Destination $tempInstallPath -Force
    Move-Item -Path $tempInstallPath -Destination $installPath -Force

    if (-not $NoPath) {
        Ensure-UserPath -Directory $InstallDir
    } else {
        Write-Log "skipped PATH update"
    }

    $versionOutput = & $installPath --version 2>$null

    Write-Log "installed to $installPath"
    if ($versionOutput) {
        Write-Log ($versionOutput -join " ")
    }

    if ($NoPath) {
        Write-Log "add $InstallDir to PATH to run openwalk directly"
    }
} finally {
    Remove-Item -Path $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
