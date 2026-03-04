"""SearchableModel mixin for Django models."""

from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any

from prismsearch_django.conf import get_client


@dataclass
class SearchField:
    """Definition of a field to index in Prism."""

    name: str
    field_type: str = "text"
    indexed: bool = True
    stored: bool = True
    boost: float = 1.0


class PrismManager:
    """Descriptor that provides search operations on a model class."""

    def __init__(self, model_class: type):
        self.model_class = model_class

    def search(self, query: str, *, limit: int = 10, offset: int = 0, fields: list[str] | None = None) -> Any:
        """Search the collection for this model."""
        from prismsearch.query import Query

        meta = self.model_class.PrismMeta
        client = get_client()
        q = Query(meta.collection, query).limit(limit).offset(offset)
        if fields:
            q = q.fields(fields)
        return q.execute(client)

    def reindex(self, *, batch_size: int = 500) -> int:
        """Reindex all instances of this model."""
        meta = self.model_class.PrismMeta
        client = get_client()
        total = 0
        batch = []

        for obj in self.model_class.objects.all().iterator():
            doc = _build_document(obj, meta)
            batch.append(doc)
            if len(batch) >= batch_size:
                client.index(meta.collection, batch)
                total += len(batch)
                batch = []

        if batch:
            client.index(meta.collection, batch)
            total += len(batch)

        return total


def _build_document(obj: Any, meta: Any) -> dict[str, Any]:
    """Build a Prism document dict from a Django model instance."""
    fields = {}
    for sf in meta.fields:
        value = getattr(obj, sf.name, None)
        if value is not None:
            # Convert Django field values to JSON-serializable types
            if hasattr(value, "__float__"):
                value = float(value)
            elif hasattr(value, "isoformat"):
                value = value.isoformat()
            else:
                value = str(value)
            fields[sf.name] = value
    return {"id": str(obj.pk), "fields": fields}


class SearchableModel:
    """Mixin that adds Prism search capabilities to a Django model.

    Usage::

        class Product(SearchableModel, models.Model):
            class PrismMeta:
                collection = "products"
                fields = [
                    SearchField("title", indexed=True, stored=True, boost=2.0),
                    SearchField("description", indexed=True, stored=True),
                ]

            title = models.CharField(max_length=200)
            description = models.TextField()
    """

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if hasattr(cls, "PrismMeta"):
            cls.prism = PrismManager(cls)
