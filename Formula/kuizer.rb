class Kuizer < Formula
  desc "Keyboard-driven disk cleanup for developers"
  homepage "https://kuizer.app"
  version "0.1.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/remcostoeten/kuizer/releases/download/v0.1.3/kuizer-aarch64-apple-darwin.tar.xz"
      sha256 "9bbd1d68046091ae6d761468a242c1640c91aef5ab851c99527d47f54a884d17"
    end
    on_intel do
      url "https://github.com/remcostoeten/kuizer/releases/download/v0.1.3/kuizer-x86_64-apple-darwin.tar.xz"
      sha256 "de83045711016b5b023f8b9c433c78464e787f7976175f88bc7353c0f21dd5b7"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/remcostoeten/kuizer/releases/download/v0.1.3/kuizer-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "9cbf9bb1c8073c73a4179d094c2079f9ec1f7fc076a3bb2084788dd15bbf2165"
    end
    on_intel do
      url "https://github.com/remcostoeten/kuizer/releases/download/v0.1.3/kuizer-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "5d9152648418e24964e93bff3e2641f9c9d4b2bb30b3f2e7a7b18526062ee9d6"
    end
  end

  livecheck do
    url :stable
    strategy :github_latest
  end

  def install
    bin.install "kuizer"
    # 0.1.3 and older read `kuizer generate` as a folder to scan.
    return if version <= "0.1.3"

    generate_completions_from_executable(bin/"kuizer", "generate", "completions")
    system bin/"kuizer", "generate", "man", "--out", buildpath/"man"
    man1.install buildpath.glob("man/*.1")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kuizer --version")
  end
end
