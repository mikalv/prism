"""Prismsearch HTTP client."""

from __future__ import annotations
from typing import Any

import httpx

from prismsearch.models import (
    HealthResponse, IndexResponse, SearchResults,
    CollectionStatsResponse, SuggestResponse, SegmentsInfo,
    OptimizeResult, BfsResponse, ShortestPathResponse, GraphStats,
    PrismError,
)


class Prismsearch:
    """Synchronous Prism client.

    Usage::

        client = Prismsearch("http://localhost:3080")
        health = client.health()
    """

    def __init__(self, base_url: str = "http://localhost:3080", *, api_key: str | None = None, timeout: float = 30.0):
        headers = {}
        if api_key:
            headers["authorization"] = f"Bearer {api_key}"
        self._http = httpx.Client(base_url=base_url, headers=headers, timeout=timeout)

    def close(self) -> None:
        self._http.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    # -- Internal helpers --

    def _get(self, path: str, **kwargs) -> Any:
        resp = self._http.get(path, **kwargs)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    def _post(self, path: str, json: Any = None) -> tuple[int, Any]:
        resp = self._http.post(path, json=json)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.status_code, resp.json()

    def _put(self, path: str, json: Any = None) -> Any:
        resp = self._http.put(path, json=json)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    def _delete(self, path: str) -> Any:
        resp = self._http.delete(path)
        if resp.status_code >= 400:
            raise PrismError(resp.status_code, resp.text)
        return resp.json()

    # -- Health --

    def health(self) -> HealthResponse:
        return HealthResponse.model_validate(self._get("/health"))

    # -- Collections --

    def list_collections(self) -> list[str]:
        data = self._get("/admin/collections")
        return data.get("collections", data) if isinstance(data, dict) else data

    def create_collection(self, name: str, schema: dict) -> dict:
        return self._put(f"/collections/{name}", json=schema)

    def delete_collection(self, name: str) -> dict:
        return self._delete(f"/collections/{name}")

    def get_schema(self, collection: str) -> dict:
        return self._get(f"/collections/{collection}/schema")

    def get_stats(self, collection: str) -> CollectionStatsResponse:
        return CollectionStatsResponse.model_validate(self._get(f"/collections/{collection}/stats"))

    # -- Documents --

    def index(self, collection: str, documents: list[dict]) -> IndexResponse:
        _, data = self._post(f"/collections/{collection}/documents", json={"documents": documents})
        return IndexResponse.model_validate(data)

    def get_document(self, collection: str, doc_id: str) -> dict | None:
        return self._get(f"/collections/{collection}/documents/{doc_id}")

    # -- Search --

    def search(self, collection: str, body: dict) -> SearchResults:
        _, data = self._post(f"/collections/{collection}/search", json=body)
        return SearchResults.model_validate(data)

    # -- Aggregations --

    def aggregate(self, collection: str, body: dict) -> dict:
        _, data = self._post(f"/collections/{collection}/aggregate", json=body)
        return data

    # -- Suggest --

    def suggest(self, collection: str, *, prefix: str, field: str, size: int = 5, fuzzy: bool = False, max_distance: int = 2) -> SuggestResponse:
        body = {"prefix": prefix, "field": field, "size": size, "fuzzy": fuzzy, "max_distance": max_distance}
        _, data = self._post(f"/collections/{collection}/_suggest", json=body)
        return SuggestResponse.model_validate(data)

    # -- More Like This --

    def mlt(self, collection: str, *, like: dict | None = None, like_text: str | None = None, fields: list[str] | None = None, size: int = 10) -> SearchResults:
        body: dict[str, Any] = {"size": size}
        if like:
            body["like"] = like
        if like_text:
            body["like_text"] = like_text
        if fields:
            body["fields"] = fields
        _, data = self._post(f"/collections/{collection}/_mlt", json=body)
        return SearchResults.model_validate(data)

    # -- Multi-search --

    def multi_search(self, collections: list[str], *, query: str | None = None, vector: list[float] | None = None, limit: int = 10) -> dict:
        body: dict[str, Any] = {"collections": collections, "limit": limit}
        if query:
            body["query"] = query
        if vector:
            body["vector"] = vector
        _, data = self._post("/_msearch", json=body)
        return data

    # -- Segments & Optimize --

    def segments(self, collection: str) -> SegmentsInfo:
        return SegmentsInfo.model_validate(self._get(f"/collections/{collection}/segments"))

    def optimize(self, collection: str, *, max_segments: int | None = None) -> OptimizeResult:
        body = {"max_segments": max_segments} if max_segments else None
        _, data = self._post(f"/collections/{collection}/optimize", json=body)
        return OptimizeResult.model_validate(data)

    # -- Graph --

    class _GraphNamespace:
        def __init__(self, client: "Prismsearch"):
            self._c = client

        def add_node(self, collection: str, node: dict) -> None:
            self._c._post(f"/collections/{collection}/graph/nodes", json=node)

        def get_node(self, collection: str, node_id: str) -> dict:
            return self._c._get(f"/collections/{collection}/graph/nodes/{node_id}")

        def remove_node(self, collection: str, node_id: str) -> None:
            self._c._delete(f"/collections/{collection}/graph/nodes/{node_id}")

        def add_edge(self, collection: str, edge: dict) -> None:
            self._c._post(f"/collections/{collection}/graph/edges", json=edge)

        def get_edges(self, collection: str, node_id: str) -> list[dict]:
            return self._c._get(f"/collections/{collection}/graph/nodes/{node_id}/edges")

        def bfs(self, collection: str, *, start: str, edge_type: str, max_depth: int = 3) -> BfsResponse:
            _, data = self._c._post(f"/collections/{collection}/graph/bfs", json={"start": start, "edge_type": edge_type, "max_depth": max_depth})
            return BfsResponse.model_validate(data)

        def shortest_path(self, collection: str, *, start: str, target: str, edge_types: list[str] | None = None) -> ShortestPathResponse:
            body: dict[str, Any] = {"start": start, "target": target}
            if edge_types:
                body["edge_types"] = edge_types
            _, data = self._c._post(f"/collections/{collection}/graph/shortest-path", json=body)
            return ShortestPathResponse.model_validate(data)

        def stats(self, collection: str) -> GraphStats:
            return GraphStats.model_validate(self._c._get(f"/collections/{collection}/graph/stats"))

    @property
    def graph(self) -> _GraphNamespace:
        return self._GraphNamespace(self)

    # -- Stats --

    def cache_stats(self) -> dict:
        return self._get("/stats/cache")

    def server_info(self) -> dict:
        return self._get("/stats/server")
