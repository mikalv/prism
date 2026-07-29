"""CLI entrypoint for agentimport."""

from __future__ import annotations

import logging
import sys

import click

from agentimport.config import Config


def _setup_logging(verbose: bool) -> None:
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s %(levelname)-8s %(name)s — %(message)s",
        datefmt="%H:%M:%S",
    )


@click.group()
@click.option("--prism-url", envvar="PRISM_URL", default="http://localhost:3080", help="Prism server URL")
@click.option("--api-key", envvar="PRISM_API_KEY", default=None, help="Prism API key")
@click.option("--state-db", envvar="AGENTIMPORT_STATE_DB", default=None, help="Path to state database")
@click.option("-v", "--verbose", is_flag=True, help="Enable debug logging")
@click.pass_context
def main(ctx: click.Context, prism_url: str, api_key: str | None, state_db: str | None, verbose: bool) -> None:
    """agentimport — Import AI assistant conversations into Prism."""
    _setup_logging(verbose)
    config = Config(prism_url=prism_url, prism_api_key=api_key)
    if state_db:
        from pathlib import Path
        config.state_db = Path(state_db).expanduser()
    ctx.ensure_object(dict)
    ctx.obj["config"] = config


@main.command()
@click.option("--sources", default=None, help="Comma-separated list of sources (claude_code,codex,gemini,copilot,chatgpt)")
@click.option("--include-tool-results", is_flag=True, help="Include tool result content")
@click.option("--include-thinking", is_flag=True, help="Include thinking/reasoning blocks")
@click.option("--roles", default=None, help="Comma-separated roles to include (user,assistant,system,tool)")
@click.option("--max-chars", type=int, default=None, help="Max characters per message")
@click.option("--batch-size", type=int, default=100, help="Prism indexing batch size")
@click.pass_context
def run(ctx: click.Context, sources: str | None, include_tool_results: bool, include_thinking: bool,
        roles: str | None, max_chars: int | None, batch_size: int) -> None:
    """Run a single import cycle."""
    from agentimport.pipeline import Pipeline

    config: Config = ctx.obj["config"]
    config.include_tool_results = include_tool_results
    config.include_thinking = include_thinking
    config.max_chars = max_chars
    config.batch_size = batch_size
    if roles:
        config.roles = set(roles.split(","))

    source_list = sources.split(",") if sources else None

    pipeline = Pipeline(config)
    stats = pipeline.run(source_list)

    click.echo(f"\n✓ Import complete")
    click.echo(f"  Files: {stats.files_scanned} scanned, {stats.files_imported} imported, {stats.files_skipped} skipped")
    click.echo(f"  Messages indexed: {stats.messages_indexed}")
    click.echo(f"  Conversations indexed: {stats.conversations_indexed}")
    if stats.errors:
        click.echo(f"  Errors: {stats.errors}", err=True)
        sys.exit(1)


@main.command()
@click.option("--interval", type=int, default=300, help="Seconds between import cycles")
@click.option("--watch", is_flag=True, help="Use filesystem watcher for near-realtime imports")
@click.option("--sources", default=None, help="Comma-separated list of sources")
@click.option("--include-tool-results", is_flag=True)
@click.option("--include-thinking", is_flag=True)
@click.pass_context
def daemon(ctx: click.Context, interval: int, watch: bool, sources: str | None,
           include_tool_results: bool, include_thinking: bool) -> None:
    """Run as a long-lived daemon, periodically importing new conversations."""
    from agentimport.daemon import run_daemon

    config: Config = ctx.obj["config"]
    config.include_tool_results = include_tool_results
    config.include_thinking = include_thinking

    source_list = sources.split(",") if sources else None

    run_daemon(config, source_names=source_list, interval=interval, watch=watch)


@main.command()
@click.option("--apply", is_flag=True, help="Apply schemas to Prism (create collections)")
@click.pass_context
def schema(ctx: click.Context, apply: bool) -> None:
    """Show or apply Prism collection schemas."""
    import json
    from agentimport.prism import SCHEMAS_DIR

    for schema_file in sorted(SCHEMAS_DIR.glob("*.json")):
        schema_data = json.loads(schema_file.read_text())
        click.echo(f"\n── {schema_data.get('collection', schema_file.stem)} ──")
        click.echo(json.dumps(schema_data, indent=2))

    if apply:
        config: Config = ctx.obj["config"]
        from agentimport.prism import PrismClient
        with PrismClient(config.prism_url, config.prism_api_key) as prism:
            prism.ensure_collections()
        click.echo("\n✓ Schemas applied to Prism")


@main.command()
@click.pass_context
def stats(ctx: click.Context) -> None:
    """Show import statistics."""
    config: Config = ctx.obj["config"]

    # State DB stats
    from agentimport.state import StateDB
    with StateDB(config.state_db) as state:
        state_stats = state.get_stats()
        click.echo("── State DB ──")
        for source, count in sorted(state_stats.items()):
            click.echo(f"  {source}: {count} files imported")

    # Prism collection stats
    from agentimport.prism import PrismClient
    try:
        with PrismClient(config.prism_url, config.prism_api_key) as prism:
            prism_stats = prism.get_stats()
            click.echo("\n── Prism Collections ──")
            for collection, info in sorted(prism_stats.items()):
                click.echo(f"  {collection}: {info['documents']} docs ({info['storage_bytes']} bytes)")
    except Exception as e:
        click.echo(f"\n  Prism unavailable: {e}", err=True)


if __name__ == "__main__":
    main()
