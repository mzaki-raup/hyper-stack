#!/usr/bin/env bash
# Forgejo: the admin user, an access token for HyperStack, repository
# <admin>/demo (README, a workflow_dispatch workflow with inputs, branch dev,
# an issue), and the forgejo-runner registered with the `docker:host` label.
SEED_NAME=forgejo
source "$(dirname "$0")/lib.sh"
running forgejo || { log "not running - skipped"; exit 0; }
wait_healthy forgejo

U=$FORGEJO_ADMIN_USER
fj() { dc exec -T -u git forgejo forgejo "$@"; }

if fj admin user list | awk 'NR > 1 { print $2 }' | grep -qx "$U"; then
  fj admin user change-password --username "$U" --password "$FORGEJO_ADMIN_PASSWORD" --must-change-password=false >/dev/null
else
  fj admin user create --admin --username "$U" --password "$FORGEJO_ADMIN_PASSWORD" \
    --email "$FORGEJO_ADMIN_EMAIL" --must-change-password=false >/dev/null
fi

API="http://$H:3000/api/v1"
A=(-s -u "$U:$FORGEJO_ADMIN_PASSWORD" -H 'Content-Type: application/json')

if [ ! -s "$STATE_DIR/forgejo-token" ]; then
  token=$(fj admin user generate-access-token --username "$U" --token-name "hyperstack-$(date +%s)" --scopes all --raw | tr -d '\r\n')
  save_state forgejo-token "$token"
fi

if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$API/repos/$U/demo")" != 200 ]; then
  curl "${A[@]}" -f -o /dev/null -X POST "$API/user/repos" \
    -d '{"name":"demo","auto_init":true,"default_branch":"main","readme":"Default"}'
fi
if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$API/repos/$U/demo/contents/.forgejo/workflows/build.yml")" != 200 ]; then
  content=$(base64 < "$DATA_DIR/forgejo/build.yml" | tr -d '\n')
  curl "${A[@]}" -f -o /dev/null -X POST "$API/repos/$U/demo/contents/.forgejo/workflows/build.yml" \
    -d "{\"content\":\"$content\",\"message\":\"ci: a build workflow with inputs\",\"branch\":\"main\"}"
fi
if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$API/repos/$U/demo/branches/dev")" != 200 ]; then
  curl "${A[@]}" -f -o /dev/null -X POST "$API/repos/$U/demo/branches" -d '{"new_branch_name":"dev","old_branch_name":"main"}'
fi
if ! curl "${A[@]}" "$API/repos/$U/demo/issues?state=all&type=issues" | grep -q '"title":"Flaky test'; then
  curl "${A[@]}" -f -o /dev/null -X POST "$API/repos/$U/demo/issues" \
    -d '{"title":"Flaky test in the cart suite","body":"`cart::add` fails about one run in ten."}'
fi

if running forgejo-runner && ! dc exec -T forgejo-runner test -f /data/.runner; then
  reg=$(fj actions generate-runner-token | tr -d '\r\n')
  dc exec -T forgejo-runner forgejo-runner register --no-interactive --instance http://forgejo:3000 \
    --token "$reg" --name hs-runner --labels docker:host >/dev/null
  log "runner registered (it starts within 15 s)"
fi

log "repo $U/demo ready; token in .state/forgejo-token"
