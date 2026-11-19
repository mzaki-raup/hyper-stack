#!/usr/bin/env bash
# OpenLDAP (FEAT-0117): the OU tree, 12 users and 6 groups of
# seed/data/ldap/seed.ldif under dc=hyperstack,dc=test. The demo password is
# hashed inside the container ({SSHA}) before it is loaded, and both
# passwords travel through files that are removed, never a command line.
# A re-run skips the entries that exist (ldapadd -c).
SEED_NAME=ldap
source "$(dirname "$0")/lib.sh"
running openldap || { log "not running - skipped"; exit 0; }
wait_healthy openldap

BASE=dc=hyperstack,dc=test
ADMIN="cn=admin,$BASE"
ex() { dc exec -T openldap "$@"; }

printf '%s' "$LDAP_ADMIN_PASSWORD" | ex sh -c 'umask 077; cat > /tmp/a.pw'
printf '%s' "$LDAP_DEMO_PASSWORD" | ex sh -c 'umask 077; cat > /tmp/d.pw'
HASH=$(ex slappasswd -T /tmp/d.pw)
ex rm -f /tmp/d.pw

added=$(sed -e "s|@BASE@|$BASE|g" -e "s|@DEMO_PASSWORD@|$HASH|g" "$DATA_DIR/ldap/seed.ldif" |
  ex sh -c "ldapadd -c -x -H ldap://localhost -D '$ADMIN' -y /tmp/a.pw 2>&1 | grep -c '^adding new entry' || true")
users=$(ex ldapsearch -x -H ldap://localhost -D "$ADMIN" -y /tmp/a.pw -b "$BASE" -LLL '(objectClass=inetOrgPerson)' dn |
  grep -c '^dn:' || true)
ex rm -f /tmp/a.pw
log "added $added entries; $users users under $BASE"
