#!/usr/bin/env bash
# Metabase: finishes the first-run setup (admin from METABASE_ADMIN_EMAIL /
# METABASE_ADMIN_PASSWORD), creates an API key for the Administrators group
# (saved in .state/metabase-api-key; Metabase shows a key only once), and
# fills a collection "HyperStack Demo" over the built-in Sample Database:
# one question per chart kind, a native question with a template tag, and a
# dashboard with two tabs and a Category filter. A re-run skips what exists.
# Needs python3 beside curl (the content is JSON).
SEED_NAME=metabase
source "$(dirname "$0")/lib.sh"
running metabase || { log "not running - skipped"; exit 0; }
wait_healthy metabase 60 || true

export MB_URL="http://$H:${METABASE_PORT:-3030}"
export MB_KEY_FILE="$STATE_DIR/metabase-api-key"
for i in $(seq 1 60); do
  curl -fsS "$MB_URL/api/health" 2>/dev/null | grep -q ok && break
  sleep 5
done
curl -fsS "$MB_URL/api/health" | grep -q ok || { warn "Metabase did not come up"; exit 1; }

python3 - <<'PY'
import json, os, sys, urllib.request, urllib.error, uuid

url = os.environ["MB_URL"]
email = os.environ["METABASE_ADMIN_EMAIL"]
password = os.environ["METABASE_ADMIN_PASSWORD"]
key_file = os.environ["MB_KEY_FILE"]


def call(method, path, body=None, session=None):
    req = urllib.request.Request(url + path, method=method)
    req.add_header("Content-Type", "application/json")
    if session:
        req.add_header("X-Metabase-Session", session)
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(req, data) as r:
            text = r.read().decode()
            return json.loads(text) if text else None
    except urllib.error.HTTPError as e:
        sys.exit(f"[metabase] {method} {path}: {e.code} {e.read().decode()[:300]}")


props = call("GET", "/api/session/properties")
token = props.get("setup-token")
# The token stays in the properties after setup; has-user-setup is the sign.
if token and not props.get("has-user-setup"):
    call("POST", "/api/setup", {
        "token": token,
        "user": {"first_name": "Bench", "last_name": "Admin", "email": email,
                 "password": password, "site_name": "HyperStack Bench"},
        "prefs": {"site_name": "HyperStack Bench", "site_locale": "en",
                  "allow_tracking": False},
    })
    print("[metabase] setup finished")
session = call("POST", "/api/session", {"username": email, "password": password})["id"]

if not os.path.exists(key_file):
    made = call("POST", "/api/api-key",
                {"name": "hyperstack-bench-" + uuid.uuid4().hex[:6], "group_id": 2},
                session)
    with open(key_file, "w") as f:
        f.write(made["unmasked_key"])
    print("[metabase] API key written to", key_file)

if any(c.get("name") == "HyperStack Demo" for c in call("GET", "/api/collection", session=session)):
    print("[metabase] demo collection exists - skipped")
    sys.exit(0)

dbs = call("GET", "/api/database", session=session)["data"]
db = next(d for d in dbs if d.get("is_sample"))
meta = call("GET", f"/api/database/{db['id']}/metadata", session=session)
tables = {t["name"]: t for t in meta["tables"]}
fields = {(t["name"], f["name"]): f["id"] for t in meta["tables"] for f in t["fields"]}
orders, products = tables["ORDERS"]["id"], tables["PRODUCTS"]["id"]
fk_product = fields[("ORDERS", "PRODUCT_ID")]
created = ["field", fields[("ORDERS", "CREATED_AT")], {"temporal-unit": "month"}]
category = ["field", fields[("PRODUCTS", "CATEGORY")], {"source-field": fk_product}]
total = ["field", fields[("ORDERS", "TOTAL")], None]

coll = call("POST", "/api/collection", {"name": "HyperStack Demo"}, session)["id"]


def mbql(table, **query):
    return {"type": "query", "database": db["id"], "query": {"source-table": table, **query}}


def card(name, display, query, settings=None):
    return call("POST", "/api/card", {
        "name": name, "display": display, "dataset_query": query,
        "visualization_settings": settings or {}, "collection_id": coll,
    }, session)["id"]


count = [["count"]]
by_month = mbql(orders, aggregation=count, breakout=[created])
by_category = mbql(orders, aggregation=count, breakout=[category])
cards = {
    "table": card("Orders table", "table", mbql(orders, limit=200)),
    "line": card("Orders per month", "line", by_month),
    "area": card("Orders per month (area)", "area", by_month),
    "bar": card("Orders by category", "bar", by_category),
    "row": card("Orders by category (row)", "row", by_category),
    "pie": card("Share of orders by category", "pie", by_category),
    "scalar": card("Orders in total", "scalar", mbql(orders, aggregation=count)),
    "combo": card("Revenue per month", "combo",
                  mbql(orders, aggregation=[["sum", total], ["count"]], breakout=[created])),
}
tag = {"id": str(uuid.uuid4()), "name": "min_total", "display-name": "Min total",
       "type": "number", "default": "100"}
cards["native"] = call("POST", "/api/card", {
    "name": "Big orders (native)", "display": "table", "collection_id": coll,
    "visualization_settings": {},
    "dataset_query": {"type": "native", "database": db["id"], "native": {
        "query": "SELECT ID, TOTAL, CREATED_AT FROM ORDERS WHERE TOTAL > {{min_total}} LIMIT 100",
        "template-tags": {"min_total": tag}}},
    "parameters": [{"id": tag["id"], "name": "Min total", "slug": "min_total",
                    "type": "number/=", "target": ["variable", ["template-tag", "min_total"]],
                    "default": 100}],
}, session)["id"]

dash = call("POST", "/api/dashboard", {"name": "HyperStack Overview", "collection_id": coll}, session)["id"]
filt = {"id": "category", "name": "Category", "slug": "category", "type": "string/="}
target = ["dimension", category]


def tile(i, tab, card_id, row, col, w, h, filtered=True):
    maps = [{"parameter_id": "category", "card_id": card_id, "target": target}] if filtered else []
    return {"id": i, "card_id": card_id, "dashboard_tab_id": tab, "row": row, "col": col,
            "size_x": w, "size_y": h, "parameter_mappings": maps, "visualization_settings": {}}


call("PUT", f"/api/dashboard/{dash}", {
    "parameters": [filt],
    "tabs": [{"id": -1, "name": "Overview"}, {"id": -2, "name": "Detail"}],
    "dashcards": [
        tile(-1, -1, cards["scalar"], 0, 0, 6, 3, filtered=False),
        tile(-2, -1, cards["line"], 0, 6, 18, 6, filtered=False),
        tile(-3, -1, cards["bar"], 6, 0, 12, 6),
        tile(-4, -1, cards["pie"], 6, 12, 12, 6),
        tile(-5, -2, cards["table"], 0, 0, 24, 10, filtered=False),
    ],
}, session)
print("[metabase] demo collection, 9 questions and a 2-tab dashboard created")
PY
