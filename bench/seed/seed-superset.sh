#!/usr/bin/env bash
# Apache Superset: a bench database (a SQLite file the script makes inside the
# container, with a sales table of 400 rows), one dataset over it, charts of
# kinds table, line, bar, pie and big number, and the dashboard "HyperStack
# Overview" with a Region select filter. Everything goes over the REST API with a bearer token (CSRF is off
# in config/superset). A re-run skips what exists.
# Needs python3 beside curl; the SQLite file is made with the container's own
# python.
SEED_NAME=superset
source "$(dirname "$0")/lib.sh"
running superset || { log "not running - skipped"; exit 0; }
wait_healthy superset 60 || true

# The data: a SQLite file under superset_home, made once.
dc exec -T superset python - <<'PY'
import os, random, sqlite3
path = "/app/superset_home/bench.db"
if os.path.exists(path):
    raise SystemExit(0)
random.seed(11)
con = sqlite3.connect(path)
con.execute("create table sales (id integer primary key, day text, region text, category text, amount real, qty integer)")
rows = []
for i in range(400):
    month = 1 + i % 12
    rows.append((i + 1, f"2026-{month:02d}-{1 + i % 27:02d}", random.choice(["East", "West", "Central", "South"]),
                 random.choice(["Widget", "Gadget", "Gizmo", "Doohickey"]),
                 round(random.uniform(20, 900), 2), random.randint(1, 12)))
con.executemany("insert into sales values (?,?,?,?,?,?)", rows)
con.commit()
print("[superset] bench.db made")
PY

export SS_URL="http://$H:${SUPERSET_PORT:-8188}"
for i in $(seq 1 40); do curl -fsS "$SS_URL/health" 2>/dev/null | grep -qi ok && break; sleep 3; done

python3 - <<'PY'
import json, os, sys, urllib.error, urllib.request

url = os.environ["SS_URL"]


def call(method, path, body=None, token=None):
    req = urllib.request.Request(url + path, method=method)
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    data = json.dumps(body).encode() if body is not None else None
    try:
        with urllib.request.urlopen(req, data) as r:
            text = r.read().decode()
            return json.loads(text) if text else None
    except urllib.error.HTTPError as e:
        sys.exit(f"[superset] {method} {path}: {e.code} {e.read().decode()[:400]}")


token = call("POST", "/api/v1/security/login", {
    "username": os.environ.get("SUPERSET_ADMIN_USER", "admin"),
    "password": os.environ["SUPERSET_ADMIN_PASSWORD"], "provider": "db", "refresh": True})["access_token"]


def find(kind, column, value):
    q = json.dumps({"filters": [{"col": column, "opr": "eq", "value": value}]})
    hit = call("GET", f"/api/v1/{kind}/?q={urllib.request.quote(q)}", token=token)["result"]
    return hit[0]["id"] if hit else None


def ensure_filter():
    """The dashboard's native filter (Region, a select over sales.region), added
    to a dashboard made before it existed too."""
    dash_id = find("dashboard", "dashboard_title", "HyperStack Overview")
    ds_id = find("dataset", "table_name", "sales")
    d = call("GET", f"/api/v1/dashboard/{dash_id}", token=token)["result"]
    meta = json.loads(d.get("json_metadata") or "{}")
    if meta.get("native_filter_configuration"):
        return
    meta["native_filter_configuration"] = [{
        "id": "NATIVE_FILTER-region", "name": "Region", "filterType": "filter_select",
        "targets": [{"datasetId": ds_id, "column": {"name": "region"}}],
        "controlValues": {"multiSelect": False, "enableEmptyFilter": False},
        "defaultDataMask": {"filterState": {}}, "cascadeParentIds": [], "scope": {"rootPath": ["ROOT_ID"], "excluded": []},
        "type": "NATIVE_FILTER"}]
    call("PUT", f"/api/v1/dashboard/{dash_id}", {"json_metadata": json.dumps(meta)}, token)
    print("[superset] the Region filter added to the dashboard")


if find("dashboard", "dashboard_title", "HyperStack Overview"):
    ensure_filter()
    print("[superset] demo content exists - skipped")
    sys.exit(0)

db = find("database", "database_name", "Bench Sales") or call("POST", "/api/v1/database/", {
    "database_name": "Bench Sales", "sqlalchemy_uri": "sqlite:////app/superset_home/bench.db",
    "expose_in_sqllab": True, "allow_dml": False}, token)["id"]
