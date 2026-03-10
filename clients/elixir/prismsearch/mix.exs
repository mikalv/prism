defmodule Prismsearch.MixProject do
  use Mix.Project

  @version "0.1.0"
  @source_url "https://github.com/mikalv/prism"

  def project do
    [
      app: :prismsearch,
      version: @version,
      elixir: "~> 1.15",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      package: package(),
      description: "Elixir client for Prism — a high-performance hybrid search engine combining full-text (Tantivy/BM25) and vector search (HNSW) for AI/RAG applications. See https://github.com/mikalv/prism",
      source_url: @source_url,
      docs: docs()
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:req, "~> 0.5"},
      {:jason, "~> 1.4"},
      {:ex_doc, "~> 0.34", only: :dev, runtime: false}
    ]
  end

  defp package do
    [
      licenses: ["MIT"],
      links: %{
        "GitHub" => @source_url,
        "Prism Search Engine" => "https://github.com/mikalv/prism",
        "Documentation" => "https://mikalv.github.io/prism/",
        "Changelog" => "https://github.com/mikalv/prism/releases"
      },
      files: ~w(lib .formatter.exs mix.exs README* LICENSE*),
      maintainers: ["Mikael Voss"]
    ]
  end

  defp docs do
    [main: "Prismsearch", source_ref: "v#{@version}"]
  end
end
