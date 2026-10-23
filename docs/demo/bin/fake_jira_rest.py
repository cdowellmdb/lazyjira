#!/usr/bin/env python3
"""Fake Jira REST API for recording the README demo.

Serves `POST /rest/api/2/search`, which lazyjira reads its ticket lists, epics and their
children, sub-task parents and prefetched details from, `GET /rest/api/2/issue/KEY`, which
it reads a ticket opened in the detail view from, and moves: `GET` and `POST
/rest/api/2/issue/KEY/transitions`. It answers from the same made-up data as the fake `jira`
CLI next to it, which it imports, so the two never disagree. A move lasts until the server
stops, and needs no resolution.

Binds a free port on 127.0.0.1 and writes it to the file named by the first argument, for
record.sh to put in the demo's jira-cli config.
"""

import importlib.machinery
import importlib.util
import json
import os
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

here = os.path.dirname(os.path.abspath(__file__))
sys.dont_write_bytecode = True  # or importing `jira` leaves a __pycache__ in the repo
loader = importlib.machinery.SourceFileLoader("fake_jira_cli", os.path.join(here, "jira"))
cli = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
loader.exec_module(cli)

DEFAULT_PAGE_SIZE = 50

# Every ticket can move to each of these: transition id -> (name, status).
TRANSITIONS = {
    "11": ("Start Progress", "In Progress"),
    "21": ("Send to Review", "In Review"),
    "31": ("Block", "Blocked"),
    "41": ("Done", "Done"),
}

# Moves made while the server runs: key -> (status, when). Every read shows them.
moved = {}
listed = cli.all_tickets


def all_tickets():
    tickets = listed()
    for ticket in tickets:
        ticket["status"] = moved.get(ticket["key"], (ticket["status"],))[0]
    return tickets


cli.all_tickets = all_tickets  # run_query reads the module's all_tickets


def issue(ticket, wanted):
    """`ticket` with the fields in `wanted` (all of them when it's None), as Jira shapes it."""
    fields = cli.issue_payload(ticket)["fields"]
    if ticket["key"] in moved:
        fields["updated"] = moved[ticket["key"]][1]
    if wanted is not None:
        fields = {name: value for name, value in fields.items() if name in wanted}
    return {"key": ticket["key"], "fields": fields}


def search(body):
    """The page of issues `body` (jql, startAt, maxResults, fields) asks for."""
    matched = cli.run_query(body.get("jql", ""))
    start = int(body.get("startAt", 0))
    size = int(body.get("maxResults", DEFAULT_PAGE_SIZE))
    wanted = body.get("fields")
    issues = [issue(ticket, wanted) for ticket in matched[start : start + size]]
    return {"startAt": start, "maxResults": size, "total": len(matched), "issues": issues}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        url = urlparse(self.path)
        if re.fullmatch(r"/rest/api/2/issue/[A-Za-z0-9_-]+/transitions", url.path):
            offered = [
                {"id": id, "name": name, "to": {"name": status}, "fields": {}}
                for id, (name, status) in TRANSITIONS.items()
            ]
            return self.reply(200, {"transitions": offered})
        found = re.fullmatch(r"/rest/api/2/issue/([A-Za-z0-9_-]+)", url.path)
        if not found:
            return self.reply(404, {"errorMessages": [f"{url.path} isn't served by the demo"]})
        tickets = [t for t in cli.all_tickets() if t["key"] == found.group(1)]
        if not tickets:
            return self.reply(404, {"errorMessages": ["Issue does not exist."]})
        asked = parse_qs(url.query).get("fields")
        self.reply(200, issue(tickets[0], asked[0].split(",") if asked else None))

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        move = re.fullmatch(r"/rest/api/2/issue/([A-Za-z0-9_-]+)/transitions", self.path)
        if move:
            offered = TRANSITIONS.get(body.get("transition", {}).get("id"))
            if not offered:
                return self.reply(400, {"errorMessages": ["The demo has no such transition."]})
            _, status = offered
            when = time.strftime("%Y-%m-%dT%H:%M:%S.000+0000", time.gmtime())
            moved[move.group(1)] = (status, when)
            return self.reply(204, None)
        if self.path != "/rest/api/2/search":
            return self.reply(404, {"errorMessages": [f"{self.path} isn't served by the demo"]})
        self.reply(200, search(body))

    def reply(self, status, payload):
        data = b"" if payload is None else json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 0), Handler)
    with open(sys.argv[1], "w") as port_file:
        port_file.write(str(server.server_port))
    server.serve_forever()
