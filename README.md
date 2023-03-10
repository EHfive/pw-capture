# pw-capture

Vulkan/OpenGL layer that captures render images to PipeWire server.

```mermaid
flowchart LR
    subgraph pw-capture Layer
        subgraph src_app[Vulkan/OpenGL App/Game]
            src_frame(frame to be present)
        end
        src_frame -->|copy & export| src_buf[(DMA-BUF fd 0..n)] <==> src_queue
        subgraph src_client[PipeWire Client]
            src_queue[(buffer queue)]
        end
    end

    gst[gst-launch-1.0 pipewiresrc ! ..]
    other[Other PipeWire video sinks/clients]

    src_client -.-> link1 -.-> gst
    src_client -.-> link2 -.-> other
    subgraph server[PipeWire Server]
        link1{{Link}}
        link2{{Link}}
    end
```

| Crate                          |                                                                                           |
| ------------------------------ | ----------------------------------------------------------------------------------------- |
| [pw-capture-client](./client/) | A PipeWire client library specialized for queued video capture                            |
| [pw-capture-vk](./vulkan/)     | A Vulkan layer that can export copies of presented images in DMA-BUF                      |
| [pw-capture-gl](./gl/)         | An OpenGL (GLX/EGL) intercept layer that can export copies of presented images in DMA-BUF |

