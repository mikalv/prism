defmodule Prismsearch.FacadeTest do
  use ExUnit.Case, async: true

  describe "client/1" do
    test "creates a client struct" do
      client = Prismsearch.client(base_url: "http://test:3080")
      assert %Prismsearch.Client{} = client
      assert client.base_url == "http://test:3080"
    end
  end

  # Integration tests — only run when PRISM_TEST_URL is set
  if System.get_env("PRISM_TEST_URL") do
    @tag :integration
    describe "integration" do
      setup do
        client = Prismsearch.client(base_url: System.get_env("PRISM_TEST_URL"))
        %{client: client}
      end

      test "health/1", %{client: client} do
        assert {:ok, health} = Prismsearch.health(client)
        assert health["status"] == "ok"
      end

      test "list_collections/1", %{client: client} do
        assert {:ok, collections} = Prismsearch.list_collections(client)
        assert is_list(collections)
      end
    end
  end
end