ds = call("POST", "/api/v1/dataset/", {"database": db, "schema": "", "table_name": "sales"}, token)["id"]


def chart(name, viz, params, queries):
    """A chart as the UI saves it: params AND the query context that runs it
    (GET /chart/{id}/data/ answers 'no query context saved' without one)."""
    params = {"viz_type": viz, "datasource": f"{ds}__table", **params}
    base = {"filters": [], "extras": {"where": "", "having": ""}, "annotation_layers": [],
            "series_limit": 0, "order_desc": True, "url_params": {}, "custom_params": {},
            "custom_form_data": {}, "orderby": [], "metrics": [], "columns": []}
    context = {"datasource": {"id": ds, "type": "table"}, "force": False,
               "queries": [{**base, **q} for q in queries], "form_data": params,
               "result_format": "json", "result_type": "full"}
    return call("POST", "/api/v1/chart/", {
        "slice_name": name, "viz_type": viz, "datasource_id": ds, "datasource_type": "table",
        "params": json.dumps(params), "query_context": json.dumps(context)}, token)["id"]


sum_amount = {"expressionType": "SQL", "sqlExpression": "SUM(amount)", "label": "Revenue"}
count = {"expressionType": "SQL", "sqlExpression": "COUNT(*)", "label": "Orders"}
month = {"expressionType": "SQL", "sqlExpression": "substr(day, 1, 7)", "label": "month"}
cols = ["day", "region", "category", "amount", "qty"]
charts = {
    "table": chart("Sales table", "table",
                   {"query_mode": "raw", "all_columns": cols, "row_limit": 200},
                   [{"columns": cols, "row_limit": 200}]),
    "line": chart("Revenue by month", "echarts_timeseries_line",
                  {"x_axis": month, "metrics": [sum_amount], "groupby": [], "row_limit": 1000},
                  [{"columns": [{**month, "columnType": "BASE_AXIS"}], "metrics": [sum_amount],
                    "row_limit": 1000}]),
    "bar": chart("Revenue by category", "echarts_timeseries_bar",
                 {"x_axis": "category", "metrics": [sum_amount], "groupby": [], "row_limit": 100},
                 [{"columns": ["category"], "metrics": [sum_amount], "row_limit": 100}]),
    "pie": chart("Orders by region", "pie",
                 {"metric": count, "groupby": ["region"], "row_limit": 100},
                 [{"columns": ["region"], "metrics": [count], "row_limit": 100}]),
    "number": chart("Orders in total", "big_number_total", {"metric": count},
                    [{"metrics": [count]}]),
}
dash = call("POST", "/api/v1/dashboard/", {"dashboard_title": "HyperStack Overview", "published": True}, token)["id"]
for cid in charts.values():
    call("PUT", f"/api/v1/chart/{cid}", {"dashboards": [dash]}, token)


def cell(n, cid, w, h):
    return {"type": "CHART", "id": f"CHART-{n}", "children": [], "meta": {"chartId": cid, "width": w, "height": h}}


position = {
    "DASHBOARD_VERSION_KEY": "v2",
    "ROOT_ID": {"type": "ROOT", "id": "ROOT_ID", "children": ["GRID_ID"]},
    "GRID_ID": {"type": "GRID", "id": "GRID_ID", "children": ["ROW-1", "ROW-2"], "parents": ["ROOT_ID"]},
    "ROW-1": {"type": "ROW", "id": "ROW-1", "children": ["CHART-n", "CHART-l"], "meta": {"background": "BACKGROUND_TRANSPARENT"}},
    "ROW-2": {"type": "ROW", "id": "ROW-2", "children": ["CHART-b", "CHART-p", "CHART-t"], "meta": {"background": "BACKGROUND_TRANSPARENT"}},
    "CHART-n": cell("n", charts["number"], 3, 20),
    "CHART-l": cell("l", charts["line"], 9, 40),
    "CHART-b": cell("b", charts["bar"], 4, 40),
    "CHART-p": cell("p", charts["pie"], 4, 40),
    "CHART-t": cell("t", charts["table"], 4, 40),
}
call("PUT", f"/api/v1/dashboard/{dash}", {
    "position_json": json.dumps(position),
    "json_metadata": json.dumps({"native_filter_configuration": [], "refresh_frequency": 0})}, token)
ensure_filter()
print("[superset] database, dataset, 5 charts, the dashboard and its Region filter created")
PY
