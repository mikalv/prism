defmodule Prismsearch.SearchResult do
  @moduledoc "A single search result."
  defstruct [:id, :score, fields: %{}, highlight: nil]

  @type t :: %__MODULE__{
    id: String.t(),
    score: float(),
    fields: map(),
    highlight: map() | nil
  }
end

defmodule Prismsearch.SearchResults do
  @moduledoc "Search results container."
  defstruct [results: [], total: 0]

  @type t :: %__MODULE__{
    results: [Prismsearch.SearchResult.t()],
    total: non_neg_integer()
  }

  def from_map(map) when is_map(map) do
    results =
      (map["results"] || [])
      |> Enum.map(fn r ->
        %Prismsearch.SearchResult{
          id: r["id"],
          score: r["score"],
          fields: r["fields"] || %{},
          highlight: r["highlight"]
        }
      end)

    %__MODULE__{results: results, total: map["total"] || 0}
  end
end
