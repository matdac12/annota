# Regenerates src-tauri/icons from the Callout drawings.
#
#   pwsh -File scripts/make-icon.ps1          # render + tauri icon + pack + verify
#   pwsh -File scripts/make-icon.ps1 -Verify  # only check the committed icons
#
# docs/mockups/icon-annota.svg is the drawing. docs/mockups/icon-annota-small.svg
# is the hand-tuned variant used at 16, 20 and 24 px, where the full one blurs.
# A Chromium browser rasterises the SVGs. `pnpm tauri icon` writes the PNGs and
# a first icon.ico. This script then rebuilds icon.ico so the small sizes are
# the small drawing. No new dependency.
param([switch]$Verify)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Sizes Windows actually asks for: the tray's small icon (16 / 20 / 24) and the
# large icon (32 / 48, and 256 for Explorer). 20 is not in every Tauri .ico;
# it is inserted here. 64 and 128 are kept when `tauri icon` emitted them.
$RequiredSizes = 16, 20, 24, 32, 48, 256
$SmallSizes = 16, 20, 24

$Root = Split-Path -Parent $PSScriptRoot
$Icons = Join-Path $Root 'src-tauri\icons'
$Ico = Join-Path $Icons 'icon.ico'
$Svg = Join-Path $Root 'docs\mockups\icon-annota.svg'
$SvgSmall = Join-Path $Root 'docs\mockups\icon-annota-small.svg'

Add-Type -AssemblyName System.Drawing

function Find-Browser {
    $candidates = @(
        "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
        "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
        "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe",
        "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
        "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
    )
    foreach ($c in $candidates) { if (Test-Path -LiteralPath $c) { return $c } }
    throw 'No Chrome or Edge found; one of them is needed to rasterise the SVGs.'
}

function Render-Svg([string]$Browser, [string]$SvgPath, [string]$OutPng, [int]$Size) {
    # Render in a window at least 512 px: Chrome clamps a smaller window and the
    # drawing shifts. Crop the top-left Size x Size.
    $window = [Math]::Max($Size, 512)
    $work = Join-Path $env:TEMP ("annota-icon-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    try {
        $svg = Get-Content -LiteralPath $SvgPath -Raw
        $html = @"
<!doctype html><html><head><meta charset="utf-8"><style>
html,body{margin:0;padding:0;background:transparent;overflow:hidden}
svg{display:block;width:${Size}px;height:${Size}px}
</style></head><body>$svg</body></html>
"@
        $page = Join-Path $work 'icon.html'
        Set-Content -LiteralPath $page -Value $html -Encoding utf8NoBOM
        $url = 'file:///' + ($page -replace '\\', '/')
        $shot = Join-Path $work 'shot.png'
        $cliArgs = @(
            '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run',
            '--no-default-browser-check', '--force-device-scale-factor=1',
            '--default-background-color=00000000',
            "--window-size=$window,$window",
            "--user-data-dir=$(Join-Path $work 'profile')",
            "--screenshot=$shot",
            $url
        )
        $proc = Start-Process -FilePath $Browser -ArgumentList $cliArgs -PassThru -Wait -WindowStyle Hidden
        $deadline = (Get-Date).AddSeconds(20)
        while (-not (Test-Path -LiteralPath $shot) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 100
        }
        if (-not (Test-Path -LiteralPath $shot)) {
            throw "Renderer exited $($proc.ExitCode) without writing $shot"
        }
        $bmp = [System.Drawing.Bitmap]::FromFile($shot)
        try {
            if ($bmp.Width -lt $Size -or $bmp.Height -lt $Size) {
                throw "Rendered $($bmp.Width)x$($bmp.Height), wanted at least ${Size}x$Size"
            }
            $crop = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $g = [System.Drawing.Graphics]::FromImage($crop)
                try {
                    $g.DrawImage($bmp, (New-Object System.Drawing.Rectangle(0, 0, $Size, $Size)),
                        (New-Object System.Drawing.Rectangle(0, 0, $Size, $Size)),
                        [System.Drawing.GraphicsUnit]::Pixel)
                }
                finally { $g.Dispose() }
                $crop.Save($OutPng, [System.Drawing.Imaging.ImageFormat]::Png)
            }
            finally { $crop.Dispose() }
        }
        finally { $bmp.Dispose() }
    }
    finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-IcoEntries([string]$Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 6) { throw "$Path is too short to be an icon" }
    $count = [int]$bytes[4] -bor ([int]$bytes[5] -shl 8)
    if ($bytes[2] -ne 1 -or $bytes[3] -ne 0) { throw "$Path is not an icon (type $($bytes[2]))" }
    $entries = @()
    for ($i = 0; $i -lt $count; $i++) {
        $o = 6 + 16 * $i
        $w = if ($bytes[$o] -eq 0) { 256 } else { [int]$bytes[$o] }
        $h = if ($bytes[$o + 1] -eq 0) { 256 } else { [int]$bytes[$o + 1] }
        $len = [BitConverter]::ToInt32($bytes, $o + 8)
        $off = [BitConverter]::ToInt32($bytes, $o + 12)
        $png = $bytes[$off] -eq 0x89 -and $bytes[$off + 1] -eq 0x50 -and
               $bytes[$off + 2] -eq 0x4E -and $bytes[$off + 3] -eq 0x47
        $entries += [pscustomobject]@{ Width = $w; Height = $h; Bytes = $len; Offset = $off; Png = $png }
    }
    return $entries
}

