import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type FieldSchema } from '@/api/client'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`
}

function FieldTypeBadge({ field }: { field: FieldSchema }) {
  const variant = field.vector_dimensions ? 'default' : 'secondary'
  const label = field.vector_dimensions
    ? `vector[${field.vector_dimensions}]`
    : field.field_type
  return <Badge variant={variant}>{label}</Badge>
}

export function CollectionsPage() {
  const [selected, setSelected] = useState<string | null>(null)

  const collectionsQuery = useQuery({
    queryKey: ['collections'],
    queryFn: api.listCollections,
  })

  const schemaQuery = useQuery({
    queryKey: ['schema', selected],
    queryFn: () => api.getCollectionSchema(selected!),
    enabled: !!selected,
  })

  const statsQuery = useQuery({
    queryKey: ['stats', selected],
    queryFn: () => api.getCollectionStats(selected!),
    enabled: !!selected,
  })

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold">Collections</h2>

      <div className="grid gap-6 lg:grid-cols-[280px_1fr]">
        <Card>
          <CardHeader>
            <CardTitle>Collections</CardTitle>
            <CardDescription>Select a collection to inspect</CardDescription>
          </CardHeader>
          <CardContent>
            {collectionsQuery.isLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            ) : collectionsQuery.error ? (
              <p className="text-destructive">Failed to load collections</p>
            ) : collectionsQuery.data?.length === 0 ? (
              <p className="text-muted-foreground">No collections found</p>
            ) : (
              <div className="space-y-1">
                {collectionsQuery.data?.map((name) => (
                  <Button
                    key={name}
                    variant={selected === name ? 'secondary' : 'ghost'}
                    className="w-full justify-start"
                    onClick={() => setSelected(name)}
                  >
                    {name}
                  </Button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {selected && (
          <div className="space-y-6">
            <Card>
              <CardHeader>
                <CardTitle>{selected}</CardTitle>
                <CardDescription>Collection statistics</CardDescription>
              </CardHeader>
              <CardContent>
                {statsQuery.isLoading ? (
                  <Skeleton className="h-12 w-48" />
                ) : statsQuery.error ? (
                  <p className="text-destructive">Failed to load stats</p>
                ) : (
                  <div className="flex gap-8">
                    <div>
                      <p className="text-sm text-muted-foreground">Documents</p>
                      <p className="text-2xl font-bold">
                        {statsQuery.data?.document_count.toLocaleString()}
                      </p>
                    </div>
                    <div>
                      <p className="text-sm text-muted-foreground">Storage</p>
                      <p className="text-2xl font-bold">
                        {formatBytes(statsQuery.data?.storage_size_bytes ?? 0)}
                      </p>
                    </div>
                  </div>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle>Schema</CardTitle>
                <CardDescription>
                  Dynamic field definitions for this collection
                </CardDescription>
              </CardHeader>
              <CardContent>
                {schemaQuery.isLoading ? (
                  <Skeleton className="h-32 w-full" />
                ) : schemaQuery.error ? (
                  <p className="text-destructive">Failed to load schema</p>
                ) : schemaQuery.data?.fields.length === 0 ? (
                  <p className="text-muted-foreground">No fields defined</p>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Field</TableHead>
                        <TableHead>Type</TableHead>
                        <TableHead>Indexed</TableHead>
                        <TableHead>Stored</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {schemaQuery.data?.fields.map((field) => (
                        <TableRow key={field.name}>
                          <TableCell className="font-mono">{field.name}</TableCell>
                          <TableCell>
                            <FieldTypeBadge field={field} />
                          </TableCell>
                          <TableCell>
                            {field.indexed ? (
                              <Badge variant="outline">Yes</Badge>
                            ) : (
                              <span className="text-muted-foreground">No</span>
                            )}
                          </TableCell>
                          <TableCell>
                            {field.stored ? (
                              <Badge variant="outline">Yes</Badge>
                            ) : (
                              <span className="text-muted-foreground">No</span>
                            )}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                )}
              </CardContent>
            </Card>
          </div>
        )}
      </div>
    </div>
  )
}
