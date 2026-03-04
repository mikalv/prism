defmodule Prismsearch.Client do
  @moduledoc "HTTP client for Prism search engine."

  defstruct [:base_url, :api_key, :req]

  @default_base_url "http://localhost:3080"

  @type t :: %__MODULE__{
    base_url: String.t(),
    api_key: String.t() | nil,
    req: Req.Request.t()
  }

  @doc "Create a new Prism client."
  def new(opts \\ []) do
    base_url = Keyword.get(opts, :base_url, @default_base_url)
    api_key = Keyword.get(opts, :api_key)
    timeout = Keyword.get(opts, :timeout, 30_000)

    headers = if api_key, do: [{"authorization", "Bearer #{api_key}"}], else: []

    req =
      Req.new(
        base_url: base_url,
        headers: headers,
        receive_timeout: timeout
      )

    %__MODULE__{base_url: base_url, api_key: api_key, req: req}
  end

  @doc false
  def get(%__MODULE__{req: req}, path, opts \\ []) do
    params = Keyword.get(opts, :params, [])

    case Req.get(req, url: path, params: params) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def post(%__MODULE__{req: req}, path, body) do
    case Req.post(req, url: path, json: body) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def put(%__MODULE__{req: req}, path, body) do
    case Req.put(req, url: path, json: body) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  @doc false
  def delete(%__MODULE__{req: req}, path) do
    case Req.delete(req, url: path) do
      {:ok, %Req.Response{status: status, body: body}} when status in 200..299 ->
        {:ok, body}
      {:ok, %Req.Response{status: status, body: body}} ->
        {:error, %Prismsearch.Error{status: status, message: error_message(body)}}
      {:error, reason} ->
        {:error, %Prismsearch.Error{status: nil, message: inspect(reason)}}
    end
  end

  defp error_message(body) when is_map(body), do: Map.get(body, "error", inspect(body))
  defp error_message(body) when is_binary(body), do: body
  defp error_message(body), do: inspect(body)
end
