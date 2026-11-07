#!/usr/bin/env bash
# CouchDB: the system databases, a second admin (dev), and `notes` (3 docs)
# and `orders` (2 docs). Each doc is updated in place on a re-run.
SEED_NAME=couch
source "$(dirname "$0")/lib.sh"
running couch || { log "not running - skipped"; exit 0; }
wait_healthy couch

C="http://$H:5984"
A=(-s -u "$COUCHDB_USER:$COUCHDB_PASSWORD")

for db in _users _replicator notes orders; do
  curl "${A[@]}" -o /dev/null -X PUT "$C/$db"   # 412 when it exists: fine
done
curl "${A[@]}" -o /dev/null -X PUT "$C/_node/_local/_config/admins/dev" -d "\"$COUCH_DEV_PASSWORD\""

for dir in "$DATA_DIR"/couch/*/; do
  db=$(basename "$dir")
  for f in "$dir"*.json; do
    id=$(basename "$f" .json)
    rev=$(curl "${A[@]}" -I "$C/$db/$id" | tr -d '\r' | sed -n 's/^[Ee][Tt]ag: *"\(.*\)"/\1/p')
    url="$C/$db/$id"
    [ -n "$rev" ] && url="$url?rev=$rev"
    curl "${A[@]}" -o /dev/null -f -X PUT -H 'Content-Type: application/json' "$url" --data-binary @"$f"
  done
done
log "notes=$(curl "${A[@]}" "$C/notes" | sed -n 's/.*"doc_count":\([0-9]*\).*/\1/p') orders=$(curl "${A[@]}" "$C/orders" | sed -n 's/.*"doc_count":\([0-9]*\).*/\1/p'); admins: $COUCHDB_USER, dev"
