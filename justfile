name        := 'cosmic-ext-control-center'
prefix      := '/usr'
bin-dir     := prefix / 'bin'
app-dir     := prefix / 'share' / 'applications'
metainfo-dir:= prefix / 'share' / 'metainfo'

# Default: build both release binaries (the editor + the panel applet).
default: build-release

build-release:
    cargo build --release

# Build, then install everything to {{prefix}} (use sudo, or override prefix for packaging).
install: build-release
    install -Dm0755 target/release/cosmic-ext-control-center \
                    {{bin-dir}}/cosmic-ext-control-center
    install -Dm0755 target/release/cosmic-ext-control-center-applet \
                    {{bin-dir}}/cosmic-ext-control-center-applet
    install -Dm0644 resources/com.pyxyll.CosmicExtControlCenter.desktop \
                    {{app-dir}}/com.pyxyll.CosmicExtControlCenter.desktop
    install -Dm0644 resources/com.pyxyll.CosmicExtControlCenterApplet.desktop \
                    {{app-dir}}/com.pyxyll.CosmicExtControlCenterApplet.desktop
    install -Dm0644 resources/com.pyxyll.CosmicExtControlCenter.metainfo.xml \
                    {{metainfo-dir}}/com.pyxyll.CosmicExtControlCenter.metainfo.xml

uninstall:
    rm -f {{bin-dir}}/cosmic-ext-control-center
    rm -f {{bin-dir}}/cosmic-ext-control-center-applet
    rm -f {{app-dir}}/com.pyxyll.CosmicExtControlCenter.desktop
    rm -f {{app-dir}}/com.pyxyll.CosmicExtControlCenterApplet.desktop
    rm -f {{metainfo-dir}}/com.pyxyll.CosmicExtControlCenter.metainfo.xml

clean:
    cargo clean
