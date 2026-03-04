defmodule Prismsearch.Graph do
  @moduledoc "Graph API operations for Prism."

  alias Prismsearch.Client

  def add_node(%Client{} = c, collection, node) when is_map(node) do
    Client.post(c, "/collections/#{collection}/graph/nodes", node)
  end

  def get_node(%Client{} = c, collection, id) do
    Client.get(c, "/collections/#{collection}/graph/nodes/#{id}")
  end

  def remove_node(%Client{} = c, collection, id) do
    Client.delete(c, "/collections/#{collection}/graph/nodes/#{id}")
  end

  def add_edge(%Client{} = c, collection, edge) when is_map(edge) do
    Client.post(c, "/collections/#{collection}/graph/edges", edge)
  end

  def get_edges(%Client{} = c, collection, node_id) do
    Client.get(c, "/collections/#{collection}/graph/nodes/#{node_id}/edges")
  end

  def bfs(%Client{} = c, collection, opts) do
    body = %{
      "start" => Keyword.fetch!(opts, :start),
      "edge_type" => Keyword.fetch!(opts, :edge_type),
      "max_depth" => Keyword.get(opts, :max_depth, 3)
    }
    Client.post(c, "/collections/#{collection}/graph/bfs", body)
  end

  def shortest_path(%Client{} = c, collection, opts) do
    body = %{
      "start" => Keyword.fetch!(opts, :start),
      "target" => Keyword.fetch!(opts, :target)
    }
    body = if Keyword.has_key?(opts, :edge_types),
      do: Map.put(body, "edge_types", Keyword.get(opts, :edge_types)),
      else: body
    Client.post(c, "/collections/#{collection}/graph/shortest-path", body)
  end

  def stats(%Client{} = c, collection) do
    Client.get(c, "/collections/#{collection}/graph/stats")
  end
end
