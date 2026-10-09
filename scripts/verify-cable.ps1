#!/usr/bin/env pwsh
#
# Manual verification for the first engineering milestone (SRS §112).
#
# Windows counterpart to scripts/verify-cable.sh. Run this on the Windows
# machine and that script on the Linux/macOS machine, with the cable
# connected, then compare the two outputs.
#
# It answers the one question the whole product depends on:
#
#   does this cable, on this hardware, on this OS, actually give us a
#   network path between these two machines?
#
# Read-only: it changes no network configuration and needs no administrator
# rights.

$ErrorActionPreference = 'Continue'

function Write-Rule { Write-Host ('-' * 62) -ForegroundColor DarkGray }
function Write-Head { param($Text) Write-Host ''; Write-Host $Text -ForegroundColor White }

Write-Host ''
Write-Host 'FastTransfer - cable verification (Windows)' -ForegroundColor White
Write-Rule

# --- Platform ---------------------------------------------------------------

$os = [System.Environment]::OSVersion.Version
Write-Host ("Platform:      Windows {0} ({1})" -f $os, $env:PROCESSOR_ARCHITECTURE)
Write-Host ("Hostname:      {0}" -f $env:COMPUTERNAME)
Write-Rule

# --- Step 1: is a Thunderbolt controller present? ---------------------------
#
# A controller existing is necessary but not sufficient. Plenty of machines
# have a Thunderbolt controller and still expose no network path over it.
#
# Matching is on device-bound evidence — hardware IDs and the bound driver
# service — and not only on the display name. The name alone is what this step
# originally checked, and it misses a controller whose FriendlyName does not
# happen to say "Thunderbolt".
#
# A trap for anyone extending this: PCI class code 0x0880 (`CC_0880`) is NOT a
# Thunderbolt marker. It is the generic PCI class 08/80 compatible ID, and
# unrelated System devices carry it — the Intel GNA Scoring Accelerator on the
# reference machine, for one. Intel's Thunderbolt INF does list `...&CC_0880`
# entries, but always paired with a specific device ID, so the class code on
# its own proves nothing. `CC_0C0340`, the USB4 host interface, is specific.

Write-Head '1. Thunderbolt / USB4 controller presence'

$tbDevices = @()
try {
    $tbDevices = @(Get-CimInstance Win32_PnPEntity -ErrorAction Stop | Where-Object {
            $_.Service -match '^Usb4(HostRouter|P2PNetAdapter|DeviceRouter)$' -or
            ($_.HardwareID -join ' ') -match 'CC_0C0340|USB4\\' -or
            ($_.CompatibleID -join ' ') -match 'CC_0C0340|USB4\\' -or
            $_.Name -match 'Thunderbolt|USB4'
        })
}
catch {
    Write-Host '  Win32_PnPEntity unavailable; cannot enumerate controllers.' -ForegroundColor Yellow
}

$TB_FOUND = 0
if ($tbDevices.Count -gt 0) {
    $tbDevices | Format-Table Status, PNPClass, Name, Service -AutoSize |
        Out-String -Width 200 | Write-Host
    $TB_FOUND = 1
}
else {
    Write-Host '  No Thunderbolt or USB4 controller enumerated.'
}
Write-Rule

# --- Step 2: is there a NETWORK interface over it? --------------------------
#
# This is the step that matters. A Thunderbolt controller that enumerates but
# exposes no network interface is exactly the "USB data-only" case the product
# must report honestly (SRS §4.2).

Write-Head '2. Network interface from that cable'

$adapters = @()
try { $adapters = @(Get-NetAdapter -ErrorAction Stop) } catch { }

$FOUND_NET = 0

if ($adapters.Count -eq 0) {
    Write-Host '  Get-NetAdapter unavailable.' -ForegroundColor Yellow
}
else {
    Write-Host '  All adapters:'
    $adapters | Sort-Object Name |
        Format-Table Name, InterfaceDescription, Status, LinkSpeed, MediaType, PhysicalMediaType -AutoSize |
        Out-String -Width 220 | Write-Host

    $tbNet = @($adapters | Where-Object {
            $_.InterfaceDescription -match 'Thunderbolt' -or $_.Name -match 'Thunderbolt'
        })

    if ($tbNet.Count -gt 0) {
        Write-Host '  Thunderbolt-backed network interface found:' -ForegroundColor Green
        $tbNet | Format-Table Name, InterfaceDescription, Status, LinkSpeed, MacAddress -AutoSize |
            Out-String -Width 220 | Write-Host
        $FOUND_NET = 1
    }
    else {
        Write-Host '  No Thunderbolt-backed network interface.' -ForegroundColor Yellow
        Write-Host '  (The controller can be present with no networking driver bound to it.)'
    }
}
Write-Rule

