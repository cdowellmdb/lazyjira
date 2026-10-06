#!/usr/bin/env python3
"""Fake Jira REST search for recording the README demo.

Serves `POST /rest/api/2/search`, the one endpoint lazyjira reads its ticket lists, epics and
their children, sub-task parents and details from. It answers from the same made-up data as the
fake `jira` CLI next to it, which it imports, so the two never disagree. Moves aren't served.

Binds a free port on 127.0.0.1 and writes it to the file named by the first argument, for
record.sh to put in the demo's jira-cli config.
"""

import importlib.machinery
import importlib.util
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

here = os.path.dirname(os.path.abspath(__file__))
sys.dont_write_bytecode = True  # or importing `jira` leaves a __pycache__ in the repo
loader = importlib.machinery.SourceFileLoader("fake_jira_cli", os.path.join(here, "jira"))
cli = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
loader.exec_module(cli)

DEFAULT_PAGE_SIZE = 50


def search(body):
    """The page of issues `body` (jql, startAt, maxResults, fields) asks for."""
    matched = cli.run_query(body.get("jql", ""))
    start = int(body.get("startAt", 0))
    size = int(body.get("maxResults", DEFAULT_PAGE_SIZE))
    wanted = body.get("fields")
    issues = []
    for ticket in matched[start : start + size]:
        # Every field a ticket has, as `issue view` shows it; the changelog is left out because
        # Jira sends it only on request.
        fields = cli.issue_payload(ticket)["fields"]
        if wanted is not None:
            fields = {name: value for name, value in fields.items() if name in wanted}
        issues.append({"key": ticket["key"], "fields": fields})
    return {"startAt": start, "maxResults": size, "total": len(matched), "issues": issues}


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/rest/api/2/search":
            return self.reply(404, {"errorMessages": [f"{self.path} isn't served by the demo"]})
        length = int(self.headers.get("Content-Length", 0))
        self.reply(200, search(json.loads(self.rfile.read(length) or b"{}")))

    def reply(self, status, payload):
        data = json.dumps(payload).encode()
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
