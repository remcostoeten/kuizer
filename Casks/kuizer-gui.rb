cask "kuizer-gui" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.3"
  sha256 arm:   "34c91c1407210e051bd27c8d85a89fe6f7c3ee1c0fb538e7b3ef9807ef013621",
         intel: "02911fcb7e633e5c289febb9d83118cee1ea37f3a9057ac345561634002a244c"

  url "https://github.com/remcostoeten/kuizer/releases/download/v#{version}/kuizer-gui-#{arch}-apple-darwin.tar.xz"
  name "kuizer"
  desc "Desktop app for keyboard-driven disk cleanup"
  homepage "https://kuizer.app"

  livecheck do
    url :url
    strategy :github_latest
  end

  binary "kuizer-gui-#{arch}-apple-darwin/kuizer-gui"
end
