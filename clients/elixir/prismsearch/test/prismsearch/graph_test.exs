defmodule Prismsearch.GraphTest do
  use ExUnit.Case, async: true

  test "module exists and has expected functions" do
    Code.ensure_loaded!(Prismsearch.Graph)
    assert function_exported?(Prismsearch.Graph, :add_node, 3)
    assert function_exported?(Prismsearch.Graph, :get_node, 3)
    assert function_exported?(Prismsearch.Graph, :remove_node, 3)
    assert function_exported?(Prismsearch.Graph, :add_edge, 3)
    assert function_exported?(Prismsearch.Graph, :get_edges, 3)
    assert function_exported?(Prismsearch.Graph, :bfs, 3)
    assert function_exported?(Prismsearch.Graph, :shortest_path, 3)
    assert function_exported?(Prismsearch.Graph, :stats, 2)
  end
end
