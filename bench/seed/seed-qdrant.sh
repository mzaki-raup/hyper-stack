#!/usr/bin/env bash
# Qdrant: `articles` (5 points, dim 4, cosine) and `faces` (1 point, dim 3, euclid).
SEED_NAME=qdrant
source "$(dirname "$0")/lib.sh"
running qdrant || { log "not running - skipped"; exit 0; }
wait_healthy qdrant

Q="http://$H:${QDRANT_PORT:-6333}"
for c in articles faces; do
  if [ "$(curl -s -o /dev/null -w '%{http_code}' "$Q/collections/$c")" != 200 ]; then
    curl -sf -o /dev/null -X PUT "$Q/collections/$c" -H 'Content-Type: application/json' \
      --data-binary @"$DATA_DIR/qdrant/$c.collection.json"
  fi
  curl -sf -o /dev/null -X PUT "$Q/collections/$c/points?wait=true" -H 'Content-Type: application/json' \
    --data-binary @"$DATA_DIR/qdrant/$c.points.json"
done
log "$(curl -s "$Q/collections/articles" | grep -o '"points_count":[0-9]*' | head -1) in articles, $(curl -s "$Q/collections/faces" | grep -o '"points_count":[0-9]*' | head -1) in faces"
