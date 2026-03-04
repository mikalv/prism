defmodule Prismsearch.ClientTest do
  use ExUnit.Case, async: true

  test "creates client with base_url" do
    client = Prismsearch.Client.new(base_url: "http://localhost:3080")
    assert client.base_url == "http://localhost:3080"
    assert client.api_key == nil
  end

  test "creates client with api_key" do
    client = Prismsearch.Client.new(
      base_url: "http://localhost:3080",
      api_key: "test-key"
    )
    assert client.api_key == "test-key"
  end

  test "default base_url" do
    client = Prismsearch.Client.new()
    assert client.base_url == "http://localhost:3080"
  end
end
