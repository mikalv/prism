defmodule Prismsearch.Collection do
  @moduledoc "Collection metadata."
  defstruct [:name, :description, :document_count, :storage_bytes, :schema]
end
