#!/usr/bin/env bash
# PuppetDB fake: nothing to load (its answers are files under config/puppetdb);
# this only checks it answers.
SEED_NAME=puppetdb
source "$(dirname "$0")/lib.sh"
running puppetdb-fake || { log "not running - skipped"; exit 0; }
wait_healthy puppetdb-fake
log "version $(curl -s "http://$H:8082/pdb/meta/v1/version"), nodes: $(curl -s "http://$H:8082/pdb/query/v4/nodes" | grep -o '"certname":"[^"]*"' | cut -d'"' -f4 | tr '\n' ' ')"
