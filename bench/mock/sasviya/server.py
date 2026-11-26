#!/usr/bin/env python3
"""A mock of SAS Viya's REST API for the Analytics activity's tests.

SAS Viya is licensed and has no public container, so this answers the calls a
read-only client makes (docs/tools/analytics.md §4.9) from canned data, in the
shapes SAS's REST API reference gives: SAS Logon's OAuth token (password and
refresh-token grants, for a registered client), the signed-in user, folders and
their members, reports, the Report Images service (a job that is `running` on
its first read and `completed` on the next, then the picture), CAS libraries,
tables, columns and a row preview, and recent jobs. A stand-in, not the
product: it checks only the client, the credentials and the bearer token.

Environment: SASVIYA_MOCK_USER / SASVIYA_MOCK_PASSWORD, SASVIYA_MOCK_CLIENT /
SASVIYA_MOCK_CLIENT_SECRET (the OAuth client; empty secret by default, like
`sas.cli`), SASVIYA_MOCK_TOKEN (a fixed access token that is also accepted),
PORT.
"""
import base64
import json
import os
import re
import struct
import uuid
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

USER = os.environ.get("SASVIYA_MOCK_USER", "sasdemo")
PASSWORD = os.environ.get("SASVIYA_MOCK_PASSWORD", "change-me")
CLIENT = os.environ.get("SASVIYA_MOCK_CLIENT", "sas.cli")
CLIENT_SECRET = os.environ.get("SASVIYA_MOCK_CLIENT_SECRET", "")
FIXED_TOKEN = os.environ.get("SASVIYA_MOCK_TOKEN", "")


def uid(name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "hs-sas-mock/" + name))


ROOT = uid("root")
FOLDERS = {
    ROOT: {"id": ROOT, "name": "SAS Content", "parentFolderUri": None},
    uid("f-public"): {"id": uid("f-public"), "name": "Public", "parentFolderUri": ROOT},
    uid("f-sales"): {"id": uid("f-sales"), "name": "Sales", "parentFolderUri": uid("f-public")},
    uid("f-my"): {"id": uid("f-my"), "name": "My Folder", "parentFolderUri": ROOT},
}
REPORTS = [
    {"id": uid("r-revenue"), "name": "Revenue by Region", "createdBy": USER,
     "creationTimeStamp": "2026-06-01T09:00:00Z", "modifiedTimeStamp": "2026-11-02T10:15:00Z",
     "folder": uid("f-sales"), "sections": 2},
    {"id": uid("r-churn"), "name": "Customer Churn", "createdBy": USER,
     "creationTimeStamp": "2026-07-10T09:00:00Z", "modifiedTimeStamp": "2026-10-20T14:40:00Z",
     "folder": uid("f-sales"), "sections": 1},
    {"id": uid("r-notes"), "name": "Scratch Report", "createdBy": USER,
     "creationTimeStamp": "2026-09-01T09:00:00Z", "modifiedTimeStamp": "2026-09-01T09:00:00Z",
     "folder": uid("f-my"), "sections": 1},
]
SERVER = "cas-shared-default"
CASLIBS = {"Public": ["ORDERS", "CUSTOMERS"], "Samples": ["CARS"]}
COLUMNS = {
    "ORDERS": [("OrderID", "double"), ("Region", "varchar"), ("Amount", "double")],
    "CUSTOMERS": [("CustomerID", "double"), ("Name", "varchar"), ("Churned", "double")],
    "CARS": [("Make", "varchar"), ("Model", "varchar"), ("MSRP", "double")],
}
ROWS = {
    "ORDERS": [[1, "East", 120.5], [2, "West", 98.0], [3, "East", 75.25], [4, "South", 64.3]],
    "CUSTOMERS": [[10, "Ada", 0], [11, "Grace", 1], [12, "Linus", 0]],
    "CARS": [["Audi", "A4", 32000], ["Volvo", "V60", 41000]],
}
JOBS = [
    {"id": uid("j1"), "name": "Nightly load", "state": "completed",
     "creationTimeStamp": "2026-11-03T02:00:00Z", "endTimeStamp": "2026-11-03T02:04:00Z"},
    {"id": uid("j2"), "name": "Churn scoring", "state": "failed",
     "creationTimeStamp": "2026-11-03T03:00:00Z", "endTimeStamp": "2026-11-03T03:00:30Z",
     "results": {"message": "ERROR: Table CUSTOMERS_SCORE not found."}},
]
TOKENS = {}
REFRESH = {}
IMAGE_JOBS = {}


