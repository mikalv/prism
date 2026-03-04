# prismsearch-django

Django integration for Prism search engine.

## Install

```bash
pip install prismsearch-django
```

## Configuration

```python
# settings.py
INSTALLED_APPS = [
    ...
    "prismsearch_django",
]

PRISMSEARCH = {
    "URL": "http://localhost:3080",
    "API_KEY": None,
    "DEFAULT_COLLECTION": None,
}
```

## Usage

```python
from django.db import models
from prismsearch_django.mixins import SearchableModel, SearchField

class Product(SearchableModel):
    class PrismMeta:
        collection = "products"
        fields = [
            SearchField("title", indexed=True, stored=True, boost=2.0),
            SearchField("description", indexed=True, stored=True),
        ]

    title = models.CharField(max_length=200)
    description = models.TextField()

# Auto-sync via signals
# Management command: python manage.py prismsearch_reindex
# Search: Product.prism.search("headphones", limit=20)
```
