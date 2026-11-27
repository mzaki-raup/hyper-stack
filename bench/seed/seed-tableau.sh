#!/usr/bin/env bash
# Tableau mock: nothing to load - its projects, workbooks, views, data sources,
# extract tasks and jobs are in mock/tableau/server.py. This signs in (user and
# password, then the personal access token) and prints what the mock holds, so
# seed-all.sh reports the Analytics feed for Tableau like every other service.
SEED_NAME=tableau
source "$(dirname "$0")/lib.sh"
running tableau-mock || { log "not running - skipped"; exit 0; }

base="http://$H:${TABLEAU_MOCK_PORT:-8280}"
for i in $(seq 1 24); do
  curl -fsS "$base/api/3.23/serverinfo" >/dev/null 2>&1 && break
  sleep 2
done
curl -fsS "$base/api/3.23/serverinfo" >/dev/null || { warn "the mock did not answer"; exit 1; }

export TB_URL="$base" TB_SITE="${TABLEAU_MOCK_SITE:-hs-demo}"
python3 - <<'PY' || exit 1
import json, os, urllib.request

base, site = os.environ["TB_URL"], os.environ["TB_SITE"]


def call(path, body=None, token=None):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Accept": "application/json", "Content-Type": "application/json",
                 **({"X-Tableau-Auth": token} if token else {})})
    with urllib.request.urlopen(req) as r:
        return json.load(r)


for how, creds in (
    ("password", {"name": os.environ["TABLEAU_MOCK_USER"],
                  "password": os.environ["TABLEAU_MOCK_PASSWORD"]}),
    ("token", {"personalAccessTokenName": os.environ["TABLEAU_MOCK_PAT_NAME"],
               "personalAccessTokenSecret": os.environ["TABLEAU_MOCK_PAT_SECRET"]}),
):
    s = call("/api/3.23/auth/signin", {"credentials": {**creds, "site": {"contentUrl": site}}})
    c = s["credentials"]
    tok, sid = c["token"], c["site"]["id"]
    n = lambda path, key, item: len(call(f"/api/3.23/sites/{sid}/{path}", token=tok)[key][item])
    print(f"[tableau] signed in by {how}: {n('projects', 'projects', 'project')} projects, "
          f"{n('workbooks', 'workbooks', 'workbook')} workbooks, {n('views', 'views', 'view')} views, "
          f"{n('datasources', 'datasources', 'datasource')} data sources")
t = call(f"/api/3.23/sites/{sid}/tasks/extractRefreshes", token=tok)["tasks"]["task"]
j = call(f"/api/3.23/sites/{sid}/jobs", token=tok)["backgroundJobs"]["backgroundJob"]
print(f"[tableau] {len(t)} extract refresh tasks, {len(j)} jobs (data is built into the mock)")
PY
