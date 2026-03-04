from prismsearch.models import (
    SearchResult, SearchResults, HealthResponse,
    IndexResponse, CollectionStatsResponse, SuggestResponse,
    SegmentsInfo, OptimizeResult,
)


def test_search_result_from_dict():
    data = {"id": "doc-1", "score": 4.82, "fields": {"title": "Test"}}
    r = SearchResult.model_validate(data)
    assert r.id == "doc-1"
    assert r.score == 4.82
    assert r.fields["title"] == "Test"


def test_search_results():
    data = {
        "results": [{"id": "1", "score": 1.0, "fields": {}}],
        "total": 1,
    }
    rs = SearchResults.model_validate(data)
    assert rs.total == 1
    assert len(rs.results) == 1


def test_health_response():
    data = {"status": "ok", "version": "0.6.6", "collections": 4, "uptime_secs": 100}
    h = HealthResponse.model_validate(data)
    assert h.status == "ok"
    assert h.collections == 4


def test_index_response():
    data = {"indexed": 5, "failed": 0, "errors": []}
    r = IndexResponse.model_validate(data)
    assert r.indexed == 5


def test_segments_info():
    data = {
        "segments": [{"id": "s1", "doc_count": 100, "deleted_count": 2, "size_bytes": 5000}],
        "total_docs": 100,
        "total_deleted": 2,
        "delete_ratio": 0.02,
    }
    s = SegmentsInfo.model_validate(data)
    assert len(s.segments) == 1


def test_optimize_result():
    data = {"segments_before": 5, "segments_after": 1, "merged": True}
    o = OptimizeResult.model_validate(data)
    assert o.merged is True
