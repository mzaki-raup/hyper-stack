#!/usr/bin/env python3
"""A mock of Power BI Report Server's REST API v2.0 for the Analytics
activity's tests.

Power BI Report Server is a licensed Windows Server product with no public
container, so this answers the calls a read-only client makes
(docs/tools/analytics.md §4.10) from canned data, in the OData shapes the
reference gives (`{"value": [...]}` with `$top`, `$skip`, `$filter=contains(
Name,'x')` and `$count`): System (product and version), folders, catalogue
items, Power BI reports, paginated reports with an Export to a PNG picture and
CSV, KPIs, data sources (connection strings with a password in them, for the
client to mask), and cache refresh plans. A stand-in, not the product: HTTP
Basic only (no NTLM), checking only the credentials.

Environment: POWERBIRS_MOCK_USER / POWERBIRS_MOCK_PASSWORD, PORT.
"""
import base64
import json
import os
import re
import struct
import uuid
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlparse

USER = os.environ.get("POWERBIRS_MOCK_USER", "dev")
PASSWORD = os.environ.get("POWERBIRS_MOCK_PASSWORD", "change-me")
BASE = "/reports/api/v2.0"


def uid(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "hs-pbirs-mock/" + name))


def item(kind, name, path, parent, size=0, desc=""):
    return {"Id": uid(path), "Name": name, "Path": path, "Type": kind, "Description": desc,
            "Hidden": False, "Size": size, "ParentFolderId": uid(parent) if parent else None,
            "CreatedBy": "BENCH\\\\admin", "CreatedDate": "2026-03-02T09:00:00Z",
            "ModifiedBy": "BENCH\\\\admin", "ModifiedDate": "2026-11-02T10:15:00Z",
            "HasParameters": False, "ContentType": ""}


FOLDERS = [
    item("Folder", "Sales", "/Sales", "/"),
    item("Folder", "Operations", "/Operations", "/"),
]
POWERBI = [
    item("PowerBIReport", "Regional Sales", "/Sales/Regional Sales", "/Sales", 412_000),
    item("PowerBIReport", "Customer Churn", "/Sales/Customer Churn", "/Sales", 298_000),
]
PAGINATED = [
    item("Report", "Order Fulfilment", "/Operations/Order Fulfilment", "/Operations", 52_000),
    item("Report", "Backlog by Warehouse", "/Operations/Backlog by Warehouse", "/Operations", 41_000),
]
KPIS = [
    {**item("Kpi", "Revenue vs Goal", "/Sales/Revenue vs Goal", "/Sales"),
     "ValueFormat": "Currency", "Visualization": "Bar",
     "Data": {"Value": 1_284_000, "Goal": 1_500_000, "Status": 0, "TrendSet": [1.0, 1.1, 1.05, 1.28]}},
    {**item("Kpi", "Open Backlog", "/Operations/Open Backlog", "/Operations"),
     "ValueFormat": "Integer", "Visualization": "Line",
     "Data": {"Value": 121, "Goal": 100, "Status": -1, "TrendSet": [90, 95, 110, 121]}},
]
DATASOURCES = [
    {"Id": uid("ds-sql"), "Name": "Sales DW", "Path": "/Sales/Sales DW", "Type": "DataSource",
     "DataSourceType": "SQL", "IsEnabled": True,
     "ConnectionString": "Data Source=dw.bench.local;Initial Catalog=Sales;User ID=report_reader;Password=hunter2-not-real"},
    {"Id": uid("ds-xl"), "Name": "Targets", "Path": "/Sales/Targets", "Type": "DataSource",
     "DataSourceType": "OLEDB", "IsEnabled": True,
     "ConnectionString": "Provider=Microsoft.ACE.OLEDB.12.0;Data Source=\\\\\\\\files\\\\targets.xlsx"},
]
PLANS = [
    {"Id": uid("plan-1"), "Owner": "BENCH\\\\admin", "CatalogItemPath": "/Sales/Regional Sales",
     "Description": "Nightly refresh", "ScheduleDescription": "Every day at 02:00",
     "EventType": "TimedSubscription", "LastRunTime": "2026-11-03T02:00:12Z",
     "LastStatus": "Completed Data Refresh", "ModifiedDate": "2026-10-01T08:00:00Z"},
    {"Id": uid("plan-2"), "Owner": "BENCH\\\\admin", "CatalogItemPath": "/Sales/Customer Churn",
     "Description": "Hourly refresh", "ScheduleDescription": "Every hour",
     "EventType": "TimedSubscription", "LastRunTime": "2026-11-03T09:00:05Z",
     "LastStatus": "Failure: the data source could not be reached", "ModifiedDate": "2026-10-01T08:00:00Z"},
]
ALL = FOLDERS + POWERBI + PAGINATED + KPIS
CSV = "Warehouse,Open orders,Oldest (days)\nAmsterdam,41,3\nSingapore,17,1\nDallas,63,6\n"


