#!/bin/sh
# Debug-only stand-in for the tailscale CLI (SKYELAB_TAILSCALE_BIN): a running tailnet where this machine has two
# online devices of its own, so live checks never list or reach real machines. Only `status` answers.
[ "$1" = status ] || { echo "fake-tailscale: only status is faked" >&2; exit 1; }
cat <<'JSON'
{
  "BackendState": "Running",
  "Self": {"ID": "nSelfFAKE", "DNSName": "this-mac.example.ts.net.", "HostName": "this-mac", "UserID": 1001,
           "TailscaleIPs": ["100.64.0.1"], "Online": true},
  "Peer": {
    "nodekey:1": {"ID": "nLaptopFAKE", "DNSName": "studio-laptop.tail1234.ts.net.", "HostName": "studio-laptop",
                  "UserID": 1001, "TailscaleIPs": ["100.64.0.2"], "Online": true},
    "nodekey:2": {"ID": "nBuildFAKE", "DNSName": "build-server-west.tail1234.ts.net.", "HostName": "build-server-west",
                  "UserID": 1001, "TailscaleIPs": ["100.64.0.3"], "Online": true},
    "nodekey:3": {"ID": "nOffFAKE", "DNSName": "old-imac.tail1234.ts.net.", "HostName": "old-imac",
                  "UserID": 1001, "TailscaleIPs": ["100.64.0.4"], "Online": false}
  }
}
JSON
