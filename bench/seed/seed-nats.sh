#!/usr/bin/env bash
# NATS JetStream: stream ORDERS on orders.> with nine messages (eight
# orders.created, one orders.paid) and the durable pull consumer `shipper`
# that has delivered none of them. Messages go in only when the stream is empty.
SEED_NAME=nats
source "$(dirname "$0")/lib.sh"
running nats || { log "not running - skipped"; exit 0; }

dc run --rm -T nats-box sh -s <<'EOF'
set -e
N="nats -s nats://nats:4222"
i=0
until $N rtt >/dev/null 2>&1; do
  i=$((i + 1)); [ $i -lt 30 ] || { echo "nats did not answer" >&2; exit 1; }; sleep 2
done
$N stream info ORDERS >/dev/null 2>&1 ||
  $N stream add ORDERS --subjects 'orders.>' --storage file --retention limits --discard old \
    --replicas 1 --defaults >/dev/null
if [ "$($N stream info ORDERS -j | grep -o '"messages": *[0-9]*' | head -1 | grep -o '[0-9]*$')" = 0 ]; then
  for i in 1 2 3 4 5 6 7 8; do $N pub orders.created "{\"order\":$i}" >/dev/null 2>&1; done
  $N pub orders.paid '{"order":9,"paid":true}' >/dev/null 2>&1
fi
$N consumer info ORDERS shipper >/dev/null 2>&1 ||
  $N consumer add ORDERS shipper --pull --deliver all --ack explicit --replay instant --defaults >/dev/null
echo "ORDERS: $($N stream info ORDERS -j | grep -o '"messages": *[0-9]*' | head -1) consumer shipper"
EOF
log "done"
