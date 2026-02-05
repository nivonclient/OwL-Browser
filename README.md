# OwL Browser

OwL Browser is a calm, minimalist Linux web browser prototype built with Rust, GTK4, libadwaita, and WebKitGTK 6.0.

## Build (Fedora 43)

Install native dependencies:

```bash
sudo dnf install \
  rust cargo \
  gtk4-devel \
  libadwaita-devel \
  webkitgtk6.0-devel \
  gstreamer1-devel \
  pkgconf-pkg-config
```

Build and run:

```bash
cargo run
```

## Notes

- The browser chrome (tabs, sidebar, omnibox) is rendered in HTML/CSS inside a dedicated WebKit webview.
- The home page is fully local and ships with the binary; it makes zero network requests until you initiate a search.
