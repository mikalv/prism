"""Orchestration tests using in-memory fakes for IMAP + Prism (real StateStore)."""

from mailsync.config import Config
from mailsync.state import StateStore
from mailsync.sync import sync

RAW = b"From: a@b\nSubject: s\nMessage-ID: <%d@x>\n\nbody %d"


def _cfg(**over):
    env = {
        "MAILSYNC_IMAP_HOST": "h",
        "MAILSYNC_IMAP_USER": "u",
        "MAILSYNC_IMAP_PASSWORD": "p",
        "MAILSYNC_COLLECTION": "mail",
        "MAILSYNC_BATCH_SIZE": "2",
    }
    env.update(over)
    return Config.from_env(env)


class FakeSource:
    def __init__(self, folders):
        # folders: {name: (uidvalidity, [(uid, raw, flags), ...])}
        self._folders = folders

    def list_folders(self):
        return list(self._folders)

    def select_folder(self, name):
        # Track the current folder like a real IMAP SELECT does.
        self._current = self._folders[name]
        uidvalidity, _ = self._current
        if uidvalidity is None:
            raise RuntimeError("cannot select")
        return uidvalidity

    def search_since(self, watermark):
        return sorted(u for u, _, _ in self._current[1] if u > watermark)

    def fetch_messages(self, uids):
        by_uid = {u: (u, raw, flags) for u, raw, flags in self._current[1]}
        for u in uids:
            if u in by_uid:
                yield by_uid[u]


class FakePrism:
    def __init__(self, fail_uids=()):
        self.fail_uids = set(fail_uids)
        self.bulk_calls = 0
        self.created = False

    def ensure_collection(self, schema):
        self.created = True
        return True

    def bulk_index(self, collection, docs):
        self.bulk_calls += 1
        items = []
        for d in docs:
            status = 500 if d.fields["uid"] in self.fail_uids else 201
            items.append({"index": {"_id": d.doc_id, "status": status}})
        return {"took": 1, "errors": bool(self.fail_uids), "items": items}


def _msgs(*uids):
    return [(u, RAW % (u, u), ["\\Seen"]) for u in uids]


def test_happy_path_indexes_all_and_sets_watermark(tmp_path):
    src = FakeSource({"INBOX": (10, _msgs(1, 2, 3))})
    prism = FakePrism()
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, log=lambda *_: None)

    assert stats.messages_indexed == 3
    assert stats.messages_failed == 0
    assert state.get_watermark(cfg.account, "INBOX", 10) == 3


def test_partial_failure_stops_watermark_before_failed_uid(tmp_path):
    src = FakeSource({"INBOX": (10, _msgs(1, 2, 3, 4))})
    prism = FakePrism(fail_uids={3})   # uid 3 fails
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, log=lambda *_: None)

    # batch_size=2 -> [1,2] ok (wm=2), [3,4] -> 3 fails so wm not advanced past 2
    assert state.get_watermark(cfg.account, "INBOX", 10) == 2
    assert stats.messages_failed >= 1


def test_folder_isolation_one_bad_folder_does_not_abort(tmp_path):
    src = FakeSource({"BAD": (None, []), "INBOX": (10, _msgs(1, 2))})
    prism = FakePrism()
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, log=lambda *_: None)

    assert stats.folders_failed == 1
    assert stats.messages_indexed == 2
    assert state.get_watermark(cfg.account, "INBOX", 10) == 2


def test_incremental_only_fetches_above_watermark(tmp_path):
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()
    state.set_watermark(cfg.account, "INBOX", 10, last_uid=2)
    src = FakeSource({"INBOX": (10, _msgs(1, 2, 3, 4))})
    prism = FakePrism()

    stats = sync(cfg, src, prism, state, log=lambda *_: None)

    assert stats.messages_indexed == 2  # only uids 3,4
    assert state.get_watermark(cfg.account, "INBOX", 10) == 4


def test_dry_run_writes_nothing(tmp_path):
    src = FakeSource({"INBOX": (10, _msgs(1, 2))})
    prism = FakePrism()
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, dry_run=True, log=lambda *_: None)

    assert stats.messages_indexed == 2
    assert prism.bulk_calls == 0
    assert prism.created is False
    assert state.get_watermark(cfg.account, "INBOX", 10) == 0


def test_limit_caps_indexed_messages(tmp_path):
    src = FakeSource({"INBOX": (10, _msgs(1, 2, 3, 4, 5))})
    prism = FakePrism()
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, limit=3, log=lambda *_: None)

    assert stats.messages_indexed == 3


def test_only_folders_filters(tmp_path):
    src = FakeSource({"INBOX": (10, _msgs(1)), "Sent": (11, _msgs(9))})
    prism = FakePrism()
    state = StateStore(tmp_path / "s.db")
    cfg = _cfg()

    stats = sync(cfg, src, prism, state, only_folders=["Sent"], log=lambda *_: None)

    assert stats.messages_indexed == 1
    assert state.get_watermark(cfg.account, "Sent", 11) == 9
    assert state.get_watermark(cfg.account, "INBOX", 10) == 0
