#!/usr/bin/env bash
# PostgreSQL: the Windows bench's extra logins (app, postgres) and the `shop`
# schema with its rows (customers, products, orders, order_lines, order_totals).
SEED_NAME=postgres
source "$(dirname "$0")/lib.sh"
running postgres || { log "not running - skipped"; exit 0; }
wait_healthy postgres

dc exec -T postgres psql -q -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d postgres \
  -v app_user="$PG_APP_USER" -v app_pass="$PG_APP_PASSWORD" -v su_pass="$PG_SUPERUSER_PASSWORD" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN SUPERUSER', :'app_user')
 WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'app_user') \gexec
SELECT 'CREATE ROLE postgres LOGIN SUPERUSER'
 WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'postgres') \gexec
ALTER ROLE :"app_user" WITH LOGIN SUPERUSER PASSWORD :'app_pass';
ALTER ROLE postgres WITH LOGIN SUPERUSER PASSWORD :'su_pass';
SQL

dc exec -T postgres psql -q -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
  < "$DATA_DIR/sql/postgres-shop.sql" >/dev/null
log "shop: $(dc exec -T postgres psql -At -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "select
  'customers=' || (select count(*) from customers) || ' products=' || (select count(*) from products) ||
  ' orders=' || (select count(*) from orders) || ' order_lines=' || (select count(*) from order_lines)")"
