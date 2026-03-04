defmodule Prismsearch.ILM do
  @moduledoc "Index Lifecycle Management operations."

  alias Prismsearch.Client

  def list_policies(%Client{} = c), do: Client.get(c, "/_ilm/policy")
  def get_policy(%Client{} = c, name), do: Client.get(c, "/_ilm/policy/#{name}")

  def create_policy(%Client{} = c, name, config) when is_map(config) do
    Client.put(c, "/_ilm/policy/#{name}", config)
  end

  def delete_policy(%Client{} = c, name), do: Client.delete(c, "/_ilm/policy/#{name}")
  def status(%Client{} = c), do: Client.get(c, "/_ilm/status")
  def explain(%Client{} = c, index), do: Client.get(c, "/#{index}/_ilm/explain")
  def rollover(%Client{} = c, index), do: Client.post(c, "/#{index}/_rollover", %{})

  def move_phase(%Client{} = c, index, phase) do
    Client.post(c, "/#{index}/_ilm/move/#{phase}", %{})
  end

  def attach_policy(%Client{} = c, collection, policy) do
    Client.post(c, "/#{collection}/_ilm/attach", %{"policy" => policy})
  end

  def list_aliases(%Client{} = c), do: Client.get(c, "/_aliases")

  def update_aliases(%Client{} = c, actions) when is_list(actions) do
    Client.put(c, "/_aliases", %{"actions" => actions})
  end
end
