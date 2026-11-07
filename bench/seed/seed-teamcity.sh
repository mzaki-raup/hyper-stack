#!/usr/bin/env bash
# TeamCity: project Bench with build configuration "Hello Build" (Bench_Hello,
# parameter GREETING, one command-line step that prints a line every 5 s), and
# the hs-agent authorised. The first-start wizard (database, licence, admin)
# has no API, so it is done by hand once - see README.md.
SEED_NAME=teamcity
source "$(dirname "$0")/lib.sh"
running teamcity || { log "not running - skipped"; exit 0; }

T="http://$H:8111"
A=(-s -u "$TEAMCITY_ADMIN_USER:$TEAMCITY_ADMIN_PASSWORD")
if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$T/app/rest/server")" != 200 ]; then
  warn "cannot log in as $TEAMCITY_ADMIN_USER yet. Finish the first-start wizard at $T"
  warn "(Proceed, internal HSQLDB, accept the licence, create admin $TEAMCITY_ADMIN_USER), then re-run."
  exit 0
fi

csrf=$(curl "${A[@]}" "$T/authenticationTest.html?csrf")
J=("${A[@]}" -H "X-TC-CSRF-Token: $csrf" -H 'Content-Type: application/json' -H 'Accept: application/json')

if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$T/app/rest/projects/id:Bench")" != 200 ]; then
  curl "${J[@]}" -f -o /dev/null -X POST "$T/app/rest/projects" \
    -d '{"parentProject":{"locator":"id:_Root"},"name":"Bench","id":"Bench"}'
fi
if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$T/app/rest/buildTypes/id:Bench_Hello")" != 200 ]; then
  curl "${J[@]}" -f -o /dev/null -X POST "$T/app/rest/projects/id:Bench/buildTypes" \
    -d '{"id":"Bench_Hello","name":"Hello Build"}'
  curl "${J[@]}" -f -o /dev/null -X POST "$T/app/rest/buildTypes/id:Bench_Hello/steps" -d '{
    "name": "Say hello", "type": "simpleRunner",
    "properties": {"property": [
      {"name": "script.content", "value": "echo Hello from the bench TeamCity\nfor i in $(seq 1 60); do echo step $i; sleep 5; done\necho done"},
      {"name": "use.custom.script", "value": "true"}]}}'
  curl "${A[@]}" -H "X-TC-CSRF-Token: $csrf" -H 'Content-Type: text/plain' -f -o /dev/null -X PUT \
    "$T/app/rest/buildTypes/id:Bench_Hello/parameters/GREETING" -d 'hello'
fi

# The agent shows up unauthorised after its first connection.
for _ in $(seq 1 24); do
  if curl "${A[@]}" -H 'Accept: application/json' "$T/app/rest/agents?locator=authorized:any" | grep -q '"name":"hs-agent"'; then
    curl "${A[@]}" -H "X-TC-CSRF-Token: $csrf" -H 'Content-Type: text/plain' -o /dev/null -X PUT \
      "$T/app/rest/agents/name:hs-agent/authorized" -d 'true'
    break
  fi
  sleep 5
done
log "project Bench / Hello Build ready; agent: $(curl "${A[@]}" -H 'Accept: application/json' "$T/app/rest/agents?locator=authorized:true" | grep -o '"name":"[^"]*"' | tr '\n' ' ')"
