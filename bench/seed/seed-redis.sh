#!/usr/bin/env bash
# Redis (23 keys) and Valkey (11 keys): every type the Redis activity draws -
# strings, hashes, lists, sets, a sorted set, streams - in nested folders.
SEED_NAME=redis
source "$(dirname "$0")/lib.sh"

if running redis; then
  wait_healthy redis
  dc exec -T redis redis-cli < "$DATA_DIR/redis/redis.txt" >/dev/null
  log "redis: $(dc exec -T redis redis-cli dbsize) keys"
else
  log "redis not running - skipped"
fi

if running valkey; then
  wait_healthy valkey
  dc exec -T valkey valkey-cli < "$DATA_DIR/redis/valkey.txt" >/dev/null
  log "valkey: $(dc exec -T valkey valkey-cli dbsize) keys"
else
  log "valkey not running - skipped"
fi
