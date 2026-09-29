class MacCleaner < Formula
  desc "macOS CLI cleaner for junk, clutter, and duplicate files"
  homepage "https://github.com/abansod/mac-cleaner"
  version ".0.0.5"
  url "https://github.com/abansod/mac-cleaner/releases/download/v.0.0.5/mac-cleaner-v.0.0.5-macos.tar.gz"
  sha256 "a2836507a118d2021a795e9daae478fbd1c34cca62168d7c646c34d08b4b36e2"
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
