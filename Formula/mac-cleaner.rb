class MacCleaner < Formula
  desc "CleanMyMac-style CLI for junk, clutter, and duplicate cleanup on macOS"
  homepage "https://github.com/abansod/mac-cleaner"
  url "https://github.com/abansod/mac-cleaner/archive/refs/tags/v0.0.2.tar.gz"
  sha256 "0de8f641150a6e25f29a62060e129f3bf9fb2ba51574023e4bf7cf17ddb604ac"
  license "MIT"
  head "https://github.com/abansod/mac-cleaner.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mac-cleaner --version")
  end
end
