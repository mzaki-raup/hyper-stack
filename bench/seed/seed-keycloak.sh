#!/usr/bin/env bash
# Keycloak: realm hs-demo (realm roles viewer/editor/admin and the composite
# power-user; groups engineering > backend, frontend, support, contractors;
# 25 users with attributes, three disabled, four with required actions;
# clients web-app, orders-api, hs-admin with its service account; login and
# admin events on) and realm hs-other (3 users). Then a few password-grant
# logins through web-app, one wrong password among them, so Sessions and
# Events have data. Everything goes through kcadm.sh inside the container.
# The realm contents are in seed/data/keycloak/*.json; @DEMO_PASSWORD@ and
# @HS_ADMIN_SECRET@ there are filled in from .env on the way in. A re-run
# skips what exists (partialImport SKIP) and re-applies the realm settings,
# the service account's roles and the hs-admin secret.
SEED_NAME=keycloak
source "$(dirname "$0")/lib.sh"
running keycloak || { log "not running - skipped"; exit 0; }
wait_healthy keycloak

KC=/opt/keycloak/bin/kcadm.sh
CFG=/tmp/hs-seed-kcadm.config
kc() { dc exec -T keycloak "$KC" "$@" --config "$CFG"; }
fill() {
  sed -e "s/@DEMO_PASSWORD@/$KEYCLOAK_DEMO_PASSWORD/g" -e "s/@HS_ADMIN_SECRET@/$KEYCLOAK_HS_ADMIN_SECRET/g" "$DATA_DIR/keycloak/$1"
}
# The bootstrap admin's credentials are already in the container's environment.
dc exec -T keycloak sh -c "$KC config credentials --config $CFG --server http://localhost:8080 --realm master \
  --user \"\$KC_BOOTSTRAP_ADMIN_USERNAME\" --password \"\$KC_BOOTSTRAP_ADMIN_PASSWORD\"" >/dev/null 2>&1 ||
  { warn "admin login failed - is KEYCLOAK_ADMIN_PASSWORD the one the container first started with?"; exit 1; }

ensure_realm() {
  kc get "realms/$1" --fields realm >/dev/null 2>&1 ||
    printf '{"realm":"%s","enabled":true,"displayName":"%s"}' "$1" "$2" | kc create realms -f - >/dev/null
}
ensure_realm hs-demo "HyperStack Demo"
ensure_realm hs-other "HyperStack Other"

# Events first, so the imports below are recorded as admin events. Sessions
# stay a week, so the Sessions view still has rows the day after seeding.
settings='{"eventsEnabled":true,"eventsExpiration":2592000,"adminEventsEnabled":true,"adminEventsDetailsEnabled":true,
  "ssoSessionIdleTimeout":604800,"ssoSessionMaxLifespan":2592000,"bruteForceProtected":false}'
for realm in hs-demo hs-other; do
  printf '%s' "$settings" | kc update "realms/$realm" -f - >/dev/null
  # The declarative user profile drops attributes it does not declare
  # (department, employeeId) unless unmanaged attributes are allowed.
  kc update "realms/$realm/users/profile" -m -s unmanagedAttributePolicy=ENABLED >/dev/null
done

fill hs-demo.json | kc create partialImport -r hs-demo -s ifResourceExists=SKIP -f - >/dev/null
fill hs-other.json | kc create partialImport -r hs-other -s ifResourceExists=SKIP -f - >/dev/null

# hs-admin: enough of realm-management for the IDM activity's first phase.
kc add-roles -r hs-demo --uusername service-account-hs-admin --cclientid realm-management \
  --rolename view-realm --rolename view-users --rolename query-users --rolename manage-users \
  --rolename query-groups --rolename view-clients --rolename query-clients --rolename manage-clients \
  --rolename view-events --rolename manage-events >/dev/null
id_of() { kc get clients -r hs-demo -q "clientId=$1" --fields id --format csv --noquotes | tr -d '\r'; }
admin_id=$(id_of hs-admin)
printf '{"secret":"%s"}' "$KEYCLOAK_HS_ADMIN_SECRET" | kc update "clients/$admin_id" -r hs-demo -m -f - >/dev/null
# Keycloak 26.8 checks an admin API token's audience and scope: without the
# realm's kc-admin-api client scope the roles above still answer 401.
scope_id=$(kc get client-scopes -r hs-demo --fields id,name --format csv --noquotes | tr -d '\r' | sed -n 's/,kc-admin-api$//p')
kc update "clients/$admin_id/default-client-scopes/$scope_id" -r hs-demo -b '{}' >/dev/null

# Login traffic, once: skipped while web-app still has sessions from an earlier run.
web_id=$(id_of web-app)
sessions=$(kc get "clients/$web_id/session-count" -r hs-demo | tr -dc '0-9')
if [ "${sessions:-0}" -eq 0 ]; then
  login() {   # user password - a password grant through web-app, as that user
    dc exec -T -e HS_PW="$2" keycloak sh -c "$KC config credentials --config /tmp/hs-seed-login.config \
      --server http://localhost:8080 --realm hs-demo --client web-app --user $1 --password \"\$HS_PW\"" >/dev/null 2>&1
  }
  for u in alice.nguyen bob.smith carol.jones frank.miller judy.chen ken.adams uma.singh; do
    login "$u" "$KEYCLOAK_DEMO_PASSWORD" || warn "login as $u failed"
  done
  login dave.kim "not-the-password" && warn "a wrong password was accepted"
  login mallory.brown "$KEYCLOAK_DEMO_PASSWORD" && warn "a disabled user logged in"
  login heidi.patel "$KEYCLOAK_DEMO_PASSWORD" && warn "a user with a required action logged in"
  dc exec -T keycloak rm -f /tmp/hs-seed-login.config
  sessions=$(kc get "clients/$web_id/session-count" -r hs-demo | tr -dc '0-9')
fi

users=$(kc get users/count -r hs-demo | tr -dc '0-9')
other=$(kc get users/count -r hs-other | tr -dc '0-9')
log "hs-demo: $users users (service account not counted), web-app sessions $sessions; hs-other: $other users"
