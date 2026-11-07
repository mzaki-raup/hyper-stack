#!/usr/bin/env bash
# The heavy profile: the shop tables in SQL Server, Oracle and Db2, the
# ClickHouse scenario tables, a Nexus raw repository, and a k3s namespace.
# Each part runs only when its container is up. SQL Server, Oracle and Db2
# accept logins before they are ready, so each waits on its healthcheck
# (which probes the database itself), not on the port (docs/GOTCHAS.md).
SEED_NAME=heavy
source "$(dirname "$0")/lib.sh"

if running mssql; then
  SEED_NAME=mssql
  wait_healthy mssql 120
  dc exec -T mssql sh -c '/opt/mssql-tools18/bin/sqlcmd -C -S localhost -U sa -P "$MSSQL_SA_PASSWORD" -b -h -1 -W -i /hs-seed/mssql-shop.sql' |
    grep '=' | tr '\n' ' ' | sed 's/^/shop: /' | while IFS= read -r l; do log "$l"; done
fi

if running oracle; then
  SEED_NAME=oracle
  wait_healthy oracle 180
  dc exec -T oracle sh -c 'sqlplus -s -L "$APP_USER/$APP_USER_PASSWORD@//localhost:1521/FREEPDB1" @/hs-seed/oracle-shop.sql' |
    grep '=' | tr -s ' \n' ' ' | while IFS= read -r l; do log "app schema: $l"; done
fi

if running db2; then
  SEED_NAME=db2
  wait_healthy db2 180
  dc exec -T db2 su - db2inst1 -c "db2 connect to $DB2_DBNAME >/dev/null && db2 -tf /hs-seed/db2-drop.sql >/dev/null; db2 connect reset >/dev/null" || true
  dc exec -T db2 su - db2inst1 -c "db2 connect to $DB2_DBNAME >/dev/null && db2 -tvsf /hs-seed/db2-shop.sql >/dev/null && db2 -x \"select 'customers=' || count(*) from customers\"; db2 connect reset >/dev/null" |
    while IFS= read -r l; do log "$DB2_DBNAME: $l"; done
fi

if running clickhouse; then
  SEED_NAME=clickhouse
  wait_healthy clickhouse
  dc exec -T clickhouse sh -c 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --multiquery < /hs-seed/clickhouse-shop.sql' |
    tr '\n' ' ' | while IFS= read -r l; do log "$l"; done
fi

if running nexus; then
  SEED_NAME=nexus
  wait_healthy nexus 120
  N="http://$H:8081/service/rest"
  if [ "$(curl -s -o /dev/null -w '%{http_code}' -u "admin:$NEXUS_ADMIN_PASSWORD" "$N/v1/status/check")" != 200 ]; then
    curl -s -f -o /dev/null -u admin:admin123 -X PUT -H 'Content-Type: text/plain' \
      "$N/v1/security/users/admin/change-password" --data-raw "$NEXUS_ADMIN_PASSWORD"
    log "admin password changed from the first-start admin123"
  fi
  A=(-s -u "admin:$NEXUS_ADMIN_PASSWORD")
  if [ "$(curl "${A[@]}" -o /dev/null -w '%{http_code}' "$N/v1/repositories/hs-raw")" != 200 ]; then
    curl "${A[@]}" -f -o /dev/null -X POST -H 'Content-Type: application/json' "$N/v1/repositories/raw/hosted" \
      -d '{"name":"hs-raw","online":true,"storage":{"blobStoreName":"default","strictContentTypeValidation":false,"writePolicy":"allow"}}'
  fi
  curl "${A[@]}" -f -o /dev/null -T "$DATA_DIR/nexus/readme.txt" "http://$H:8081/repository/hs-raw/docs/readme.txt"
  log "repositories: $(curl "${A[@]}" "$N/v1/repositories" | grep -o '"name" *: *"[^"]*"' | cut -d'"' -f4 | tr '\n' ' ')"
fi

if running k3s; then
  SEED_NAME=k3s
  wait_healthy k3s
  dc exec -T k3s kubectl apply -f - < "$DATA_DIR/k3s/shop.yaml" >/dev/null
  log "namespace shop applied; kubeconfig: .state/k3s/kubeconfig.yaml (server https://127.0.0.1:6443 - change it to BENCH_HOST off this box)"
fi
