"""Daemon mode — long-running import loop with optional filesystem watcher."""

from __future__ import annotations

import logging
import signal
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from agentimport.config import Config

logger = logging.getLogger(__name__)


class _Shutdown:
    """Graceful shutdown handler for SIGTERM/SIGINT."""

    def __init__(self) -> None:
        self.requested = False
        signal.signal(signal.SIGTERM, self._handler)
        signal.signal(signal.SIGINT, self._handler)

    def _handler(self, signum, frame) -> None:
        logger.info("Shutdown requested (signal %d)", signum)
        self.requested = True


def run_daemon(
    config: Config,
    *,
    source_names: list[str] | None = None,
    interval: int = 300,
    watch: bool = False,
) -> None:
    """Run the import pipeline in a loop with backoff on errors.

    Args:
        config: Import configuration.
        source_names: Optional list of source names to import from.
        interval: Seconds between import cycles.
        watch: If True, use filesystem watcher for near-realtime imports.
    """
    from agentimport.pipeline import Pipeline

    shutdown = _Shutdown()
    pipeline = Pipeline(config)
    backoff = 1
    max_backoff = 300

    logger.info("Daemon starting — interval=%ds, watch=%s", interval, watch)

    if watch:
        _run_with_watch(pipeline, config, source_names, shutdown)
    else:
        _run_polling(pipeline, source_names, interval, shutdown, backoff, max_backoff)

    logger.info("Daemon stopped")


def _run_polling(
    pipeline,
    source_names: list[str] | None,
    interval: int,
    shutdown: _Shutdown,
    backoff: int,
    max_backoff: int,
) -> None:
    """Simple polling loop."""
    while not shutdown.requested:
        try:
            stats = pipeline.run(source_names)
            logger.info("Cycle complete: %s", stats)
            backoff = 1  # Reset on success
        except Exception:
            logger.exception("Import cycle failed, backing off %ds", backoff)
            time.sleep(backoff)
            backoff = min(backoff * 2, max_backoff)
            continue

        # Sleep in small increments so we can respond to shutdown quickly
        for _ in range(interval):
            if shutdown.requested:
                break
            time.sleep(1)


def _run_with_watch(
    pipeline,
    config: Config,
    source_names: list[str] | None,
    shutdown: _Shutdown,
) -> None:
    """Filesystem watcher mode using watchdog (optional dependency)."""
    try:
        from watchdog.events import FileSystemEventHandler
        from watchdog.observers import Observer
    except ImportError:
        logger.error("watchdog not installed. Install with: pip install agentimport[daemon]")
        return

    class _Handler(FileSystemEventHandler):
        def __init__(self):
            self.pending = False

        def on_modified(self, event):
            if not event.is_directory:
                self.pending = True

        def on_created(self, event):
            if not event.is_directory:
                self.pending = True

    handler = _Handler()
    observer = Observer()

    # Watch source directories
    from agentimport.sources.base import get_all_sources, get_source_by_name

    if source_names:
        sources = [s for n in source_names if (s := get_source_by_name(n))]
    else:
        sources = get_all_sources()

    watched = 0
    for source in sources:
        source_cfg = config.sources.get(source.name)
        roots = source_cfg.roots if source_cfg and source_cfg.roots else source.default_roots()
        for root in roots:
            if root.exists():
                observer.schedule(handler, str(root), recursive=True)
                watched += 1
                logger.info("Watching %s for %s", root, source.name)

    if not watched:
        logger.warning("No directories to watch — falling back to polling")
        _run_polling(pipeline, source_names, 60, shutdown, 1, 300)
        return

    observer.start()

    # Do an initial import
    try:
        pipeline.run(source_names)
    except Exception:
        logger.exception("Initial import failed")

    try:
        while not shutdown.requested:
            if handler.pending:
                handler.pending = False
                time.sleep(2)  # Debounce
                try:
                    pipeline.run(source_names)
                except Exception:
                    logger.exception("Watch-triggered import failed")
            time.sleep(1)
    finally:
        observer.stop()
        observer.join()
