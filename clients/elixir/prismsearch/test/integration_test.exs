defmodule Prismsearch.IntegrationTest do
  use ExUnit.Case

  @moduletag :integration

  @test_collection "prismsearch_elixir_test_#{:rand.uniform(999_999)}"

  setup_all do
    case System.get_env("PRISM_TEST_URL") do
      nil ->
        IO.puts("Skipping integration tests: PRISM_TEST_URL not set")
        :ok

      url ->
        client = Prismsearch.client(base_url: url)
        %{client: client, url: url}
    end
  end

  @tag :integration
  test "full lifecycle", context do
    unless Map.has_key?(context, :client) do
      IO.puts("Skipping: no PRISM_TEST_URL")
      :ok
    else
      client = context.client

      # Health
      assert {:ok, health} = Prismsearch.health(client)
      assert health["status"] == "ok"

      # Create collection
      schema = %{
        "backends" => %{
          "text" => %{
            "fields" => [
              %{"name" => "title", "type" => "text", "stored" => true, "indexed" => true},
              %{"name" => "content", "type" => "text", "stored" => true, "indexed" => true}
            ]
          }
        }
      }

      assert {:ok, _} = Prismsearch.create_collection(client, @test_collection, schema)

      # Index documents
      docs = [
        %{"id" => "1", "title" => "Elixir Testing", "content" => "Integration test document"},
        %{"id" => "2", "title" => "Phoenix Framework", "content" => "Web framework for Elixir"}
      ]

      assert {:ok, _} = Prismsearch.index(client, @test_collection, docs)

      # Wait for indexing
      Process.sleep(500)

      # Search
      query = Prismsearch.Query.new(@test_collection, "Elixir")
      assert {:ok, results} = Prismsearch.search(client, query)
      assert results.total > 0

      # List collections
      assert {:ok, collections} = Prismsearch.list_collections(client)
      assert @test_collection in collections

      # Cleanup
      assert {:ok, _} = Prismsearch.delete_collection(client, @test_collection)
    end
  end
end
