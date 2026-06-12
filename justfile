name        := 'cosmic-control-center'
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
    install -Dm0755 target/release/cosmic-control-center \
                    {{bin-dir}}/cosmic-control-center
    install -Dm0755 target/release/cosmic-control-center-applet \
                    {{bin-dir}}/cosmic-control-center-applet
    install -Dm0644 resources/com.pyxyll.CosmicControlCenter.desktop \
                    {{app-dir}}/com.pyxyll.CosmicControlCenter.desktop
    install -Dm0644 resources/com.pyxyll.CosmicControlCenterApplet.desktop \
                    {{app-dir}}/com.pyxyll.CosmicControlCenterApplet.desktop
    install -Dm0644 resources/com.pyxyll.CosmicControlCenter.metainfo.xml \
                    {{metainfo-dir}}/com.pyxyll.CosmicControlCenter.metainfo.xml

uninstall:
    rm -f {{bin-dir}}/cosmic-control-center
    rm -f {{bin-dir}}/cosmic-control-center-applet
    rm -f {{app-dir}}/com.pyxyll.CosmicControlCenter.desktop
    rm -f {{app-dir}}/com.pyxyll.CosmicControlCenterApplet.desktop
    rm -f {{metainfo-dir}}/com.pyxyll.CosmicControlCenter.metainfo.xml

clean:
    cargo clean
