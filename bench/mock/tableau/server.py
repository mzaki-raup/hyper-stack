#!/usr/bin/env python3
"""A mock of Tableau Server's REST API for the Analytics activity's tests.

Tableau Server is licensed and has no public container, so this answers the
calls a read-only client makes (docs/tools/analytics.md §4.8) from canned
data, in the shapes the REST API reference gives (JSON, as asked for with
`Accept: application/json`): serverinfo, auth/signin (user and password, or a
personal access token) and signout, projects, workbooks, a workbook's views,
data sources, a view's picture (PNG) and data (CSV), extract refresh tasks and
jobs. It is a stand-in, not the product: it checks nothing a real Server
would beyond the credentials and the auth token.

Environment: TABLEAU_MOCK_USER / TABLEAU_MOCK_PASSWORD, TABLEAU_MOCK_PAT_NAME /
TABLEAU_MOCK_PAT_SECRET, TABLEAU_MOCK_SITE (content URL of the one site besides
the default, which has an empty one), PORT.
"""
import json
import os
import re
import struct
import uuid
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

USER = os.environ.get("TABLEAU_MOCK_USER", "dev")
PASSWORD = os.environ.get("TABLEAU_MOCK_PASSWORD", "change-me")
PAT_NAME = os.environ.get("TABLEAU_MOCK_PAT_NAME", "hyperstack")
PAT_SECRET = os.environ.get("TABLEAU_MOCK_PAT_SECRET", "change-me")
SITE = os.environ.get("TABLEAU_MOCK_SITE", "hs-demo")
API_VERSION = "3.23"


def uid(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "hs-mock/" + name))


