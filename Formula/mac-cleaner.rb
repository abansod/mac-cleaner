class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version "0.0.7"
  url "https://github.com/abansod/mac-cleaner/releases/download/v0.0.7/mac-cleaner-v0.0.7-macos.tar.gz"
  sha256 "29dd3159c2b25803584dedc1101af667dc4763ae2ce2dc0f22f68926e2f167db"
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
