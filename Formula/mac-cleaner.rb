class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.0.4"
  url "https://github.com/abansod/mac-cleaner/releases/download/v0.0.4/mac-cleaner-v0.0.4-macos.tar.gz"
  sha256 "859c02f958286f9bb8e53fea0762fc126db4cf716e11e3b76fc7f256778fdba1"
  license "MIT"

  depends_on :macos

  head do
    url "https://github.com/abansod/mac-cleaner.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
    else
      bin.install "mac-cleaner"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mac-cleaner --version")
  end
end
