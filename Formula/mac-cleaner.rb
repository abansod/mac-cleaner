class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.1.0"
  url "https://github.com/abansod/mac-cleaner/releases/download/v0.1.0/mac-cleaner-v0.1.0-macos.tar.gz"
  sha256 "3f6fa30b506701aa399230e7efea02e93887cf0946fbd0c5a40b7397d077c288"
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
