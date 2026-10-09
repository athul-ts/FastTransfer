---
description: Run the read-only cable verification and interpret the result against the link classification rules
allowed-tools: Bash(./scripts/verify-cable.sh), Bash(bash scripts/verify-cable.sh), Read
---

Run the cable verification script and interpret its output.

1. Run `./scripts/verify-cable.sh` (fall back to `bash scripts/verify-cable.sh` if it
   is not executable). The script is read-only — it changes no network configuration.
   If it needs to be made executable, tell the user the command rather than running it.

2. Report which of these four cases the output describes. Use the SRS §4 vocabulary
   exactly — the whole point of this plugin is that the classification is not
   guessed at:

   | Case | Evidence | What the app must report |
   | --- | --- | --- |
   | A | Thunderbolt device enumerated **and** a network interface over it with an IP address | `THUNDERBOLT` |
   | B | Thunderbolt device enumerated, **no** network interface | `USB_DATA_ONLY` |
   | C | No Thunderbolt controller, but a USB-C data path exists | `USB_DATA_ONLY` |
   | D | Cable carries power only | `CHARGE_ONLY` |

3. If the result is B or C, say plainly that the link cannot carry the transfer and
   what the fallback is (Ethernet or Wi-Fi). Do not soften this — a user who is told
   "Thunderbolt detected" when there is no IP path will spend an hour debugging the
   wrong thing. This is SRS FR-LINK-003 and it is a tested invariant
   (`src-tauri/src/link.rs`).

4. Remind the user that verification requires running the script on **both**
   machines and comparing the link-local addresses.
