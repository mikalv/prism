class Prism < Formula
  desc "Hybrid search engine combining full-text and vector search for AI/RAG applications"
  homepage "https://mikalv.github.io/prism/"
  version "0.6.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-darwin-aarch64.tar.gz"
      sha256 "1b9925c27eee3a484948f2faf618e66ecc954f5195d0f58db3d4a5884f133c8e"
    else
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-darwin-x86_64.tar.gz"
      sha256 "5d462f4340d00f01b1d95688e35bc17172ed1b3427b0476f739dfec2c431050a"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-linux-aarch64-static.tar.gz"
      sha256 "b8ec6a02ee546cb5957a0dafa41b83451390bafadb7dd841250cfa4aec787f9f"
    else
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-linux-x86_64-static.tar.gz"
      sha256 "300c0103f8d7d04b3015cb7433a1e3c356bbc028c99458f542b01a4d5fed670e"
    end
  end

  def install
    bin.install "prism-server"
    bin.install "prism"
    bin.install "prism-import"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/prism-server --version")
  end
end
