class MacCleaner < Formula
  desc "CleanMyMac-style CLI for junk, clutter, and duplicate cleanup on macOS"
  homepage "https://github.com/abansod/mac-cleaner"
  url "https://github.com/abansod/mac-cleaner/archive/refs/tags/v0.0.1.tar.gz"
  sha256 "4a1c8925ee99dced87332107f5cf7edb40dd7483fd6afe1d78addb2943252556"
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
