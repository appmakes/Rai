class Rai < Formula
  desc "Run AI instructions directly from your terminal, scripts, and CI/CD pipelines"
  homepage "https://appmakes.github.io/Rai/"
  version "1.0.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/appmakes/Rai/releases/download/v#{version}/rai-aarch64-apple-darwin.tar.gz"
      # sha256 "PLACEHOLDER" # Update after release build completes
    else
      url "https://github.com/appmakes/Rai/releases/download/v#{version}/rai-x86_64-apple-darwin.tar.gz"
      # sha256 "PLACEHOLDER" # Update after release build completes
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/appmakes/Rai/releases/download/v#{version}/rai-aarch64-unknown-linux-gnu.tar.gz"
      # sha256 "PLACEHOLDER" # Update after release build completes
    else
      url "https://github.com/appmakes/Rai/releases/download/v#{version}/rai-x86_64-unknown-linux-gnu.tar.gz"
      # sha256 "PLACEHOLDER" # Update after release build completes
    end
  end

  def install
    bin.install "rai"
  end

  test do
    assert_match "rai", shell_output("#{bin}/rai --version")
  end
end
