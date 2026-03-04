"""Pydantic v2 models for Prism API types."""

from __future__ import annotations
from typing import Any
from pydantic import BaseModel, Field


class SearchResult(BaseModel):
    id: str
    score: float
    fields: dict[str, Any] = Field(default_factory=dict)
    highlight: dict[str, list[str]] | None = None


class SearchResults(BaseModel):
    results: list[SearchResult] = Field(default_factory=list)
    total: int = 0


class HealthResponse(BaseModel):
    status: str
    version: str
    collections: int
    uptime_secs: int


class IndexResponse(BaseModel):
    indexed: int
    failed: int
    errors: list[dict[str, str]] = Field(default_factory=list)


class CollectionStatsResponse(BaseModel):
    collection: str = ""
    document_count: int = 0
    storage_bytes: int = 0


class SegmentInfo(BaseModel):
    id: str
    doc_count: int
    deleted_count: int
    size_bytes: int


class SegmentsInfo(BaseModel):
    segments: list[SegmentInfo] = Field(default_factory=list)
    total_docs: int = 0
    total_deleted: int = 0
    delete_ratio: float = 0.0


class OptimizeResult(BaseModel):
    segments_before: int
    segments_after: int
    merged: bool


class SuggestionEntry(BaseModel):
    term: str
    score: float
    doc_freq: int


class SuggestResponse(BaseModel):
    suggestions: list[SuggestionEntry] = Field(default_factory=list)
    did_you_mean: str | None = None


class BfsResponse(BaseModel):
    nodes: list[str] = Field(default_factory=list)
    count: int = 0


class ShortestPathResponse(BaseModel):
    path: list[str] | None = None
    length: int | None = None


class GraphStats(BaseModel):
    node_count: int = 0
    edge_count: int = 0


class PrismError(Exception):
    """Error from Prism API."""

    def __init__(self, status: int, message: str):
        self.status = status
        self.message = message
        super().__init__(f"Prism error ({status}): {message}")
