#!/usr/bin/env bash
# Grafana + Loki: creates a service account "hyperstack" (Viewer) with a token
# saved in .state/grafana-token (Grafana shows a token only once), and pushes
# demo log lines to Loki: apps orders, payments and shipping in levels info,
# warn and error over the last hour, so the Logs activity's Grafana source and
# the dashboard's Logs panel have data. The dashboard and both data sources are
# provisioned from config/grafana. A re-run skips an existing account and token,
# and pushes the log lines again only when Loki has none.
# Needs python3 beside curl.
SEED_NAME=grafana
source "$(dirname "$0")/lib.sh"
running grafana || { log "not running - skipped"; exit 0; }

export GF_URL="http://$H:${GRAFANA_PORT:-3130}" LOKI_URL="http://$H:${LOKI_PORT:-3131}"
export GF_TOKEN_FILE="$STATE_DIR/grafana-token"
for i in $(seq 1 40); do
  curl -fsS "$GF_URL/api/health" 2>/dev/null | grep -q '"database": *"ok"' && break
  sleep 3
done
curl -fsS "$GF_URL/api/health" | grep -q '"database": *"ok"' || { warn "Grafana did not come up"; exit 1; }
for i in $(seq 1 40); do
  curl -fsS "$LOKI_URL/ready" 2>/dev/null | grep -q ready && break
  sleep 3
done

python3 - <<'PY'
import base64, json, os, random, sys, time, urllib.error, urllib.request

gf = os.environ["GF_URL"]
loki = os.environ["LOKI_URL"]
auth = "Basic " + base64.b64encode(
    f"{os.environ.get('GRAFANA_ADMIN_USER', 'admin')}:{os.environ['GRAFANA_ADMIN_PASSWORD']}".encode()).decode()


def call(method, url, body=None, headers=None):
    req = urllib.request.Request(url, method=method)
    req.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(req, data) as r:
            text = r.read().decode()
            return json.loads(text) if text else None
    except urllib.error.HTTPError as e:
        sys.exit(f"[grafana] {method} {url}: {e.code} {e.read().decode()[:300]}")


token_file = os.environ["GF_TOKEN_FILE"]
if not os.path.exists(token_file):
    found = call("GET", gf + "/api/serviceaccounts/search?query=hyperstack", headers={"Authorization": auth})
    accounts = [a for a in found.get("serviceAccounts", []) if a["name"] == "hyperstack"]
    sa = accounts[0] if accounts else call(
        "POST", gf + "/api/serviceaccounts", {"name": "hyperstack", "role": "Viewer"},
        {"Authorization": auth})
    made = call("POST", f"{gf}/api/serviceaccounts/{sa['id']}/tokens",
                {"name": "bench-" + str(int(time.time()))}, {"Authorization": auth})
    with open(token_file, "w") as f:
        f.write(made["key"])
    print("[grafana] service-account token written to", token_file)

now_ns = int(time.time() * 1e9)
probe = urllib.request.Request(
    loki + "/loki/api/v1/query_range?query=%7Bapp%3D~%22.%2B%22%7D&limit=1&start=" + str(now_ns - 7200 * 10**9))
have = json.load(urllib.request.urlopen(probe))["data"]["result"]
if have:
    print("[grafana] Loki already has log lines - skipped")
    sys.exit(0)

random.seed(7)
messages = {
    "orders": ["order {n} accepted", "order {n} packed", "order {n} shipped", "stock low for sku {n}",
               "payment for order {n} is late", "order {n} failed: address not found"],
    "payments": ["card charge {n} approved", "card charge {n} declined", "refund {n} issued",
                 "gateway timeout after 30s on charge {n}", "charge {n} retried"],
    "shipping": ["label {n} printed", "parcel {n} handed to carrier", "carrier API returned 503 for parcel {n}"],
}
levels = ["info"] * 6 + ["warn"] * 2 + ["error"]
streams = {}
n = 1000
for i in range(360):
    ts = now_ns - (3600 - i * 10) * 10**9 + random.randint(0, 9 * 10**8)
    app = random.choice(list(messages))
    text = random.choice(messages[app]).format(n=n + i)
    level = "error" if ("failed" in text or "503" in text or "timeout" in text) else random.choice(levels)
    streams.setdefault((app, level), []).append([str(ts), f"{level.upper()} {text}"])
payload = {"streams": [{"stream": {"app": a, "level": l, "env": "bench"}, "values": v}
                       for (a, l), v in streams.items()]}
call("POST", loki + "/loki/api/v1/push", payload)
print("[grafana] pushed", sum(len(v) for v in streams.values()), "log lines to Loki")
PY
