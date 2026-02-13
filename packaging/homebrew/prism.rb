class Prism < Formula
  desc "Hybrid search engine combining full-text and vector search for AI/RAG applications"
  homepage "https://mikalv.github.io/prism/"
  version "0.6.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-darwin-aarch64.tar.gz"
      sha256 "4feb65c12c17f9bc03a7ee6952a4ee4d39e8cbe7d51c72c083fed0c8217e3a5e"
    else
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-darwin-x86_64.tar.gz"
      sha256 "d09a2829e5b7e3aa33c8242bb6f83c5f849680181216256671d71034188cc6ee"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-linux-aarch64-static.tar.gz"
      sha256 "3c4109292029b269168cd72eb4e9a2d32b4ff7e6ef5dd8625f28354f7260ba8e"
    else
      url "https://github.com/mikalv/prism/releases/download/v#{version}/prism-v#{version}-linux-x86_64-static.tar.gz"
      sha256 "672500a943c702a9d153c7f4aff09540af402f6d63ff96cec61a9652799220ed"
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
