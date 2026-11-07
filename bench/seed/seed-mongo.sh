#!/usr/bin/env bash
# MongoDB: shop (orders, customers), catalogue (products, reviews + indexes),
# hstest (empty orders), and a root user in `admin` (auth itself stays off).
SEED_NAME=mongo
source "$(dirname "$0")/lib.sh"
running mongo || { log "not running - skipped"; exit 0; }
wait_healthy mongo

for f in "$DATA_DIR"/mongo/*.json; do
  name=$(basename "$f" .json)   # <db>.<collection>
  db=${name%%.*}
  coll=${name#*.}
  if [ "$(tr -d '[:space:]' < "$f")" = "[]" ]; then
    dc exec -T mongo mongosh --quiet "$db" --eval "db.getCollection('$coll').drop(); db.createCollection('$coll')" >/dev/null
  else
    dc exec -T mongo mongoimport --quiet --db "$db" --collection "$coll" --drop --jsonArray < "$f"
  fi
done

dc exec -T -e HS_U="$MONGO_USER" -e HS_P="$MONGO_PASSWORD" mongo mongosh --quiet --eval '
  const cat = db.getSiblingDB("catalogue");
  cat.products.createIndex({ sku: 1 }, { unique: true });
  cat.reviews.createIndex({ sku: 1 });
  const admin = db.getSiblingDB("admin");
  if (admin.getUser(process.env.HS_U)) admin.changeUserPassword(process.env.HS_U, process.env.HS_P);
  else admin.createUser({ user: process.env.HS_U, pwd: process.env.HS_P, roles: ["root"] });
' >/dev/null
log "$(dc exec -T mongo mongosh --quiet --eval '
  ["shop", "catalogue", "hstest"].map(d => d + "(" + db.getSiblingDB(d).getCollectionNames().sort()
    .map(c => c + "=" + db.getSiblingDB(d)[c].countDocuments()).join(" ") + ")").join(" ")')"
