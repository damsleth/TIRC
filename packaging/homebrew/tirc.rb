class Tirc < Formula
  desc "Fast, opinionated terminal IRC client"
  homepage "https://github.com/example/tirc"
  url "https://github.com/example/tirc/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_RELEASE_SHA256"
  license "MIT OR Apache-2.0"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/tirc-app")
  end

  test do
    assert_match "tirc", shell_output("#{bin}/tirc --help")
  end
end
