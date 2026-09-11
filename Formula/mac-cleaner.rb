class MacCleaner < Formula
  desc "CleanMyMac-style CLI for junk, clutter, and duplicate cleanup on macOS"
  homepage "https://github.com/abansod/mac-cleaner"
  url "https://github.com/abansod/mac-cleaner/archive/refs/tags/v0.0.1.tar.gz"
  sha256 "4a1c8925ee99dced87332107f5cf7edb40dd7483fd6afe1d78addb2943252556"
  license "MIT"
  head "https://github.com/abansod/mac-cleaner.git", branch: "main"

  depends_on "python@3.12"
  depends_on :macos

  def install
    venv = virtualenv_create(libexec, "python3.12")
    venv.pip_install buildpath
    bin.install_symlink libexec/"bin/mac-cleaner"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mac-cleaner --version")
  end
end
