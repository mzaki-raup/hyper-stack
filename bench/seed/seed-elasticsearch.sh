#!/usr/bin/env bash
# Elasticsearch (and Kibana, when it runs): gives Kibana's service account its
# password, writes an API key to .state/elastic-api-key (a read-only key over
# hs-*; Elasticsearch shows a key only once, so a re-run makes a new one and
# invalidates the old), and loads three indices with fixed ids, so a re-run
# overwrites rather than duplicates:
#   hs-logs      360 log lines, apps orders / payments / shipping, levels
#                DEBUG..ERROR, a trace id and a host (mapping: @timestamp date,
#                level and app keyword, message text)
#   hs-orders    60 orders with a customer, items, total, status and a date
#   hs-products  24 products with a category, price, stock and tags
# Needs python3 beside curl.
SEED_NAME=elasticsearch
source "$(dirname "$0")/lib.sh"
running elasticsearch || { log "not running - skipped"; exit 0; }
wait_healthy elasticsearch 60 || true

export ES_URL="http://$H:${ELASTIC_PORT:-9200}" ES_KEY_FILE="$STATE_DIR/elastic-api-key"
export KB_URL="http://$H:${KIBANA_PORT:-5601}"
python3 - <<'PY' || exit 1
import base64, json, os, random, sys, time, urllib.error, urllib.request

es, kb = os.environ["ES_URL"], os.environ["KB_URL"]
auth = "Basic " + base64.b64encode(f"elastic:{os.environ['ELASTIC_PASSWORD']}".encode()).decode()