SITES = {"": uid("default-site"), SITE: uid("site-" + SITE)}
OWNER = {"id": uid("owner")}
PROJECTS = [
    {"id": uid("p-sales"), "name": "Sales", "description": "Revenue dashboards",
     "createdAt": "2026-01-12T09:00:00Z", "updatedAt": "2026-10-30T16:21:00Z", "owner": OWNER},
    {"id": uid("p-ops"), "name": "Operations", "description": "Fulfilment and support",
     "createdAt": "2026-02-02T09:00:00Z", "updatedAt": "2026-11-02T08:05:00Z", "owner": OWNER},
]
WORKBOOKS = [
    {"id": uid("w-regional"), "name": "Regional Sales", "contentUrl": "RegionalSales",
     "project": {"id": PROJECTS[0]["id"], "name": "Sales"}, "owner": OWNER,
     "createdAt": "2026-01-14T10:00:00Z", "updatedAt": "2026-11-01T07:30:00Z",
     "showTabs": "true", "size": "412"},
    {"id": uid("w-orders"), "name": "Order Fulfilment", "contentUrl": "OrderFulfilment",
     "project": {"id": PROJECTS[1]["id"], "name": "Operations"}, "owner": OWNER,
     "createdAt": "2026-02-05T10:00:00Z", "updatedAt": "2026-11-03T12:10:00Z",
     "showTabs": "false", "size": "250"},
]
VIEWS = [
    {"id": uid("v-overview"), "name": "Overview", "contentUrl": "RegionalSales/sheets/Overview",
     "workbook": {"id": WORKBOOKS[0]["id"]}, "owner": OWNER,
     "createdAt": "2026-01-14T10:00:00Z", "updatedAt": "2026-11-01T07:30:00Z"},
    {"id": uid("v-bymonth"), "name": "By Month", "contentUrl": "RegionalSales/sheets/ByMonth",
     "workbook": {"id": WORKBOOKS[0]["id"]}, "owner": OWNER,
     "createdAt": "2026-01-14T10:00:00Z", "updatedAt": "2026-11-01T07:30:00Z"},
    {"id": uid("v-backlog"), "name": "Backlog", "contentUrl": "OrderFulfilment/sheets/Backlog",
     "workbook": {"id": WORKBOOKS[1]["id"]}, "owner": OWNER,
     "createdAt": "2026-02-05T10:00:00Z", "updatedAt": "2026-11-03T12:10:00Z"},
]
DATASOURCES = [
    {"id": uid("d-orders"), "name": "Orders (extract)", "contentUrl": "OrdersExtract",
     "type": "sqlserver", "hasExtracts": "true",
     "project": {"id": PROJECTS[1]["id"], "name": "Operations"}, "owner": OWNER,
     "createdAt": "2026-02-01T10:00:00Z", "updatedAt": "2026-11-03T02:00:00Z"},
    {"id": uid("d-regions"), "name": "Regions", "contentUrl": "Regions", "type": "excel-direct",
     "hasExtracts": "false", "project": {"id": PROJECTS[0]["id"], "name": "Sales"},
     "owner": OWNER, "createdAt": "2026-01-10T10:00:00Z", "updatedAt": "2026-10-01T10:00:00Z"},
]
EXTRACT_TASKS = [
    {"extractRefresh": {"id": uid("t-orders"), "priority": 50, "consecutiveFailedCount": "0",
                        "type": "FullRefresh",
                        "schedule": {"id": uid("s-nightly"), "name": "Nightly", "frequency": "Daily"},
                        "datasource": {"id": DATASOURCES[0]["id"]}}},
    {"extractRefresh": {"id": uid("t-regional"), "priority": 50, "consecutiveFailedCount": "2",
                        "type": "IncrementalRefresh",
                        "schedule": {"id": uid("s-hourly"), "name": "Hourly", "frequency": "Hourly"},
                        "workbook": {"id": WORKBOOKS[0]["id"]}}},
]
JOBS = [
    {"id": uid("j1"), "mode": "Asynchronous", "type": "RefreshExtracts", "progress": "100",
     "finishCode": "0", "createdAt": "2026-11-03T02:00:00Z", "startedAt": "2026-11-03T02:00:02Z",
     "completedAt": "2026-11-03T02:04:10Z"},
    {"id": uid("j2"), "mode": "Asynchronous", "type": "RefreshExtracts", "progress": "100",
     "finishCode": "1", "createdAt": "2026-11-03T03:00:00Z", "startedAt": "2026-11-03T03:00:01Z",
     "completedAt": "2026-11-03T03:00:40Z",
     "statusNotes": {"statusNote": [{"type": "failure", "value": "Could not connect to the data source",
                                     "text": "Could not connect to the data source"}]}},
]
CSV = {
    "v-overview": "Region,Sales,Profit\nEast,120340,20110\nWest,98210,15800\nCentral,75120,9020\nSouth,64300,7410\n",
    "v-bymonth": "Month,Sales\n2026-07,31200\n2026-08,33850\n2026-09,36100\n2026-10,35400\n",
    "v-backlog": "Warehouse,Open orders,Oldest (days)\nAmsterdam,41,3\nSingapore,17,1\nDallas,63,6\n",
}
CSV = {uid(k): v for k, v in CSV.items()}
TOKENS = {}