function Read-IconBitmap([string]$Path, [int]$Size, $Entry) {
    # System.Drawing.Icon mis-decodes PNG entries, so slice the PNG out.
    if (-not $Entry.Png) { throw "the $Size px entry of $Path is not PNG" }
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $slice = New-Object byte[] $Entry.Bytes
    [Array]::Copy($bytes, $Entry.Offset, $slice, 0, $Entry.Bytes)
    $ms = New-Object System.IO.MemoryStream(, $slice)
    return [System.Drawing.Bitmap]::FromStream($ms)
}

function Write-Ico([string]$Path, [int[]]$Sizes, [hashtable]$Pngs) {
    $stream = [System.IO.MemoryStream]::new()
    $writer = [System.IO.BinaryWriter]::new($stream)
    try {
        $writer.Write([uint16]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]$Sizes.Count)
        $offset = 6 + 16 * $Sizes.Count
        foreach ($size in $Sizes) {
            $writer.Write([byte]($(if ($size -ge 256) { 0 } else { $size })))
            $writer.Write([byte]($(if ($size -ge 256) { 0 } else { $size })))
            $writer.Write([byte]0)
            $writer.Write([byte]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]32)
            $writer.Write([uint32]$Pngs[$size].Length)
            $writer.Write([uint32]$offset)
            $offset += $Pngs[$size].Length
        }
        foreach ($size in $Sizes) { $writer.Write($Pngs[$size]) }
        $writer.Flush()
        [System.IO.File]::WriteAllBytes($Path, $stream.ToArray())
    }
    finally { $writer.Dispose(); $stream.Dispose() }
}

function Remove-PlatformExtras([string]$Dir) {
    # `tauri icon` also writes macOS, mobile and Appx logos, plus a loose
    # 64x64.png. This app ships an NSIS bundle and names its icons in
    # tauri.conf.json, so those outputs are removed. Listed before they are deleted.
    $names = @(
        'icon.icns', 'android', 'ios', '64x64.png', 'StoreLogo.png',
        'Square30x30Logo.png', 'Square44x44Logo.png', 'Square71x71Logo.png',
        'Square89x89Logo.png', 'Square107x107Logo.png', 'Square142x142Logo.png',
        'Square150x150Logo.png', 'Square284x284Logo.png', 'Square310x310Logo.png'
    )
    $extras = @(
        $names | ForEach-Object { Join-Path $Dir $_ } | Where-Object { Test-Path -LiteralPath $_ }
    )
    if (-not $extras) { return }
    "removing platform extras the Windows bundle does not use:"
    foreach ($path in $extras) { "  $path" }
    foreach ($path in $extras) { Remove-Item -LiteralPath $path -Recurse -Force }
}

function Test-PngFile([string]$Path, [int]$Size) {
    if (-not (Test-Path -LiteralPath $Path)) { throw "missing $Path" }
    $bmp = [System.Drawing.Bitmap]::FromFile($Path)
    try {
        if ($bmp.Width -ne $Size -or $bmp.Height -ne $Size) {
            throw "$Path is $($bmp.Width)x$($bmp.Height), wanted ${Size}x$Size"
        }
    }
    finally { $bmp.Dispose() }
}

function Count-Colours($Bitmap) {
    $navy = 0; $red = 0; $yellow = 0
    for ($y = 0; $y -lt $Bitmap.Height; $y++) {
        for ($x = 0; $x -lt $Bitmap.Width; $x++) {
            $p = $Bitmap.GetPixel($x, $y)
            if ($p.A -lt 128) { continue }
            # Tile: navy, blue channel above the others (#2D3C5E down to #172033).
            if ($p.B -gt $p.R + 15 -and $p.B -gt $p.G -and $p.B -ge 40 -and $p.R -le 90) { $navy++ }
            # Box: red above green and blue.
            if ($p.R -ge 160 -and ($p.R - $p.G) -ge 30 -and ($p.R - $p.B) -ge 40) { $red++ }
            # Note: yellow, both red and green high and blue low.
            if ($p.R -ge 190 -and $p.G -ge 140 -and $p.B -le 140 -and ($p.G - $p.B) -ge 40) { $yellow++ }
        }
    }
    return [pscustomobject]@{ Navy = $navy; Red = $red; Yellow = $yellow }
}

