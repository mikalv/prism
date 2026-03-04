# prismsearch

Python client for Prism search engine.

## Install

```bash
pip install prismsearch
```

## Usage

```python
from prismsearch import Prismsearch, Query

client = Prismsearch("http://localhost:3080")

# Health check
health = client.health()

# Search
results = Query("products", "headphones").fields(["title"]).limit(5).execute(client)
for r in results.results:
    print(r.id, r.score, r.fields)
```
