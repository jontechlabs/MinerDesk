#!/bin/sh
set -eu
# Preserve authorization state during package upgrades. Remove the app-owned
# records only when the package is actually removed or purged.
case "${1:-}" in
  remove|purge)
    auth=/var/lib/minerdesk/root-authorization
    # Never follow a redirected storage path during recursive removal.
    if [ -d "$auth" ] && [ "$(readlink -f -- "$auth")" = "$auth" ] \
       && [ "$(stat -c %u -- "$auth")" = 0 ] && [ "$(stat -c %a -- "$auth")" = 700 ]; then
      find "$auth" -type f -name grant.json -delete
      rm -rf -- "$auth"
    fi
    rm -f -- /usr/share/polkit-1/actions/com.minerdesk.approved-miner.policy
    rm -f -- /usr/lib/minerdesk/minerdesk-approved-helper
    # These grants cover only the packaged update helper. Keep them intact on
    # upgrade; remove only numeric UID policy names on actual uninstall.
    for policy in /etc/sudoers.d/minerdesk-updates-*; do
      uid=${policy##*/minerdesk-updates-}
      case "$uid" in ''|*[!0-9]*) continue ;; esac
      if [ -f "$policy" ] && [ ! -L "$policy" ] && [ "$(stat -c %u -- "$policy")" = 0 ]; then
        rm -f -- "$policy"
      fi
    done
    updates=/var/lib/minerdesk/updates
    if [ -d "$updates" ] && [ "$(readlink -f -- "$updates")" = "$updates" ] \
       && [ "$(stat -c %u -- "$updates")" = 0 ]; then
      rm -rf -- "$updates"
    fi
    rmdir /usr/lib/minerdesk /var/lib/minerdesk 2>/dev/null || true
    ;;
esac
