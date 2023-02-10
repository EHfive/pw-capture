# pw-capture

A Vulkan layer that captures Vulkan display images to PipeWire. (OpenGL support is planned)

| Crate                          |                                                                                           |
| ------------------------------ | ----------------------------------------------------------------------------------------- |
| [pw-capture-client](./client/) | A graphics API agnostic PipeWire client library specialized for queued video capture      |
| [pw-capture-vk](./vulkan/)     | A Vulkan layer that can export copies of presented images to PipeWire server (in DMA-BUF) |

Inspired by [obs-vkcapture](https://github.com/nowrep/obs-vkcapture).

## Usage (Vulkan)

First build the layer.

```bash
cargo build
stat ./target/debug/libpw_capture_vk.so
```

Then add layer [manifest](./vulkan/layer.json) to Vulkan loader lookup path and enable it, we can just source the [.envrc](./vulkan/.envrc) file.

```
source ./vulkan/.envrc
vulkaninfo | grep pwcapture
```

Now we can load the layer, just launch a Vulkan in same shell. We would use `vkcube` (in `vulkan-tools`) here. You would see layer logs if successfully loaded.

```
vkcube
```

You can also find info of layer created node with `pw-dump`.

```
pw-dump | jq '.[] | select(.info.props."media.software" == "pw-capture")'
```

With latest PipeWire(at least [4b60569c](https://gitlab.freedesktop.org/pipewire/pipewire/-/commit/4b60569c4a78987c28b12d7353a687bafee1568e)) gst plugins installed, you can display the node with `glimagesink`.

```
# find the node `target-object` with
gst-device-monitor-1.0 Video/Source

# make GL plugins use EGL so it can import DMA-BUF as EGL buffer,
# not required on Wayland as it uses EGL by default
export GST_GL_PLATFORM=egl

# launch the pipeline
gst-launch-1.0 -e pipewiresrc target-object=999 ! glimagesink ignore-alpha=0

# to convert video/x-raw(memory:DMABuf) to video/x-raw, use glupload ! glcolorconvert ! gldownload
gst-launch-1.0 -e pipewiresrc target-object=999 \
    ! glupload ! glcolorconvert ! gldownload \
    ! queue ! 'video/x-raw' ! autovideosink

# to encode with VA-API
gst-launch-1.0 -e pipewiresrc target-object=999 min-buffers=64 \
    ! glupload ! glcolorconvert ! gldownload \
    ! videorate ! 'video/x-raw,framerate=60/1' ! queue \
    ! vah264enc ! h264parse ! mp4mux ! filesink location=test.mp4
```

## TODO

- [ ] Installation script
- [ ] Support export image mapped to memfd as fallback of DMA-BUF export
- [ ] OpenGL support
- [ ] Support color conversion to common YUV formats with render pipeline
- [ ] Renegotiate stream format on Vulkan swapchain recreation
- [ ] Passing cursor position in buffer meta
- [ ] Better handling of node description & Wine application node name
