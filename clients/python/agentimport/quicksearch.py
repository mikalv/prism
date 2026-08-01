#!/usr/bin/env python3
"""quicksearch — Quick search across imported agent conversations in Prism.

Usage:
    uv run quicksearch.py "how to fix auth bug"
    uv run quicksearch.py "database migration" --source claude_code --limit 5
    uv run quicksearch.py "async pattern" --role assistant --project myapp
"""

from __future__ import annotations

import sys
from datetime import datetime

import click

from agentimport.prism import PrismClient, COLLECTION_MESSAGES


@click.command()
@click.argument("query")
@click.option("--prism-url", envvar="PRISM_URL", default="http://localhost:3080")
@click.option("--api-key", envvar="PRISM_API_KEY", default=None)
@click.option("--limit", "-n", default=10, help="Max results")
@click.option("--source", "-s", default=None, help="Filter by source (claude_code, codex, gemini, copilot, chatgpt)")
@click.option("--role", "-r", default=None, help="Filter by role (user, assistant, system, tool)")
@click.option("--project", "-p", default=None, help="Filter by project")
@click.option("--model", "-m", default=None, help="Filter by model")
@click.option("--content-type", "-t", default=None, help="Filter by content_type (message, tool_call, tool_result, thinking)")
@click.option("--json-output", "--json", is_flag=True, help="Output as JSON")
def quicksearch(
    query: str,
    prism_url: str,
    api_key: str | None,
    limit: int,
    source: str | None,
    role: str | None,
    project: str | None,
    model: str | None,
    content_type: str | None,
    json_output: bool,
) -> None:
    """Search across all imported AI assistant conversations."""
    filters = {}
    if source:
        filters["source"] = source
    if role:
        filters["role"] = role
    if project:
        filters["project"] = project
    if model:
        filters["model"] = model
    if content_type:
        filters["content_type"] = content_type

    with PrismClient(prism_url, api_key=api_key) as prism:
        try:
            data = prism.search_messages(query, limit=limit, **filters)
        except Exception as e:
            click.echo(f"Error: {e}", err=True)
            sys.exit(1)

    results = data.get("results", [])

    if json_output:
        import json
        click.echo(json.dumps(data, indent=2, default=str))
        return

    if not results:
        click.echo("No results found.")
        return

    click.echo(f"Found {data.get('total', len(results))} results:\n")

    for i, r in enumerate(results, 1):
        fields = r.get("fields", {})
        score = r.get("score", 0)
        source_name = fields.get("source", "?")
        role_name = fields.get("role", "?")
        ct = fields.get("content_type", "message")
        text = fields.get("text", "")
        ts = fields.get("ts", "")
        proj = fields.get("project", "")
        model_name = fields.get("model", "")
        conv_id = fields.get("conversation_id", "")

        # Header line
        header_parts = [f"#{i}", f"[{source_name}]", f"{role_name}"]
        if ct != "message":
            header_parts.append(f"({ct})")
        if model_name:
            header_parts.append(f"model={model_name}")
        header_parts.append(f"score={score:.2f}")
        click.echo(click.style(" ".join(header_parts), bold=True))

        # Meta line
        meta_parts = []
        if proj:
            meta_parts.append(f"project={proj}")
        if ts:
            meta_parts.append(f"ts={ts}")
        if conv_id:
            meta_parts.append(f"conv={conv_id[:12]}")
        if meta_parts:
            click.echo(click.style("  " + "  ".join(meta_parts), dim=True))

        # Text preview (first 3 lines, max 300 chars)
        preview = text[:300]
        lines = preview.split("\n")[:3]
        for line in lines:
            click.echo(f"  {line}")
        if len(text) > 300 or len(text.split("\n")) > 3:
            click.echo(click.style("  ...", dim=True))

        click.echo()


if __name__ == "__main__":
    quicksearch()
