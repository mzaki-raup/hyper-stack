#!/usr/bin/env bash
# RabbitMQ: exchange `shop` (topic), queues demo / orders.created /
# emails.outbound, binding shop -> orders.created on order.*, and three ready
# messages in `demo`. A re-run empties `demo` first, so it holds three again.
SEED_NAME=rabbitmq
source "$(dirname "$0")/lib.sh"
running rabbitmq || { log "not running - skipped"; exit 0; }
wait_healthy rabbitmq

M="http://$H:${RABBITMQ_MGMT_PORT:-15672}/api"
A=(-s -u "$RABBITMQ_USER:$RABBITMQ_PASSWORD")
wait_http "$M/overview" 30 "${A[@]}" || true   # 401 without the login
curl "${A[@]}" -f -o /dev/null -X POST "$M/definitions" -H 'Content-Type: application/json' \
  --data-binary @"$DATA_DIR/rabbitmq/definitions.json"
curl "${A[@]}" -o /dev/null -X DELETE "$M/queues/%2F/demo/contents"
for n in 1 2 3; do
  curl "${A[@]}" -f -o /dev/null -X POST "$M/exchanges/%2F/amq.default/publish" -H 'Content-Type: application/json' \
    -d "{\"properties\":{\"headers\":{\"source\":\"bench\"}},\"routing_key\":\"demo\",\"payload\":\"{\\\"order\\\":$n,\\\"status\\\":\\\"new\\\"}\",\"payload_encoding\":\"string\"}"
done
sleep 2
log "queues: $(curl "${A[@]}" "$M/queues/%2F?columns=name,messages" | grep -oE '"(messages|name)":("[^"]*"|[0-9]+)' | paste -d' ' - - | tr '\n' ' ')"
