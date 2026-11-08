#!/usr/bin/env bash
# WebDAV: /documents (plan.md, 2026/standup.txt), /photos/holiday.jpg,
# /hs-check/ and hello.txt.
SEED_NAME=webdav
source "$(dirname "$0")/lib.sh"
running webdav || { log "not running - skipped"; exit 0; }
wait_healthy webdav

W="http://$H:${WEBDAV_PORT:-8086}"
A=(-s -u "$WEBDAV_USER:$WEBDAV_PASSWORD")
root="$DATA_DIR/webdav"
while IFS= read -r d; do
  rel=${d#"$root"}
  [ -n "$rel" ] && curl "${A[@]}" -o /dev/null -X MKCOL "$W$rel/"   # 405 when it exists: fine
done < <(find "$root" -type d | sort)
while IFS= read -r f; do
  rel=${f#"$root"}
  case "$rel" in */.keep) continue ;; esac
  curl "${A[@]}" -f -o /dev/null -T "$f" "$W$rel"
done < <(find "$root" -type f | sort)
log "root holds: $(curl "${A[@]}" -X PROPFIND -H 'Depth: 1' "$W/" | grep -oE '<D:href>[^<]*' | sed 's/<D:href>//' | tr '\n' ' ')"
