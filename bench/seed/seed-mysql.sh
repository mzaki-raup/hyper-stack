#!/usr/bin/env bash
# MySQL: database `shop` with `agents` (4 rows) and `tickets` (6 rows).
SEED_NAME=mysql
source "$(dirname "$0")/lib.sh"
running mysql || { log "not running - skipped"; exit 0; }
wait_healthy mysql

dc exec -T mysql sh -c 'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql -uroot' < "$DATA_DIR/sql/mysql-shop.sql"
log "shop: $(dc exec -T mysql sh -c 'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql -uroot -N -e "select concat((select count(*) from shop.agents), \" agents, \", (select count(*) from shop.tickets), \" tickets\")"')"
