#!/usr/bin/env bash
# Kafka (plain, 9092): topics orders.created (3 partitions, 12 keyed orders),
# payments and billing; consumer group `billing` has read 4 of orders.created
# and lags 8 (the lag scenario).
#
# Kafka SASL (9094): SCRAM-SHA-256 and SCRAM-SHA-512 credentials for the dev
# user, topic `orders` (3 partitions, six keyed JSON orders: four paid, two
# pending), and group `billing` that has read 2 and lags 4.
# Messages are produced only into an empty topic, so a re-run adds nothing.
SEED_NAME=kafka
source "$(dirname "$0")/lib.sh"

BIN=/opt/kafka/bin

# offsets <service> <bootstrap> <topic> [extra args]: total end offset of a topic
offsets() {
  local svc=$1 bs=$2 topic=$3
  shift 3
  dc exec -T "$svc" $BIN/kafka-get-offsets.sh --bootstrap-server "$bs" --topic "$topic" "$@" 2>/dev/null |
    awk -F: '{ s += $3 } END { print s + 0 }'
}

if running kafka; then
  wait_healthy kafka
  BS=127.0.0.1:9092
  for spec in orders.created:3 payments:1 billing:1; do
    dc exec -T kafka $BIN/kafka-topics.sh --bootstrap-server $BS --create --if-not-exists \
      --topic "${spec%%:*}" --partitions "${spec##*:}" --replication-factor 1 >/dev/null
  done
  for t in orders.created payments billing; do
    if [ "$(offsets kafka $BS "$t")" = 0 ]; then
      dc exec -T kafka $BIN/kafka-console-producer.sh --bootstrap-server $BS --topic "$t" \
        --property parse.key=true --property 'key.separator=|' < "$DATA_DIR/kafka/$t.txt"
      if [ "$t" = orders.created ]; then
        dc exec -T kafka $BIN/kafka-console-consumer.sh --bootstrap-server $BS --topic "$t" \
          --group billing --from-beginning --max-messages 4 --timeout-ms 20000 >/dev/null 2>&1 || true
      fi
    fi
  done
  log "plain: orders.created=$(offsets kafka $BS orders.created) payments=$(offsets kafka $BS payments) billing=$(offsets kafka $BS billing) messages"
else
  log "kafka not running - skipped"
fi

if running kafka-sasl; then
  wait_healthy kafka-sasl
  BS=127.0.0.1:9094
  CFG=(--command-config /tmp/admin.properties)
  # One mechanism per call: two in one request fail (docs/GOTCHAS.md).
  for mech in SCRAM-SHA-256 SCRAM-SHA-512; do
    dc exec -T kafka-sasl $BIN/kafka-configs.sh --bootstrap-server $BS "${CFG[@]}" --alter \
      --add-config "$mech=[password=$KAFKA_SASL_PASSWORD]" --entity-type users --entity-name "$KAFKA_SASL_USER" >/dev/null
  done
  dc exec -T kafka-sasl $BIN/kafka-topics.sh --bootstrap-server $BS "${CFG[@]}" --create --if-not-exists \
    --topic orders --partitions 3 --replication-factor 1 >/dev/null
  if [ "$(offsets kafka-sasl $BS orders "${CFG[@]}")" = 0 ]; then
    dc exec -T kafka-sasl $BIN/kafka-console-producer.sh --bootstrap-server $BS --producer.config /tmp/admin.properties \
      --topic orders --property parse.key=true --property 'key.separator=|' < "$DATA_DIR/kafka/sasl-orders.txt"
    dc exec -T kafka-sasl $BIN/kafka-console-consumer.sh --bootstrap-server $BS --consumer.config /tmp/admin.properties \
      --topic orders --group billing --from-beginning --max-messages 2 --timeout-ms 20000 >/dev/null 2>&1 || true
  fi
  # Prove the dev user's SCRAM login works.
  dc exec -T -e U="$KAFKA_SASL_USER" -e P="$KAFKA_SASL_PASSWORD" kafka-sasl sh -c \
    'printf "%s\n" security.protocol=SASL_PLAINTEXT sasl.mechanism=SCRAM-SHA-256 "sasl.jaas.config=org.apache.kafka.common.security.scram.ScramLoginModule required username=\"$U\" password=\"$P\";" > /tmp/dev-scram.properties'
  topics=$(dc exec -T kafka-sasl $BIN/kafka-topics.sh --bootstrap-server $BS --command-config /tmp/dev-scram.properties --list 2>/dev/null | tr '\n' ' ')
  log "sasl: orders=$(offsets kafka-sasl $BS orders "${CFG[@]}") messages; $KAFKA_SASL_USER via SCRAM-SHA-256 sees: $topics"
else
  log "kafka-sasl not running - skipped"
fi
