class MacCleaner < Formula
  desc "CleanMyMac-style CLI for junk, clutter, and duplicate cleanup on macOS"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.0.2"
  url "https://github.com/abansod/mac-cleaner/archive/refs/tags/v0.0.2.tar.gz"
  sha256 "0de8f641150a6e25f29a62060e129f3bf9fb2ba51574023e4bf7cf17ddb604ac"
  license "MIT"

  depends_on :macos
  # Source-archive installs still compile; dropped when url points at the universal binary tarball.
  depends_on "rust" => :build if stable.url.include?("/archive/refs/tags/")

  head do
    url "https://github.com/abansod/mac-cleaner.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head? || File.exist?("Cargo.toml")
      system "cargo", "install", *std_cargo_args
    else
      bin.install "mac-cleaner"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mac-cleaner --version")
  end
end
