# prism-import

Import data from external search engines into Prism.

## Synopsis

```bash
prism-import <COMMAND>
```

## Commands

- [es](#es-elasticsearch-import) — Import from Elasticsearch

---

## es (Elasticsearch Import)

Import data from Elasticsearch using the scroll API.

```bash
prism-import es --source <URL> --index <NAME> [OPTIONS]
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--source <URL>` | required | Elasticsearch URL |
| `--index <NAME>` | required | Index name or pattern |
| `--target <NAME>` | index name | Target Prism collection name |
| `--user <USER>` | — | Username for basic auth |
| `--password <PASS>` | — | Password for basic auth |
| `--api-key <KEY>` | — | API key for authentication |
| `--batch-size <N>` | `1000` | Scroll API batch size |
| `--dry-run` | false | Only show schema, don't import |
| `--schema-out <FILE>` | — | Output schema to YAML file |

### Authentication

Three authentication methods:

1. **None** (default) — For local/unsecured clusters
2. **Basic auth** — Username and password
3. **API key** — Elasticsearch API key

```bash
# No auth
prism-import es --source http://localhost:9200 --index logs

# Basic auth
prism-import es --source https://elastic.example.com:9200 \
  --index logs \
  --user elastic \
  --password secret

# API key
prism-import es --source https://elastic.example.com:9200 \
  --index logs \
  --api-key "base64encodedkey=="
```

### Examples

#### Preview schema (dry run)

See what fields will be imported without transferring data:

```bash
prism-import es --source http://localhost:9200 --index products --dry-run
```

Output:

```
Connecting to http://localhost:9200...

Schema for 'products':
  Fields:
    - id: keyword
    - name: text
    - description: text
    - price: f64
    - category: keyword
    - created_at: date
    - embedding: vector (dims=384)

--dry-run specified, skipping import.
```

#### Export schema to file

Save schema for review or modification:

```bash
prism-import es --source http://localhost:9200 \
  --index products \
  --schema-out products-schema.yaml \
  --dry-run
```

#### Full import

```bash
prism-import es --source http://localhost:9200 \
  --index products \
  --target prism-products \
  --batch-size 2000
```

Output:

```
Connecting to http://localhost:9200...

Schema for 'products':
  Fields:
    - id: keyword
    - name: text
    - description: text
    - price: f64

Importing 125,430 documents...

⠸ [00:02:15] [████████████████████░░░░░░░░░░░░░░░░░░░░] 62715/125430 (463/s) ETA: 02:15

Done! Imported 125430 documents in 271.2s (0 failed)

Import complete: 125430 documents to collection 'prism-products'
```

#### Import with different target name

```bash
prism-import es --source http://localhost:9200 \
  --index logs-2024-* \
  --target archived-logs
```

### Type Mapping

Elasticsearch types are mapped to Prism types:

| Elasticsearch | Prism |
|---------------|-------|
| `text` | `text` |
| `keyword` | `keyword` |
| `long`, `integer`, `short`, `byte` | `i64` |
| `double`, `float`, `half_float` | `f64` |
| `boolean` | `bool` |
| `date` | `date` |
| `dense_vector` | `vector` |
| `object`, `nested` | `json` |
| Other | `unknown` |

### Handling Large Indexes

For very large indexes:

1. **Increase batch size** for throughput:
   ```bash
   --batch-size 5000
   ```

2. **Use index patterns** to split:
   ```bash
   # Import each month separately
   prism-import es --source ... --index logs-2024-01 --target logs-jan
   prism-import es --source ... --index logs-2024-02 --target logs-feb
   ```

3. **Monitor progress** — Progress bar shows ETA and docs/sec

### Error Handling

- **Network errors** — Retried automatically (up to 3 times)
- **Document errors** — Logged and counted, import continues
- **Schema errors** — Stops import, shows error

Failed documents are counted in the final summary:

```
Done! Imported 125400 documents in 271.2s (30 failed)
Warning: 30 documents failed
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success (even if some documents failed) |
| `1` | Connection or configuration error |

## See Also

- [prism-cli](prism-cli.md) — Management CLI
- [Schema Reference](../reference/schema.md) — Collection schema format