def call(url, method="GET", body=None, headers=None, ndjson=False, ok404=False):
    data = None
    if body is not None:
        data = body.encode() if ndjson else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method, headers={
        "Authorization": auth,
        "Content-Type": "application/x-ndjson" if ndjson else "application/json", **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        if ok404 and e.code == 404:
            return None
        sys.exit(f"[elasticsearch] {method} {url}: {e.code} {e.read().decode()[:300]}")


for _ in range(60):
    try:
        call(es + "/_cluster/health")
        break
    except SystemExit:
        time.sleep(3)
    except Exception:
        time.sleep(3)
else:
    sys.exit("[elasticsearch] did not come up")

# Kibana's service account.
call(es + "/_security/user/kibana_system/_password", "POST",
     {"password": os.environ["KIBANA_SYSTEM_PASSWORD"]})

# The read-only API key.
old = call(es + "/_security/api_key?name=hyperstack&active_only=true")["api_keys"]
if old:
    call(es + "/_security/api_key", "DELETE", {"ids": [k["id"] for k in old]})
key = call(es + "/_security/api_key", "POST", {
    "name": "hyperstack",
    "role_descriptors": {"hs-read": {
        "cluster": ["monitor"],
        "indices": [{"names": ["hs-*"], "privileges": ["read", "view_index_metadata"]}]}}})
open(os.environ["ES_KEY_FILE"], "w").write(key["encoded"])

# The data: fixed seeds, so the same documents every time.
rnd = random.Random(7)
apps = {"orders": ["checkout started", "order {n} created", "stock reserved for order {n}",
                   "order {n} cancelled by the customer", "price list reloaded"],
        "payments": ["payment {n} authorised", "payment {n} captured", "card declined for payment {n}",
                     "refund {n} issued", "gateway timeout after 3000 ms"],
        "shipping": ["label printed for order {n}", "parcel {n} handed to the carrier",
                     "address check failed for order {n}", "carrier API slow: 1800 ms",
                     "parcel {n} delivered"]}
levels = ["DEBUG"] * 3 + ["INFO"] * 9 + ["WARN"] * 2 + ["ERROR"]
start = 1793491200  # 2026-11-01T00:00:00Z
logs = []
for i in range(360):
    app = rnd.choice(list(apps))
    level = rnd.choice(levels)
    msg = rnd.choice(apps[app]).format(n=1000 + rnd.randrange(60))
    if "declined" in msg or "timeout" in msg or "failed" in msg:
        level = "ERROR" if level in ("DEBUG", "INFO") else level
    ts = start + i * 1440 + rnd.randrange(600)
    logs.append((f"log-{i:04d}", {
        "@timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts)),
        "app": app, "level": level, "message": msg,
        "host": f"{app}-{1 + rnd.randrange(3)}", "trace_id": f"{rnd.getrandbits(64):016x}"}))

cats = ["Books", "Garden", "Kitchen", "Office", "Toys", "Sport"]
products = []
for i in range(24):
    products.append((f"p-{i:03d}", {
        "sku": f"HS-{1000 + i}", "name": f"{rnd.choice(['Classic', 'Pro', 'Mini', 'Max'])} {cats[i % 6]} item {i + 1}",
        "category": cats[i % 6], "price": round(5 + rnd.random() * 95, 2), "stock": rnd.randrange(0, 200),
        "tags": rnd.sample(["new", "sale", "eco", "gift", "bestseller"], 2)}))
orders = []
for i in range(60):
    items = rnd.sample(products, rnd.randrange(1, 4))
    orders.append((f"o-{i:04d}", {
        "order_id": 1000 + i, "customer": rnd.choice(["Ada", "Grace", "Linus", "Margaret", "Dennis", "Barbara"]),
        "items": [{"sku": p["sku"], "qty": rnd.randrange(1, 4)} for _, p in items],
        "total": round(sum(p["price"] for _, p in items) * (1 + rnd.random()), 2),
        "status": rnd.choice(["paid", "paid", "paid", "pending", "shipped", "cancelled"]),
        "created": time.strftime("%Y-%m-%d", time.gmtime(start + i * 43200))}))

mappings = {
    "hs-logs": {"@timestamp": {"type": "date"}, "app": {"type": "keyword"}, "level": {"type": "keyword"},
                "message": {"type": "text"}, "host": {"type": "keyword"}, "trace_id": {"type": "keyword"}},
    "hs-orders": {"order_id": {"type": "integer"}, "customer": {"type": "keyword"},
                  "items": {"type": "nested", "properties": {"sku": {"type": "keyword"}, "qty": {"type": "integer"}}},
                  "total": {"type": "double"}, "status": {"type": "keyword"}, "created": {"type": "date", "format": "yyyy-MM-dd"}},
    "hs-products": {"sku": {"type": "keyword"}, "name": {"type": "text"}, "category": {"type": "keyword"},
                    "price": {"type": "double"}, "stock": {"type": "integer"}, "tags": {"type": "keyword"}}}
for index, docs in (("hs-logs", logs), ("hs-orders", orders), ("hs-products", products)):
    if call(f"{es}/{index}", "GET", ok404=True) is None:
        call(f"{es}/{index}", "PUT", {"settings": {"number_of_shards": 1, "number_of_replicas": 0},
                                      "mappings": {"properties": mappings[index]}})
    bulk = "".join(json.dumps({"index": {"_index": index, "_id": i}}) + "\n" + json.dumps(d) + "\n"
                   for i, d in docs)
    r = call(es + "/_bulk?refresh=true", "POST", bulk, ndjson=True)
    if r.get("errors"):
        sys.exit(f"[elasticsearch] bulk into {index} reported errors")
    print(f"[elasticsearch] {index}: {len(docs)} documents")

print(f"[elasticsearch] API key saved to {os.environ['ES_KEY_FILE']} (read-only over hs-*)")

# A Kibana data view over the logs, when Kibana is up (best effort).
try:
    call(kb + "/api/data_views/data_view", "POST",
         {"data_view": {"title": "hs-logs", "name": "HyperStack logs", "timeFieldName": "@timestamp"},
          "override": True}, headers={"kbn-xsrf": "hyperstack"})
    print("[elasticsearch] Kibana data view 'HyperStack logs' created")
except (SystemExit, Exception):
    print("[elasticsearch] Kibana is not up yet - data view skipped (re-run the seed once it is)")
PY
