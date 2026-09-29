class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.0.9"
  url "https://github.com/abansod/mac-cleaner/releases/download/v0.0.9/mac-cleaner-v0.0.9-macos.tar.gz"
  sha256 "8ece4daeeccd186e68e3ac5774490650860be78853525568ecc852cfa15aa206"
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
