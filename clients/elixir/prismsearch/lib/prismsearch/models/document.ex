defmodule Prismsearch.Document do
  @moduledoc "A Prism document."
  defstruct [:id, fields: %{}]

  @type t :: %__MODULE__{
    id: String.t(),
    fields: map()
  }
end
