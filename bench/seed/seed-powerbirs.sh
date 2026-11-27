#!/usr/bin/env bash
# Power BI Report Server mock: nothing to load - its folders, paginated and
# Power BI reports, KPIs, data sources and cache refresh plans are in
# mock/powerbirs/server.py. This signs in (Basic) and prints what the mock
# holds, so seed-all.sh reports the Analytics feed for Power BI Report Server
# like every other service.
SEED_NAME=powerbirs
source "$(dirname "$0")/lib.sh"
running powerbirs-mock || { log "not running - skipped"; exit 0; }

base="http://$H:${POWERBIRS_MOCK_PORT:-8480}/reports/api/v2.0"
auth="${POWERBIRS_MOCK_USER:-dev}:${POWERBIRS_MOCK_PASSWORD:-change-me}"
for i in $(seq 1 24); do
  curl -sS -o /dev/null -u "$auth" "$base/System" 2>/dev/null && break
  sleep 2
done
curl -fsS -u "$auth" "$base/System" >/dev/null || { warn "the mock did not answer or refused $POWERBIRS_MOCK_USER"; exit 1; }

export PB_URL="$base" PB_AUTH="$auth"
python3 - <<'PY' || exit 1
import base64, json, os, urllib.request

base, auth = os.environ["PB_URL"], os.environ["PB_AUTH"]


def get(path):
    r = urllib.request.Request(base + "/" + path, headers={
        "Authorization": "Basic " + base64.b64encode(auth.encode()).decode(),
        "Accept": "application/json"})
    return json.load(urllib.request.urlopen(r))


n = lambda path: len(get(path)["value"])
print(f"[powerbirs] {get('System')['ProductName']} {get('System')['ProductVersion']}: "
      f"{n('Folders')} folders, {n('Reports')} paginated reports, {n('PowerBIReports')} Power BI reports, "
      f"{n('Kpis')} KPIs, {n('DataSources')} data sources, {n('CacheRefreshPlans')} refresh plans "
      "(data is built into the mock)")
PY
