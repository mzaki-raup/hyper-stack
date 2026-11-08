#!/usr/bin/env bash
# Redmine: default trackers/statuses/priorities, REST API on, the admin
# password, project `demo` with three issues (seed/data/redmine/seed.rb).
SEED_NAME=redmine
source "$(dirname "$0")/lib.sh"
running redmine || { log "not running - skipped"; exit 0; }
wait_healthy redmine

out=$(dc exec -T -e HS_ADMIN_PASSWORD="$REDMINE_ADMIN_PASSWORD" redmine \
  bundle exec rails runner /hs-seed/seed.rb 2>&1 | tr -d '\r')
key=$(printf '%s\n' "$out" | sed -n 's/^REDMINE_API_KEY=//p' | tail -1)
if [ -z "$key" ]; then
  printf '%s\n' "$out" >&2
  warn "seed.rb did not finish"
  exit 1
fi
save_state redmine-api-key "$key"
log "project demo: $(curl -s -H "X-Redmine-API-Key: $key" "http://$H:${REDMINE_PORT:-3010}/issues.json?project_id=demo&status_id=*" | grep -o '"total_count":[0-9]*') issues; API key in .state/redmine-api-key"
