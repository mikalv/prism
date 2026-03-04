defmodule Prismsearch.Error do
  @moduledoc "Error returned from Prism API."
  defexception [:status, :message]

  @type t :: %__MODULE__{
    status: integer() | nil,
    message: String.t()
  }

  @impl true
  def message(%__MODULE__{status: nil, message: msg}), do: "Prism error: #{msg}"
  def message(%__MODULE__{status: status, message: msg}), do: "Prism error (#{status}): #{msg}"
end