Inspired by [obs-vkcapture](https://github.com/nowrep/obs-vkcapture).

## Usage

Just launch Vulkan/OpenGL apps with `pw-capture` wrapper, the capture node would now registered on PipeWire graph and waiting for connection from sink nodes (see section [Pipe image datas to GStreamer](#pipe-image-datas-to-gstreamer)).

```bash
pw-capture vkcube
# X11
pw-capture glxgears
pw-capture eglgears_x11
pw-capture glxgears32
# Wayland
pw-capture eglgears_wayland
# Wine apps using DXVK
pw-capture wine some_game.exe
```

Or for OpenGL app:

```bash
# `$LIB` is a dynamic string tokens of ld.so and would
# expands to `lib64` or `lib32` depending on the architecture
env LD_LIBRARY_PATH="/usr/\$LIB" \
    LD_PRELOAD=libpw-capture-gl.so \
    glxgears
```

for Vulkan app:

```bash
ENABLE_PW_CAPTURE=1 vkcube
```

`pw-capture` script is just a combination of two above.

**Note**: use `pw-dump` to inspect the node info and use tools like [pw-viz](https://github.com/Ax9D/pw-viz) or [qpwgraph](https://gitlab.freedesktop.org/rncbc/qpwgraph) to view the node in graph.

### Requirements

- pipewire: `>=0.3.41`
- libffi: Wayland event dispatching

Below are implicit dependencies and would be loaded on demand

- libx11, libxcb: DRI3 buffer export and X11/XCB cursor query
- libwayland-client: Wayland cursor interception
- libglvnd: libEGL, libGLX/libGL interception

### Installation

| Repo       | Package                                                                         |
| ---------- | ------------------------------------------------------------------------------- |
| AUR (Arch) | [pw-capture-git](https://aur.archlinux.org/packages/pw-capture-git)             |
|            | [lib32-pw-capture-git](https://aur.archlinux.org/packages/lib32-pw-capture-git) |

#### Install Manually

We have set up a meson script to make installation more \*unix idiomatic, you could instead follow [Development](#development).

```bash
meson setup builddir --prefix /usr -Dprofile=release
# avoid running cargo in root
meson install -C builddir --destdir destdir
tree builddir/destdir
sudo cp -r builddir/destdir/usr/. /usr
```

Optionally, build layers for 32-bit:

```
export PKG_CONFIG=i686-pc-linux-gnu-pkg-config
meson setup builddir32 --prefix /usr --libdir lib32 \
    -Dprofile=release -Dtarget=i686-unknown-linux-gnu
meson install -C builddir32 --destdir destdir
tree builddir32/destdir
sudo cp -r builddir32/destdir/usr/lib32/. /usr/lib32
```

### Pipe image datas to GStreamer

With latest PipeWire(at least 0.3.66) gst plugins installed, you can pipe the node to other sinks with `pipewiresrc`. Currently it only supports `video/x-raw(memory:DMABuf)`, so you would have to use `gl*` plugins as intermediary.

```bash
# find the node `target-object` with command below
gst-device-monitor-1.0 Video/Source
# or use jq to filter "object.serial" property
pw-dump | jq '.[] | select(.info.props."media.software" == "pw-capture") | .info.props."object.serial"'

# make GL plugins use EGL so it can import DMA-BUF as EGL image than to GL texture,
# not required on Wayland as it uses EGL by default
export GST_GL_PLATFORM=egl

# launch the pipeline, presuming "object.serial" of the layer node is 999
gst-launch-1.0 -e pipewiresrc target-object=999 ! glimagesink ignore-alpha=0

# force GRAY8 format, the color conversion is performed inside layer with `vkCmdBlitImage`
gst-launch-1.0 -e pipewiresrc target-object=999 \
    ! 'video/x-raw(memory:DMABuf),format=GRAY8' ! glimagesink ignore-alpha=0

# to convert `video/x-raw(memory:DMABuf)` to `video/x-raw`, use `glupload ! glcolorconvert ! gldownload`
gst-launch-1.0 -e pipewiresrc target-object=999 \
    ! glupload ! glcolorconvert ! gldownload \
    ! queue ! 'video/x-raw' ! autovideosink

# you can also pipe the src to `v4l2sink` (with v4l2loopback)
# the videorate filter is required as `v4l2sink` does not accept variable framerate
gst-launch-1.0 -e pipewiresrc target-object=999 min-buffers=64 \
    ! glupload ! glcolorconvert ! gldownload \
    ! videorate ! 'video/x-raw,format=YUY2,framerate=60/1' \
    ! v4l2sink device=/dev/video1

# to encode with VA-API
gst-launch-1.0 -e pipewiresrc target-object=999 min-buffers=64 \
    ! glupload ! glcolorconvert ! gldownload \
    ! videorate ! 'video/x-raw,framerate=60/1' ! queue \
    ! vah264enc ! h264parse ! mp4mux ! filesink location=test.mp4
```

## TODO

- [x] Installation script
- [x] OpenGL support
- [x] Passing cursor position & bitmap in buffer meta (X11)
- [x] Wayland cursor capture (by intercepting libwayland-client)
- [x] Better handling of node description & Wine application node name
- [ ] Support export image that maps or copies to memfd as fallback of DMA-BUF export
- [ ] Add more control options (via env vars or config file)
- [ ] Support color conversion to common YUV formats with render pipeline
- [ ] Renegotiate stream format on Vulkan swapchain recreation
- [ ] Allows single buffer display mode
- [ ] Saner error handling, make sure dangling resources are freed before return
- [ ] Support alternative server protocol (may be obs-vkcapture)

## Development

### Vulkan

First build the layer.

```bash
cargo build -p pw-capture-vk
stat ./target/debug/libpw_capture_vk.so
```

Then add layer [manifest](./vulkan/layer.json) to Vulkan loader lookup path and enable it, you can just source the [.envrc](./vulkan/.envrc) file.

```bash
source ./vulkan/.envrc
vulkaninfo | grep pwcapture
```

Now the layer would be loaded by Vulkan loader when Vulkan app launches. We would use `vkcube` (in `vulkan-tools`) here. You would see layer logs if it successfully loaded.

```bash
vkcube
```

You can also find info of the layer created node with `pw-dump`.

```bash
pw-dump | jq '.[] | select(.info.props."media.software" == "pw-capture")'
```

### OpenGL

First build the intercept library.

```bash
cargo build -p pw-capture-gl
stat ./target/debug/libpw_capture_gl.so
```

Set LD_PRELOAD to the path of built library so it can hook onto GLX/EGL functions.

```
export LD_PRELOAD="$(pwd)/target/debug/libpw_capture_gl.so"
```

The intercept layer supports both GLX and EGL, try it out with `glxgears`, `eglgears_x11` or `eglgears_wayland`.