def png(width=640, height=300):
    bars = [0.5, 0.75, 0.4, 0.9, 0.6]
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            colour = (253, 253, 250)
            slot, inside = divmod(x - 40, (width - 80) // len(bars))
            if 0 <= slot < len(bars) and 14 < inside < (width - 80) // len(bars) - 14:
                if height - 30 - int(bars[slot] * (height - 60)) < y < height - 30:
                    colour = (242, 200, 17)
            if y == height - 30 and 30 < x < width - 30:
                colour = (90, 90, 90)
            row += bytes(colour)
        rows.append(bytes(row))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))


PICTURE = png()


def odata(items, query, context):
    top = int(query.get("$top", ["100"])[0])
    skip = int(query.get("$skip", ["0"])[0])
    f = query.get("$filter", [""])[0]
    m = re.fullmatch(r"contains\(Name,\s*'(.*)'\)", f)
    if m:
        items = [i for i in items if m.group(1).lower() in i["Name"].lower()]
    out = {"@odata.context": f"{BASE}/$metadata#{context}", "value": items[skip:skip + top]}
    if query.get("$count", [""])[0] == "true":
        out["@odata.count"] = len(items)
    return out


class Handler(BaseHTTPRequestHandler):
    server_version = "PowerBIRSMock/1"

    def log_message(self, fmt, *args):
        pass

    def send(self, status, body=b"", kind="application/json", extra=None):
        if not isinstance(body, bytes):
            body = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def authed(self):
        auth = self.headers.get("Authorization", "")
        if not auth.lower().startswith("basic "):
            return False
        try:
            user, _, pw = base64.b64decode(auth[6:]).decode().partition(":")
        except Exception:
            return False
        return user.split("\\")[-1] == USER and pw == PASSWORD

    def do_GET(self):
        url = urlparse(self.path)
        path, query = unquote(url.path), parse_qs(url.query)
        if not self.authed():
            return self.send(401, {"error": {"code": "Unauthorized", "message": "Sign in required"}},
                             extra={"WWW-Authenticate": 'Basic realm="Report Server"'})
        if not path.startswith(BASE + "/"):
            return self.send(404, {"error": {"code": "NotFound", "message": "No such resource"}})
        rest = path[len(BASE) + 1:]
        if rest == "System":
            return self.send(200, {"@odata.context": f"{BASE}/$metadata#System/$entity",
                                   "ProductName": "Microsoft Power BI Report Server",
                                   "ProductVersion": "15.0.1115.172", "ProductType": "Enterprise"})
        lists = {"Folders": FOLDERS, "PowerBIReports": POWERBI, "Reports": PAGINATED, "Kpis": KPIS,
                 "DataSources": DATASOURCES, "CacheRefreshPlans": PLANS, "CatalogItems": ALL}
        if rest in lists:
            return self.send(200, odata(lists[rest], query, rest))
        m = re.fullmatch(r"Folders\(([^)]+)\)/CatalogItems", rest)
        if m:
            kids = [i for i in ALL if i["ParentFolderId"] == m.group(1)]
            return self.send(200, odata(kids, query, "CatalogItems"))
        m = re.fullmatch(r"(PowerBIReports|Reports|Kpis|Folders)\(([^)]+)\)", rest)
        if m:
            found = next((i for i in lists[m.group(1)] if i["Id"] == m.group(2)), None)
            return self.send(200, found) if found else self.send(
                404, {"error": {"code": "ItemNotFound", "message": "The item was not found"}})
        m = re.fullmatch(r"Reports\(([^)]+)\)/Export/(IMAGE|CSV)", rest)
        if m and any(i["Id"] == m.group(1) for i in PAGINATED):
            if m.group(2) == "IMAGE":
                return self.send(200, PICTURE, "image/png")
            return self.send(200, CSV.encode(), "text/csv")
        m = re.fullmatch(r"PowerBIReports\(([^)]+)\)/(Export/\w+|Content)", rest)
        if m:
            return self.send(400, {"error": {"code": "NotSupported",
                                             "message": "A Power BI report cannot be rendered by the server"}})
        self.send(404, {"error": {"code": "NotFound", "message": "No such resource"}})


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", int(os.environ.get("PORT", "8080"))), Handler).serve_forever()
