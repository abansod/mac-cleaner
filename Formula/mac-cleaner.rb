class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.0.3"
  url "https://github.com/abansod/mac-cleaner/releases/download/v0.0.3/mac-cleaner-v0.0.3-macos.tar.gz"
  sha256 "765d231dd95d6274f30b1d18ad1d5226cc1edd63533a4a82c077663bb44cf964"
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
