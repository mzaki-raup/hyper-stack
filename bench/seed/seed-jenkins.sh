#!/usr/bin/env bash
# Jenkins: the user, jobs and API token come from config/jenkins at start
# (init.groovy.d). This builds unit-tests once (green) and deploy-staging once
# with FAIL=true (red), the two states the Automation activity shows.
SEED_NAME=jenkins
source "$(dirname "$0")/lib.sh"
running jenkins || { log "not running - skipped"; exit 0; }
wait_healthy jenkins

token=$(dc exec -T jenkins cat /var/jenkins_home/hs-token | tr -d '\r\n')
save_state jenkins-token "$token"
J="http://$H:${JENKINS_PORT:-8091}"
A=(-s -u "$JENKINS_USER:$token")

never_built() { [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$J/job/$1/lastBuild/api/json")" = 404 ]; }
never_built unit-tests && curl "${A[@]}" -f -o /dev/null -X POST "$J/job/unit-tests/build"
never_built deploy-staging && curl "${A[@]}" -f -o /dev/null -X POST \
  "$J/job/deploy-staging/buildWithParameters?ENV=staging&FAIL=true&STEPS=5"

log "jobs: $(curl "${A[@]}" -g "$J/api/json?tree=jobs[name]" | grep -o '"name":"[^"]*"' | cut -d'"' -f4 | tr '\n' ' ')token in .state/jenkins-token"