function Test-Icon([string]$Path) {
    $entries = Get-IcoEntries $Path
    $sizes = @($entries | ForEach-Object { $_.Width })
    foreach ($size in $RequiredSizes) {
        if ($sizes -notcontains $size) { throw "icon.ico is missing the $size px entry (has $($sizes -join ', '))" }
    }
    foreach ($e in $entries) {
        if ($e.Width -ne $e.Height) { throw "entry $($e.Width)x$($e.Height) is not square" }
        if (-not $e.Png) { throw "the $($e.Width) px entry is not PNG" }
    }

    $big = Read-IconBitmap $Path 48 ($entries | Where-Object Width -eq 48)
    try { $at48 = Count-Colours $big }
    finally { $big.Dispose() }
    if ($at48.Navy -lt 400) { throw "48 px has $($at48.Navy) navy pixels; the tile is missing" }
    if ($at48.Red -lt 80) { throw "48 px has $($at48.Red) red pixels; the Box is missing" }
    if ($at48.Yellow -lt 80) { throw "48 px has $($at48.Yellow) yellow pixels; the Note is missing" }

    $tiny = Read-IconBitmap $Path 16 ($entries | Where-Object Width -eq 16)
    try { $at16 = Count-Colours $tiny }
    finally { $tiny.Dispose() }
    # The small drawing: a readable red frame and a yellow Note, not a smear.
    if ($at16.Red -lt 12) { throw "16 px has $($at16.Red) red pixels; the Box is unreadable" }
    if ($at16.Yellow -lt 8) { throw "16 px has $($at16.Yellow) yellow pixels; the Note is unreadable" }
    if ($at16.Navy -lt 20) { throw "16 px has $($at16.Navy) navy pixels; the tile is missing" }

    Test-PngFile (Join-Path $Icons '32x32.png') 32
    Test-PngFile (Join-Path $Icons '128x128.png') 128
    Test-PngFile (Join-Path $Icons '128x128@2x.png') 256
    $iconPng = Join-Path $Icons 'icon.png'
    if (-not (Test-Path -LiteralPath $iconPng)) { throw "missing $iconPng" }
    $sheet = [System.Drawing.Bitmap]::FromFile($iconPng)
    try {
        if ($sheet.Width -ne $sheet.Height -or $sheet.Width -lt 512) {
            throw "icon.png is $($sheet.Width)x$($sheet.Height); wanted a square of at least 512"
        }
    }
    finally { $sheet.Dispose() }

    "src-tauri/icons/icon.ico: $($entries.Count) entries, sizes $($sizes -join ', '); 48 px navy $($at48.Navy) red $($at48.Red) yellow $($at48.Yellow); 16 px navy $($at16.Navy) red $($at16.Red) yellow $($at16.Yellow)"
}

if (-not $Verify) {
    $browser = Find-Browser
    $tmp = Join-Path $env:TEMP ("annota-icon-build-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $large = Join-Path $tmp 'icon-1024.png'
        Render-Svg -Browser $browser -SvgPath $Svg -OutPng $large -Size 1024
        $probe = [System.Drawing.Bitmap]::FromFile($large)
        try {
            $corner = $probe.GetPixel(0, 0)
            if ($corner.A -ne 0) { throw "the 1024 px render is opaque at the corner (alpha $($corner.A))" }
        }
        finally { $probe.Dispose() }

        Push-Location -LiteralPath $Root
        try {
            & pnpm exec tauri icon $large --output $Icons
            if ($LASTEXITCODE -ne 0) { throw "tauri icon exited $LASTEXITCODE" }
        }
        finally { Pop-Location }

        Remove-PlatformExtras $Icons

        $generated = Get-IcoEntries $Ico
        $sizes = [System.Collections.Generic.List[int]]::new()
        foreach ($entry in $generated) {
            if (-not $sizes.Contains($entry.Width)) { $sizes.Add($entry.Width) }
        }
        foreach ($size in $RequiredSizes) {
            if (-not $sizes.Contains($size)) { $sizes.Add($size) }
        }
        $sizes.Sort()

        $pngs = @{}
        foreach ($size in $sizes) {
            $out = Join-Path $tmp "icon-$size.png"
            if ($SmallSizes -contains $size) {
                Render-Svg -Browser $browser -SvgPath $SvgSmall -OutPng $out -Size $size
                $pngs[$size] = [System.IO.File]::ReadAllBytes($out)
                continue
            }
            $entry = $generated | Where-Object { $_.Width -eq $size } | Select-Object -First 1
            if ($entry -and $entry.Png) {
                $bytes = [System.IO.File]::ReadAllBytes($Ico)
                $slice = New-Object byte[] $entry.Bytes
                [Array]::Copy($bytes, $entry.Offset, $slice, 0, $entry.Bytes)
                $pngs[$size] = $slice
            }
            else {
                Render-Svg -Browser $browser -SvgPath $Svg -OutPng $out -Size $size
                $pngs[$size] = [System.IO.File]::ReadAllBytes($out)
            }
        }
        Write-Ico $Ico ([int[]]$sizes.ToArray()) $pngs
    }
    finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Test-Icon $Ico
