#!/bin/sh
set -eu
# Preserve approvals during a package upgrade. A new MinerDesk version requires
# explicit approval again, but may still need to revoke the previous record.
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
    rmdir /usr/lib/minerdesk /var/lib/minerdesk 2>/dev/null || true
    ;;
esac