# --- Step 3: link-local addressing ------------------------------------------
#
# On a bare cable there is no DHCP server, so both machines need 169.254.x.x.
# Windows self-assigns APIPA automatically once the link is up, so its absence
# means the link never came up - not that it is misconfigured.

Write-Head '3. Link-local addressing (needed for a router-free transfer)'

# Windows parks APIPA (169.254.x.x) on *every* adapter with no DHCP lease,
# including adapters that are disconnected - so the bare presence of a
# link-local address proves nothing. `PrefixOrigin: WellKnown` is the marker
# for that placeholder. What matters is a link-local address on an adapter
# whose link is actually Up, so the adapter status is joined in here.
try {
    $ll4 = @(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction Stop |
            Where-Object { $_.IPAddress -like '169.254.*' })
}
catch { $ll4 = @() }

if ($ll4.Count -eq 0) {
    Write-Host '  No 169.254.x.x address on any adapter.' -ForegroundColor Yellow
}
else {
    $rows = foreach ($a in $ll4) {
        $status = 'unavailable'
        try {
            $status = (Get-NetAdapter -InterfaceIndex $a.InterfaceIndex -ErrorAction Stop).Status
        }
        catch { }
        [pscustomobject]@{
            Interface = $a.InterfaceAlias
            IPAddress = $a.IPAddress
            Origin    = $a.PrefixOrigin
            Adapter   = $status
        }
    }
    $rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host

    if (@($rows | Where-Object { $_.Adapter -eq 'Up' }).Count -gt 0) {
        Write-Host '  Link-local address present on a live adapter.' -ForegroundColor Green
    }
    else {
        Write-Host '  Every link-local address sits on a disconnected adapter.' -ForegroundColor Yellow
        Write-Host '  Windows parks APIPA on idle adapters, so none of these mean a live link.'
    }
}

try {
    $ll6 = @(Get-NetIPAddress -AddressFamily IPv6 -ErrorAction Stop |
            Where-Object { $_.IPAddress -like 'fe80::*' })
}
catch { $ll6 = @() }

if ($ll6.Count -gt 0) {
    Write-Host ("  IPv6 link-local on {0} interface(s) (normal on every up adapter)." -f $ll6.Count)
}
Write-Rule

# --- Step 4: reachability ---------------------------------------------------

Write-Head '4. Reaching the other computer'
Write-Host '  Run this on both machines and compare the addresses.'
Write-Host ''
Write-Host '    ping <other-ip>'
Write-Host ''
Write-Host '  Then, once FastTransfer is running, the peer should appear in the'
Write-Host '  Nearby Devices list without any manual address entry.'
Write-Rule

# --- Verdict ----------------------------------------------------------------

Write-Head 'Verdict'

if ($FOUND_NET -eq 1) {
    Write-Host '  A network path over the cable was found.' -ForegroundColor Green
    Write-Host '  FastTransfer should classify this link as THUNDERBOLT or USB4_NETWORK.'
}
elseif ($TB_FOUND -eq 1) {
    Write-Host '  Thunderbolt hardware is present but exposes no network interface.' -ForegroundColor Red
    Write-Host '  FastTransfer must classify this as USB_DATA_ONLY and fall back to'
    Write-Host '  Ethernet or Wi-Fi. If it claims Thunderbolt, that is a bug - SRS FR-LINK-003.'
}
else {
    Write-Host '  No Thunderbolt networking path detected.' -ForegroundColor Yellow
    Write-Host '  Either the cable is not Thunderbolt, the controller is absent, or the'
    Write-Host '  OS driver stack is not exposing the network path. This is the most'
    Write-Host '  common outcome on consumer hardware - see SRS 4.1.'
}

Write-Host ''
