#!/usr/bin/env python3
"""Re-import syslog + auth.log into prism via ES-compat _bulk API.
Restarts from scratch (index is currently empty)."""
import json, time, urllib.request, sys

PRISM = "http://127.0.0.1:3080"
BATCH = 2000

def parse_syslog_line(line, source_file):
    # RFC5424-ish: "Aug 14 10:26:07 host process[pid]: message" or ISO-timestamped
    parts = line.split(" ", 4)
    if len(parts) >= 5 and parts[0][:1].isdigit() and "-" in parts[0]:
        # ISO format: 2026-08-14T10:26:07.351+00:00 host proc[pid]: msg
        ts, host, proc = parts[0], parts[1], parts[2]
        msg = " ".join(parts[3:])
        pid = ""
        if "[" in proc and "]" in proc:
            proc, pid = proc.split("[", 1)
            pid = pid.split("]", 1)[0]
        return {"timestamp": ts, "hostname": host, "process": proc,
                "pid": pid, "message": msg.rstrip("\n"), "source_file": source_file}
    # classic syslog: "Aug 14 10:26:07 host proc[pid]: msg"
    if len(parts) >= 5:
        ts, day, tm, host, rest = parts[0], parts[1], parts[2], parts[3], parts[4]
        proc = rest.split(":", 1)[0]
        msg = rest.split(":", 1)[1] if ":" in rest else rest
        pid = ""
        if "[" in proc and "]" in proc:
            proc, pid = proc.split("[", 1)
            pid = pid.split("]", 1)[0]
        return {"timestamp": f"{ts} {day} {tm}", "hostname": host, "process": proc,
                "pid": pid, "message": msg.rstrip("\n"), "source_file": source_file}
    return None

def bulk_import(files):
    total_docs = 0
    t0 = time.time()
    for path in files:
        batch_meta, batch_docs = [], []
        try:
            f = open(path, "r", errors="replace")
        except OSError as e:
            print(f"SKIP {path}: {e}", flush=True)
            continue
        with f:
            for i, line in enumerate(f):
                doc = parse_syslog_line(line, path)
                if not doc:
                    continue
                doc_id = f"{path}-{i}"
                batch_meta.append(json.dumps({"index": {"_index": "syslog", "_id": doc_id}}))
                batch_docs.append(json.dumps(doc))
                if len(batch_docs) >= BATCH:
                    if flush(batch_meta, batch_docs):
                        total_docs += len(batch_docs)
                    batch_meta, batch_docs = [], []
        if batch_docs and flush(batch_meta, batch_docs):
            total_docs += len(batch_docs)
        print(f"done {path}: total={total_docs} elapsed={time.time()-t0:.0f}s", flush=True)

def flush(meta, docs):
    body = "\n".join(m + "\n" + d for m, d in zip(meta, docs)) + "\n"
    req = urllib.request.Request(
        f"{PRISM}/_bulk",
        data=body.encode("utf-8", "replace"),
        headers={"Content-Type": "application/x-ndjson"},
        method="POST")
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            r = json.loads(resp.read())
            if r.get("errors"):
                errs = [it for it in r.get("items", []) if "error" in it.get("index", {})]
                print(f"WARN: {len(errs)} errors in batch, first: {errs[:1]}", flush=True)
                return len(errs) < len(docs) // 2  # tolerate <50% failure
            return True
    except Exception as e:
        print(f"ERROR flushing batch: {e}", flush=True)
        time.sleep(2)
        return False

if __name__ == "__main__":
    bulk_import(["/var/log/syslog", "/var/log/auth.log"])
    print("COMPLETE", flush=True)
