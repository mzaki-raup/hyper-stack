#!/usr/bin/env bash
# SAS Viya mock: nothing to load - its folders, reports, CAS libraries, tables
# and jobs are in mock/sasviya/server.py. This signs in through the OAuth
# client and prints what the mock holds, so seed-all.sh reports the Analytics
# feed for SAS Viya like every other service.
SEED_NAME=sasviya
source "$(dirname "$0")/lib.sh"
running sasviya-mock || { log "not running - skipped"; exit 0; }

base="http://$H:${SASVIYA_MOCK_PORT:-8380}"
export SV_URL="$base"
python3 - <<'PY' || exit 1
import base64, json, os, time, urllib.parse, urllib.request, urllib.error

base = os.environ["SV_URL"]
client = os.environ.get("SASVIYA_MOCK_CLIENT", "sas.cli")
secret = os.environ.get("SASVIYA_MOCK_CLIENT_SECRET", "")


def wait():
    for _ in range(24):
        try:
            urllib.request.urlopen(base + "/identities/users/@currentUser")
        except urllib.error.HTTPError:
            return True  # 401: it is up
        except Exception:
            time.sleep(2)
    return False


if not wait():
    raise SystemExit("[sasviya] the mock did not answer")
body = urllib.parse.urlencode({"grant_type": "password",
                               "username": os.environ["SASVIYA_MOCK_USER"],
                               "password": os.environ["SASVIYA_MOCK_PASSWORD"]}).encode()
req = urllib.request.Request(base + "/SASLogon/oauth/token", data=body, headers={
    "Authorization": "Basic " + base64.b64encode(f"{client}:{secret}".encode()).decode(),
    "Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"})
token = json.load(urllib.request.urlopen(req))["access_token"]


def get(path):
    r = urllib.request.Request(base + path, headers={
        "Authorization": "Bearer " + token, "Accept": "application/json"})
    return json.load(urllib.request.urlopen(r))


reports = get("/reports/reports")
libs = get("/casManagement/servers/cas-shared-default/caslibs")
jobs = get("/jobExecution/jobs")
count = lambda v: v.get("count", len(v.get("items", [])))
print(f"[sasviya] signed in as the OAuth client {client}: {count(reports)} reports, "
      f"{count(libs)} CAS libraries, {count(jobs)} jobs (data is built into the mock)")
PY