def png(width=640, height=300):
    """A small bar-chart picture, so a view has something to show."""
    bars = [0.55, 0.8, 0.4, 0.95, 0.65, 0.3]
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            colour = (250, 250, 252)
            slot, inside = divmod(x - 40, (width - 80) // len(bars))
            if 0 <= slot < len(bars) and 12 < inside < (width - 80) // len(bars) - 12:
                if y > height - 30 - int(bars[slot] * (height - 60)) and y < height - 30:
                    colour = (78, 121, 167)
            if y == height - 30 and 30 < x < width - 30:
                colour = (90, 90, 90)
            row += bytes(colour)
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


PICTURE = png()


def page(items, key, query):
    size = int(query.get("pageSize", ["100"])[0])
    number = int(query.get("pageNumber", ["1"])[0])
    part = items[(number - 1) * size: number * size]
    return {"pagination": {"pageNumber": str(number), "pageSize": str(size),
                           "totalAvailable": str(len(items))}, key: part}


class Handler(BaseHTTPRequestHandler):
    server_version = "TableauMock/1"

    def log_message(self, fmt, *args):
        pass

    def send(self, status, body=b"", kind="application/json"):
        if not isinstance(body, bytes):
            body = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def fail(self, status, code, summary, detail):
        self.send(status, {"error": {"summary": summary, "detail": detail, "code": code}})

    def body_json(self):
        n = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(n) or b"{}") if n else {}

    def do_POST(self):
        path = urlparse(self.path).path
        m = re.fullmatch(r"/api/([\d.]+)/auth/(signin|signout)", path)
        if not m:
            return self.fail(404, "404000", "Resource Not Found", "No such call")
        if m.group(2) == "signout":
            TOKENS.pop(self.headers.get("X-Tableau-Auth"), None)
            return self.send(204)
        c = self.body_json().get("credentials", {})
        site = c.get("site", {}).get("contentUrl", "")
        ok = (c.get("name") == USER and c.get("password") == PASSWORD) or (
            c.get("personalAccessTokenName") == PAT_NAME
            and c.get("personalAccessTokenSecret") == PAT_SECRET)
        if not ok:
            return self.fail(401, "401001", "Signin Error",
                             "Invalid authentication credentials were provided.")
        if site not in SITES:
            return self.fail(401, "401002", "Signin Error",
                             "The site could not be found.")
        token = uuid.uuid4().hex
        TOKENS[token] = site
        self.send(200, {"credentials": {
            "site": {"id": SITES[site], "contentUrl": site},
            "user": {"id": OWNER["id"]}, "token": token}})

    def do_GET(self):
        url = urlparse(self.path)
        query = parse_qs(url.query)
        if re.fullmatch(r"/api/[\d.]+/serverinfo", url.path):
            return self.send(200, {"serverInfo": {
                "productVersion": {"value": "2024.2.0", "build": "20242.24.0716.1240"},
                "restApiVersion": API_VERSION}})
        m = re.fullmatch(r"/api/[\d.]+/sites/([^/]+)/(.+)", url.path)
        if not m:
            return self.fail(404, "404000", "Resource Not Found", "No such call")
        token = self.headers.get("X-Tableau-Auth")
        if token not in TOKENS:
            return self.fail(401, "401002", "Unauthorized Access",
                             "Invalid authentication credentials were provided.")
        rest = m.group(2)
        if rest == "projects":
            return self.send(200, {"projects": page(PROJECTS, "project", query)})
        if rest == "workbooks":
            return self.send(200, {"workbooks": page(WORKBOOKS, "workbook", query)})
        if rest == "views":
            return self.send(200, {"views": page(VIEWS, "view", query)})
        if rest == "datasources":
            return self.send(200, {"datasources": page(DATASOURCES, "datasource", query)})
        if rest == "tasks/extractRefreshes":
            return self.send(200, {"tasks": {"task": EXTRACT_TASKS}})
        if rest == "jobs":
            return self.send(200, {"backgroundJobs": {"backgroundJob": JOBS}})
        m = re.fullmatch(r"workbooks/([^/]+)/views", rest)
        if m:
            mine = [v for v in VIEWS if v["workbook"]["id"] == m.group(1)]
            return self.send(200, {"views": {"view": mine}})
        m = re.fullmatch(r"views/([^/]+)", rest)
        if m:
            # Get View: the view with its workbook's and project's names.
            for v in VIEWS:
                if v["id"] == m.group(1):
                    wb = next(w for w in WORKBOOKS if w["id"] == v["workbook"]["id"])
                    return self.send(200, {"view": dict(
                        v, workbook={"id": wb["id"], "name": wb["name"]}, project=wb["project"])})
        m = re.fullmatch(r"views/([^/]+)/(image|data)", rest)
        if m and any(v["id"] == m.group(1) for v in VIEWS):
            if m.group(2) == "image":
                return self.send(200, PICTURE, "image/png")
            return self.send(200, CSV[m.group(1)].encode(), "text/csv")
        self.fail(404, "404000", "Resource Not Found", "No such resource")


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
