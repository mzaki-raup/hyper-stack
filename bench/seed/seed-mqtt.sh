#!/usr/bin/env bash
# MQTT (Mosquitto, anonymous): retained messages under home/, factory/, fleet/.
SEED_NAME=mqtt
source "$(dirname "$0")/lib.sh"
running mqtt || { log "not running - skipped"; exit 0; }
wait_healthy mqtt

pub() { dc exec -T mqtt mosquitto_pub -r -q 1 -t "$1" -m "$2"; }
pub home/kitchen/temperature 21.5
pub home/kitchen/humidity 48
pub home/garage/door closed
pub factory/line1/status '{"state":"running","rpm":1200}'
pub factory/line2/status '{"state":"fault","code":"E42"}'
pub fleet/truck-7/gps '3.139,101.687'
log "retained: $(dc exec -T mqtt sh -c "mosquitto_sub -t '#' --retained-only -W 2 -v 2>/dev/null | cut -d' ' -f1 | tr '\n' ' '" || true)"