def png(width=640, height=300):
    bars = [0.4, 0.7, 0.55, 0.9, 0.35]
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            colour = (252, 252, 250)
            slot, inside = divmod(x - 40, (width - 80) // len(bars))
            if 0 <= slot < len(bars) and 14 < inside < (width - 80) // len(bars) - 14:
                if height - 30 - int(bars[slot] * (height - 60)) < y < height - 30:
                    colour = (0, 121, 193)
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


def listing(items, query, key="items"):
    start = int(query.get("start", ["0"])[0])
    limit = int(query.get("limit", ["20"])[0])
    return {key: items[start:start + limit], "count": len(items), "start": start, "limit": limit}


def report_json(r):
    return {"id": r["id"], "name": r["name"], "createdBy": r["createdBy"],
            "creationTimeStamp": r["creationTimeStamp"], "modifiedTimeStamp": r["modifiedTimeStamp"],
            "links": [{"rel": "self", "uri": f"/reports/reports/{r['id']}"}]}


class Handler(BaseHTTPRequestHandler):
    server_version = "SasViyaMock/1"

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

    def authed(self):
        auth = self.headers.get("Authorization", "")
        token = auth[7:] if auth.lower().startswith("bearer ") else ""
        return token in TOKENS or (FIXED_TOKEN and token == FIXED_TOKEN)

    def client_ok(self):
        auth = self.headers.get("Authorization", "")
        if not auth.lower().startswith("basic "):
            return False
        try:
            cid, _, secret = base64.b64decode(auth[6:]).decode().partition(":")
        except Exception:
            return False
        return cid == CLIENT and secret == CLIENT_SECRET

    def do_POST(self):
        path = urlparse(self.path).path
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        if path == "/SASLogon/oauth/token":
            if not self.client_ok():
                return self.send(401, {"error": "unauthorized",
                                       "error_description": "Bad credentials"})
            form = parse_qs(raw.decode())
            grant = form.get("grant_type", [""])[0]
            if grant == "password":
                ok = (form.get("username", [""])[0] == USER
                      and form.get("password", [""])[0] == PASSWORD)
            elif grant == "refresh_token":
                ok = form.get("refresh_token", [""])[0] in REFRESH
            else:
                return self.send(400, {"error": "unsupported_grant_type"})
            if not ok:
                return self.send(401, {"error": "unauthorized",
                                       "error_description": "Bad credentials"})
            token, refresh = uuid.uuid4().hex, uuid.uuid4().hex
            TOKENS[token] = USER
            REFRESH[refresh] = USER
            return self.send(200, {"access_token": token, "token_type": "bearer",
                                   "refresh_token": refresh, "expires_in": 43199,
                                   "scope": "openid"})
        if not self.authed():
            return self.send(401, {"errorCode": 401, "message": "Unauthorized"})
        if path == "/reportImages/jobs":
            req = json.loads(raw or b"{}")
            job = uuid.uuid4().hex
            IMAGE_JOBS[job] = {"reads": 0, "report": req.get("reportUri", "")}
            return self.send(201, {"id": job, "state": "pending"})
        self.send(404, {"errorCode": 404, "message": "No such resource"})

    def do_GET(self):
        url = urlparse(self.path)
        path, query = url.path, parse_qs(url.query)
        if not self.authed():
            return self.send(401, {"errorCode": 401, "message": "Unauthorized"})
        if path == "/identities/users/@currentUser":
            return self.send(200, {"id": USER, "name": "SAS Demo", "providerId": "local"})
        if path == "/folders/folders/@myFolder":
            return self.send(200, FOLDERS[uid("f-my")])
        m = re.fullmatch(r"/folders/folders/([^/]+)(/members)?", path)
        if m:
            fid = ROOT if m.group(1) == "@rootFolder" else m.group(1)
            if fid not in FOLDERS:
                return self.send(404, {"errorCode": 404, "message": "Folder not found"})
            if not m.group(2):
                return self.send(200, FOLDERS[fid])
            kids = [{"id": f["id"], "name": f["name"], "type": "child", "contentType": "folder",
                     "uri": f"/folders/folders/{f['id']}"}
                    for f in FOLDERS.values() if f["parentFolderUri"] == fid]
            kids += [{"id": r["id"], "name": r["name"], "type": "child", "contentType": "report",
                      "uri": f"/reports/reports/{r['id']}",
                      "modifiedTimeStamp": r["modifiedTimeStamp"]}
                     for r in REPORTS if r["folder"] == fid]
            return self.send(200, listing(kids, query))
        if path == "/reports/reports":
            found = REPORTS
            f = query.get("filter", [""])[0]
            mm = re.fullmatch(r"contains\(name,'(.*)'\)", f)
            if mm:
                found = [r for r in REPORTS if mm.group(1).lower() in r["name"].lower()]
            return self.send(200, listing([report_json(r) for r in found], query))
        m = re.fullmatch(r"/reports/reports/([^/]+)", path)
        if m:
            r = next((r for r in REPORTS if r["id"] == m.group(1)), None)
            return self.send(200, report_json(r)) if r else self.send(
                404, {"errorCode": 404, "message": "Report not found"})
        m = re.fullmatch(r"/reportImages/jobs/([^/]+)", path)
        if m and m.group(1) in IMAGE_JOBS:
            job = IMAGE_JOBS[m.group(1)]
            job["reads"] += 1
            if job["reads"] < 2:
                return self.send(200, {"id": m.group(1), "state": "running"})
            return self.send(200, {"id": m.group(1), "state": "completed", "images": [
                {"sectionIndex": 0, "links": [{"rel": "image", "type": "image/png",
                 "href": f"/reportImages/jobs/{m.group(1)}/images/0"}]}]})
        m = re.fullmatch(r"/reportImages/jobs/([^/]+)/images/0", path)
        if m and m.group(1) in IMAGE_JOBS:
            return self.send(200, PICTURE, "image/png")
        if path == "/casManagement/servers":
            return self.send(200, listing([{"name": SERVER}], query))
        if path == f"/casManagement/servers/{SERVER}/caslibs":
            return self.send(200, listing([{"name": n} for n in CASLIBS], query))
        m = re.fullmatch(rf"/casManagement/servers/{SERVER}/caslibs/([^/]+)/tables", path)
        if m and m.group(1) in CASLIBS:
            return self.send(200, listing(
                [{"name": t, "rowCount": len(ROWS[t]), "columnCount": len(COLUMNS[t])}
                 for t in CASLIBS[m.group(1)]], query))
        m = re.fullmatch(rf"/casManagement/servers/{SERVER}/caslibs/([^/]+)/tables/([^/]+)/columns", path)
        if m and m.group(2) in COLUMNS:
            return self.send(200, listing([{"name": n, "type": t} for n, t in COLUMNS[m.group(2)]], query))
        m = re.fullmatch(rf"/casRowSets/servers/{SERVER}/caslibs/([^/]+)/tables/([^/]+)/rows", path)
        if m and m.group(2) in ROWS:
            return self.send(200, listing([{"cells": r} for r in ROWS[m.group(2)]], query))
        if path == "/jobExecution/jobs":
            return self.send(200, listing(JOBS, query))
        self.send(404, {"errorCode": 404, "message": "No such resource"})


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", int(os.environ.get("PORT", "8080"))), Handler).serve_forever()
