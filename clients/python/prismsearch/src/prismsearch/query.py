"""Pipe-style query builder for Prism searches."""

from __future__ import annotations
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from prismsearch.client import Prismsearch
    from prismsearch.models import SearchResults


class Query:
    """Chainable query builder.

    Usage::

        results = (
            Query("products", "wireless headphones")
            .fields(["title", "description"])
            .limit(20)
            .highlight(fields=["title"])
            .execute(client)
        )
    """

    def __init__(self, collection: str, query: str | None = None):
        self.collection = collection
        self._query = query
        self._vector: list[float] | None = None
        self._fields: list[str] = []
        self._limit: int = 10
        self._offset: int = 0
        self._merge_strategy: str | None = None
        self._text_weight: float | None = None
        self._vector_weight: float | None = None
        self._highlight: dict | None = None
        self._rerank: dict | None = None
        self._min_score: float | None = None
        self._score_function: str | None = None
        self._rrf_k: int | None = None
        self._aggregations: list[dict] = []

    def fields(self, fields: list[str]) -> Query:
        self._fields = fields
        return self

    def limit(self, n: int) -> Query:
        self._limit = n
        return self

    def offset(self, n: int) -> Query:
        self._offset = n
        return self

    def vector(self, vec: list[float]) -> Query:
        self._vector = vec
        return self

    def min_score(self, s: float) -> Query:
        self._min_score = s
        return self

    def score_function(self, expr: str) -> Query:
        self._score_function = expr
        return self

    def merge_strategy(self, s: str) -> Query:
        self._merge_strategy = s
        return self

    def text_weight(self, w: float) -> Query:
        self._text_weight = w
        return self

    def vector_weight(self, w: float) -> Query:
        self._vector_weight = w
        return self

    def rrf_k(self, k: int) -> Query:
        self._rrf_k = k
        return self

    def highlight(self, *, fields: list[str], pre_tag: str = "<em>", post_tag: str = "</em>", fragment_size: int = 150, number_of_fragments: int = 3) -> Query:
        self._highlight = {
            "fields": fields,
            "pre_tag": pre_tag,
            "post_tag": post_tag,
            "fragment_size": fragment_size,
            "number_of_fragments": number_of_fragments,
        }
        return self

    def aggregate(self, name: str, **kwargs: Any) -> Query:
        agg = {"name": name, **kwargs}
        self._aggregations.append(agg)
        return self

    def to_request_body(self) -> dict[str, Any]:
        body: dict[str, Any] = {"limit": self._limit}
        if self._query is not None:
            body["query"] = self._query
        if self._vector is not None:
            body["vector"] = self._vector
        if self._fields:
            body["fields"] = self._fields
        if self._offset > 0:
            body["offset"] = self._offset
        if self._merge_strategy:
            body["merge_strategy"] = self._merge_strategy
        if self._text_weight is not None:
            body["text_weight"] = self._text_weight
        if self._vector_weight is not None:
            body["vector_weight"] = self._vector_weight
        if self._highlight:
            body["highlight"] = self._highlight
        if self._rerank:
            body["rerank"] = self._rerank
        if self._min_score is not None:
            body["min_score"] = self._min_score
        if self._score_function:
            body["score_function"] = self._score_function
        if self._rrf_k is not None:
            body["rrf_k"] = self._rrf_k
        return body

    def to_aggregate_body(self) -> dict[str, Any]:
        body: dict[str, Any] = {
            "aggregations": self._aggregations,
            "scan_limit": self._limit,
        }
        if self._query:
            body["query"] = self._query
        return body

    def execute(self, client: Prismsearch) -> SearchResults:
        """Execute search and return typed results."""
        return client.search(self.collection, self.to_request_body())

    def execute_aggs(self, client: Prismsearch) -> dict:
        """Execute aggregation query."""
        return client.aggregate(self.collection, self.to_aggregate_body())
