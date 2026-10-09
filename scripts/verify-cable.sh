#!/usr/bin/env bash
#
# Manual verification for the first engineering milestone (SRS §112).
#
# Run this on BOTH computers with the cable connected, and compare the output.
# It answers the one question the whole product depends on:
#
#   does this cable, on this hardware, on this OS, actually give us a
#   network path between these two machines?
#
# This script is read-only. It changes no network configuration.

set -uo pipefail

GREEN=$'\033[0;32m'
RED=$'\033[0;31m'
YELLOW=$'\033[0;33m'
BOLD=$'\033[1m'
NC=$'\033[0m'

hr() { printf '%s\n' "────────────────────────────────────────────────────────────"; }

printf '\n%sFastTransfer — cable verification%s\n' "$BOLD" "$NC"
hr

# --- Platform ---------------------------------------------------------------

OS=$(uname -s)
printf 'Platform:      %s (%s)\n' "$OS" "$(uname -m)"
printf 'Hostname:      %s\n' "$(hostname)"
hr

# --- Step 1: is a Thunderbolt device attached? ------------------------------

printf '\n%s1. Thunderbolt device presence%s\n' "$BOLD" "$NC"

TB_FOUND=0
case "$OS" in
  Linux)
    if [ -d /sys/bus/thunderbolt/devices ]; then
      for dev in /sys/bus/thunderbolt/devices/*; do
        [ -e "$dev" ] || continue
        name=$(cat "$dev/device_name" 2>/dev/null || echo "unknown")
        printf '  %s -> %s\n' "$(basename "$dev")" "$name"
        TB_FOUND=1
      done
      [ "$TB_FOUND" -eq 0 ] && printf '  No Thunderbolt devices enumerated.\n'
    else
      printf '  No /sys/bus/thunderbolt — this kernel is not exposing Thunderbolt.\n'
    fi
    ;;

  Darwin)
    if system_profiler SPThunderboltDataType 2>/dev/null | grep -q "Thunderbolt"; then
      system_profiler SPThunderboltDataType 2>/dev/null \
        | grep -E "Status|Device Name|Link Status" | sed 's/^/  /'
      TB_FOUND=1
    else
      printf '  system_profiler reports no Thunderbolt hardware.\n'
    fi
    ;;

  MINGW*|MSYS*|CYGWIN*)
    printf '  Run this in PowerShell instead:\n'
    printf '    Get-PnpDevice -Class Net | Where-Object { $_.FriendlyName -match "Thunderbolt" }\n'
    printf '    Get-NetAdapter | Format-Table Name,InterfaceDescription,LinkSpeed,Status\n'
    ;;

  *)
    printf '  Unrecognised platform.\n'
    ;;
esac
hr

# --- Step 2: is there a NETWORK interface over it? --------------------------
#
# This is the step that matters. A Thunderbolt device that enumerates but
# exposes no network interface is exactly the "USB data-only" case the product
# must report honestly (SRS §4.2).

printf '\n%s2. Network interface from that cable%s\n' "$BOLD" "$NC"

FOUND_NET=0

case "$OS" in
  Linux)
    for iface in /sys/class/net/*; do
      name=$(basename "$iface")
      [ "$name" = "lo" ] && continue
      driver=$(basename "$(readlink -f "$iface/device/driver" 2>/dev/null)" 2>/dev/null || echo "?")
      carrier=$(cat "$iface/carrier" 2>/dev/null || echo "?")
      speed=$(cat "$iface/speed" 2>/dev/null || echo "?")

      if echo "$name $driver" | grep -qi "thunder"; then
        printf '  %s  driver=%s carrier=%s speed=%sMbps\n' "$name" "$driver" "$carrier" "$speed"
        FOUND_NET=1
      fi
    done

    printf '\n  All non-loopback interfaces:\n'
    ip -brief address 2>/dev/null | sed 's/^/    /' || ifconfig -a | sed 's/^/    /'
    ;;

  Darwin)
    if ifconfig bridge0 >/dev/null 2>&1; then
      printf '  bridge0 (Thunderbolt Bridge) present:\n'
      ifconfig bridge0 | sed 's/^/    /'
      FOUND_NET=1
    else
      printf '  No bridge0. The Thunderbolt Bridge is not configured.\n'
      printf '  Open System Settings > Network and add a Thunderbolt Bridge.\n'
    fi

    printf '\n  All interfaces:\n'
    ifconfig -a | grep -E "^[a-z0-9]+:" | sed 's/^/    /'
    ;;

  MINGW*|MSYS*|CYGWIN*)
    printf '  Use PowerShell:\n'
    printf '    Get-NetAdapter | Format-Table Name,InterfaceDescription,LinkSpeed,Status\n'
    ;;
esac
hr

# --- Step 3: link-local addressing ------------------------------------------
#
# On a bare cable there is no DHCP server, so both machines need 169.254.x.x.

printf '\n%s3. Link-local addressing (needed for a router-free transfer)%s\n' "$BOLD" "$NC"

case "$OS" in
  Linux)
    if ip -4 address 2>/dev/null | grep -q "169.254."; then
      printf '  %sLink-local IPv4 address present%s\n' "$GREEN" "$NC"
      ip -4 address | grep "169.254." | sed 's/^/    /'
    else
      printf '  %sNo 169.254.x.x address%s\n' "$YELLOW" "$NC"
      printf '  On a direct cable with no DHCP, one side must self-assign.\n'
      printf '  NetworkManager usually does this automatically once the link is up.\n'
    fi
    ;;
  Darwin)
    if ifconfig 2>/dev/null | grep -q "169.254."; then
      printf '  %sLink-local IPv4 address present%s\n' "$GREEN" "$NC"
      ifconfig | grep "169.254." | sed 's/^/    /'
    else
      printf '  %sNo 169.254.x.x address%s\n' "$YELLOW" "$NC"
    fi
    ;;
  *)
    printf '  Check with: ipconfig  (Windows) — look for an APIPA 169.254.x.x address.\n'
    ;;
esac
hr

# --- Step 4: reachability ---------------------------------------------------

printf '\n%s4. Reaching the other computer%s\n' "$BOLD" "$NC"
printf '  Run this on both machines and compare the addresses.\n\n'

case "$OS" in
  MINGW*|MSYS*|CYGWIN*) printf '    ping <other-ip>\n' ;;
  *) printf '    ping -c 3 <other-ip>\n' ;;
esac

printf '\n  Then, once FastTransfer is running, the peer should appear in the\n'
printf '  Nearby Devices list without any manual address entry.\n'
hr

# --- Verdict ----------------------------------------------------------------

printf '\n%sVerdict%s\n' "$BOLD" "$NC"

if [ "$FOUND_NET" -eq 1 ]; then
  printf '  %sA network path over the cable was found.%s\n' "$GREEN" "$NC"
  printf '  FastTransfer should classify this link as THUNDERBOLT or USB4_NETWORK.\n'
elif [ "$TB_FOUND" -eq 1 ]; then
  printf '  %sThunderbolt hardware is present but exposes no network interface.%s\n' "$RED" "$NC"
  printf '  FastTransfer must classify this as USB_DATA_ONLY and fall back to\n'
  printf '  Ethernet or Wi-Fi. If it claims Thunderbolt, that is a bug — SRS FR-LINK-003.\n'
else
  printf '  %sNo Thunderbolt networking path detected.%s\n' "$YELLOW" "$NC"
  printf '  Either the cable is not Thunderbolt, the controller is absent, or the\n'
  printf '  OS driver stack is not exposing the network path. This is the most\n'
  printf '  common outcome on consumer hardware — see SRS §4.1.\n'
fi

printf '\n'
