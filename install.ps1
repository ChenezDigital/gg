# gg installer for Windows (x86_64 / ARM64). Works in Windows PowerShell 5.1 and PowerShell 7+.
#
#   irm https://raw.githubusercontent.com/ChenezDigital/gg/main/install.ps1 | iex
#
# Environment variables:
#   GG_VERSION         release tag to install, e.g. v0.1.0 (default: latest)
#   GG_INSTALL_DIR     where to put gg.exe (default: %LOCALAPPDATA%\Programs\gg)
#   GG_REPO            GitHub repository to download from (default: ChenezDigital/gg)
#   GG_BASE_URL        download base URL; overrides GG_REPO/GG_VERSION (mirrors, testing)
#   GG_NO_MODIFY_PATH  set to 1 to leave the user PATH alone

# Everything runs in a child scope: `irm | iex` executes in the caller's session, so preferences set
# here must not leak into it, and errors use `throw` because `exit` would close the caller's window.
& {
    $ErrorActionPreference = 'Stop'
    # Windows PowerShell 5.1 redraws the progress bar per chunk, which makes downloads very slow.
    $ProgressPreference = 'SilentlyContinue'
    Set-StrictMode -Version 2.0

    function Get-Setting([string]$Name, [string]$Default) {
        $value = [Environment]::GetEnvironmentVariable($Name)
        if ([string]::IsNullOrEmpty($value)) { return $Default }
        return $value
    }

    function Say([string]$Message) { Write-Host "gg-install: $Message" }

    function Get-Target {
        # The machine-wide value is the native architecture even in an x86 or emulated x64 PowerShell.
        $arch = [Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITECTURE', 'Machine')
        if ([string]::IsNullOrEmpty($arch)) { $arch = $env:PROCESSOR_ARCHITECTURE }
        switch ($arch) {
            'AMD64' { return 'x86_64-pc-windows-msvc' }
            'ARM64' { return 'aarch64-pc-windows-msvc' }
            default { throw "gg-install: unsupported CPU architecture: $arch (supported: AMD64, ARM64)" }
        }
    }

    function Assert-SecureUrl([string]$Url) {
        if ($Url -like 'https://*') { return }
        if ($Url -like 'http://127.0.0.1*' -or $Url -like 'http://localhost*') { return }
        throw "gg-install: refusing non-HTTPS URL: $Url"
    }

    function Get-File([string]$Url, [string]$OutFile) {
        Assert-SecureUrl $Url
        Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    }

    function Test-SamePath([string]$A, [string]$B) {
        return [string]::Equals($A.Trim().TrimEnd('\'), $B.Trim().TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    }

    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw "gg-install: PowerShell 5.1 or later is required (found $($PSVersionTable.PSVersion))"
    }
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'gg-install: this installer is for Windows; on Linux and macOS use install.sh'
    }

    # .NET Framework may not offer TLS 1.2 by default, and GitHub requires it.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = Get-Setting 'GG_REPO' 'ChenezDigital/gg'
    $version = Get-Setting 'GG_VERSION' 'latest'
    $installDir = Get-Setting 'GG_INSTALL_DIR' (Join-Path $env:LOCALAPPDATA 'Programs\gg')
    $target = Get-Target
    $asset = "gg-$target.zip"

    $base = Get-Setting 'GG_BASE_URL' ''
    if ($base -eq '') {
        if ($version -eq 'latest') {
            $base = "https://github.com/$repo/releases/latest/download"
        } else {
            $base = "https://github.com/$repo/releases/download/$version"
        }
    }
    $base = $base.TrimEnd('/')

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('gg-install-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $zip = Join-Path $tmp $asset
        Say "downloading $base/$asset"
        Get-File "$base/$asset" $zip
        Get-File "$base/$asset.sha256" "$zip.sha256"

        $expected = ((Get-Content -LiteralPath "$zip.sha256" -Raw).Trim() -split '\s+')[0]
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash
        if (-not [string]::Equals($expected, $actual, [StringComparison]::OrdinalIgnoreCase)) {
            throw "gg-install: checksum mismatch for $asset (expected $expected, got $actual)"
        }
        Say 'checksum verified'

        Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
        $src = Join-Path $tmp "gg-$target"
        $newExe = Join-Path $src 'gg.exe'
        if (-not (Test-Path -LiteralPath $newExe -PathType Leaf)) {
            throw "gg-install: $asset does not contain gg-$target\gg.exe"
        }

        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        $dest = Join-Path $installDir 'gg.exe'
        if (Test-Path -LiteralPath $dest) {
            # A running gg.exe cannot be overwritten, but it can be renamed.
            Remove-Item -LiteralPath "$dest.old" -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $dest -Destination "$dest.old" -Force
        }
        Copy-Item -LiteralPath $newExe -Destination $dest -Force
        Remove-Item -LiteralPath "$dest.old" -Force -ErrorAction SilentlyContinue

        $completions = Join-Path $src 'completions'
        if (Test-Path -LiteralPath $completions) {
            Copy-Item -Path $completions -Destination $installDir -Recurse -Force
        }

        $installed = (& $dest --version) -join ' '
        Say "installed $installed to $dest"

        $onPath = $false
        foreach ($dir in ($env:Path -split ';')) {
            if ($dir -ne '' -and (Test-SamePath $dir $installDir)) { $onPath = $true }
        }
        if (-not $onPath) {
            if ((Get-Setting 'GG_NO_MODIFY_PATH' '') -in @('', '0')) {
                $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
                $inUserPath = $false
                if ($userPath) {
                    foreach ($dir in ($userPath -split ';')) {
                        if ($dir -ne '' -and (Test-SamePath $dir $installDir)) { $inUserPath = $true }
                    }
                }
                if (-not $inUserPath) {
                    $newPath = if ($userPath) { $userPath.TrimEnd(';') + ";$installDir" } else { $installDir }
                    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
                    Say "added $installDir to your user PATH (new terminals pick it up)"
                }
                $env:Path = $env:Path.TrimEnd(';') + ";$installDir"
            } else {
                Say "$installDir is not on PATH; add it to run gg by name"
            }
        }

        Say 'next: run `gg setup`'
        Say 'tab completion: add this line to your $PROFILE:'
        Write-Host '    gg completions powershell | Out-String | Invoke-Expression'
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
