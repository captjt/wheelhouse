#!/usr/bin/env python3
"""A made-up Linear API, for tests: it answers the hub's GraphQL queries
from a JSON file of issues, read again on each request, so a test changes
Linear by writing the file. Point the hub at it with WH_LINEAR_ENDPOINT and
any WH_LINEAR_KEY. scripts/build-e2e.sh uses it.

    fake-linear.py PORT ISSUES.json

Each issue: {key, title, team, labels, cycle_active, state_type, description,
branch}.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

port, path = int(sys.argv[1]), sys.argv[2]


def issues():
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return []


def node(i, full=False):
    n = {
        "id": "uuid-" + i["key"],
        "identifier": i["key"],
        "title": i.get("title", ""),
        "url": "https://linear.app/acme/issue/" + i["key"],
        "priorityLabel": "Medium",
        "updatedAt": "2026-11-18T00:00:00Z",
        "state": {"name": "Todo", "type": i.get("state_type", "unstarted")},
        "assignee": {"name": "You"},
        "team": {"key": i.get("team", "ENG")},
        "labels": {"nodes": [{"name": l} for l in i.get("labels", [])]},
        "cycle": {"isActive": True} if i.get("cycle_active") else None,
    }
    if full:
        n["description"] = i.get("description", "")
        n["branchName"] = i.get("branch", "")
        n["comments"] = {"nodes": []}
    return n


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        q, v = body.get("query", ""), body.get("variables") or {}
        if "issue(id" in q:
            found = [i for i in issues() if i["key"] == v.get("id")]
            data = {"issue": node(found[0], full=True) if found else None}
        elif "assignedIssues" in q:
            teams = v.get("teams")
            nodes = [node(i) for i in issues() if teams is None or i.get("team") in teams]
            data = {"viewer": {"assignedIssues": {"nodes": nodes}}}
        else:
            data = {}
        out = json.dumps({"data": data}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
