# [DEBUG-perf9] Windows frame-time loop. Temporary — delete with the lag bug.
#
#   .\scripts\perf-loop.ps1 -Log C:\path\to\flight.bbl
#
# Builds release, then runs one arm per present mode / backend. Each arm loads
# the log, drives synthetic hover + zoom for -Frames frames, and writes a
# report. Exit code 1 from an arm means a frame breached -SpikeMs: red.

param(
    [Parameter(Mandatory = $true)][string]$Log,
    [int]$Frames = 900,
    [int]$SpikeMs = 100,
    [string]$OutDir = "perf-out"
)

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$exe = ".\target\release\blackbird.exe"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$arms = [ordered]@{
    "novsync"     = @{}                                                        # today's default
    "vsync"       = @{ BLACKBIRD_PRESENT = "vsync" }
    "vsync-lat1"  = @{ BLACKBIRD_PRESENT = "vsync"; BLACKBIRD_LATENCY = "1" }
    "novsync-vk"  = @{ WGPU_BACKEND = "vulkan" }
    "vsync-vk"    = @{ BLACKBIRD_PRESENT = "vsync"; WGPU_BACKEND = "vulkan" }
}

# Per-arm overrides are removed again after the run, so arms cannot leak into
# each other.
$keys = @("BLACKBIRD_PRESENT", "BLACKBIRD_LATENCY", "WGPU_BACKEND")

foreach ($arm in $arms.Keys) {
    foreach ($k in $keys) { Remove-Item "env:$k" -ErrorAction SilentlyContinue }
    foreach ($k in $arms[$arm].Keys) { Set-Item "env:$k" $arms[$arm][$k] }

    $env:BLACKBIRD_PERF = "1"
    $env:BLACKBIRD_PERF_OPEN = $Log
    $env:BLACKBIRD_PERF_FRAMES = "$Frames"
    $env:BLACKBIRD_PERF_SPIKE_MS = "$SpikeMs"
    $env:BLACKBIRD_PERF_HOVER = "1"
    $env:BLACKBIRD_PERF_ZOOM = "1"
    $env:BLACKBIRD_PERF_OUT = "$OutDir\$arm.log"
    $env:BLACKBIRD_LOG_FILE = "$OutDir\$arm.trace"
    $env:RUST_LOG = "blackbird=debug"

    Write-Host "=== arm: $arm ===" -ForegroundColor Cyan
    $p = Start-Process -FilePath $exe -PassThru -Wait
    Get-Content "$OutDir\$arm.log" -ErrorAction SilentlyContinue
    Write-Host "exit=$($p.ExitCode)"
}

Write-Host "`nreports in $OutDir\ — paste them back" -ForegroundColor Green
