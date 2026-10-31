# GPU rendering (phase 7)

Date: 2026-10-31. Phase 7 of `2026-10-29-one-look.md`, which reserved
`render.backend` and left `auto` and `gpu` to this phase.

## 1. What draws

Every surface the shell draws itself goes through one `Renderer`
(`render.rs`): `vello_cpu` into a CPU canvas copied into shm, or
`vello_hybrid` on the Vulkan device behind the compositor's render node
into a canvas texture copied into linear dmabufs (`gpu.rs`). The bar, the
sidebar, the desktop clock, the launcher, panels, tooltips, toasts, the
OSD, the switcher, capture, the screensaver, the modal cards, the lock, the
unlock picture and the greeter all take the GPU when it is up
(`surface.rs` `on_gpu`). Three draw outside that path:

- the pixel dissolve (`surfaces/dissolve.rs`) writes its frames into the
  CPU canvas; on the GPU it draws the same frame as a scene, each
  picture's cells one pixel each stretched back unfiltered, the first cut
  to the cells not yet flipped, equal to the CPU frame pixel for pixel
  (`the_scene_draws_the_frame_the_canvas_gets`);
- the wallpaper (`surface.rs` `Backdrop`) is a picture, not a scene. On
  the CPU its crossfade blends two BGRA pictures into shm; on the GPU each
  picture is uploaded once into a BGRA texture in device memory and the
  crossfade is one full-screen pass into a dmabuf. The service's own copy
  stays where it was; the texture is not in the shell's RSS;
- the compositor-drawn pixel surfaces (scrim, catcher, frame zones, veil)
  are single-pixel buffers and draw nothing.

The device opens on a thread of its own (`gpu-start`): every surface draws
on the CPU until it is up and moves across on its next present. The
driver is opened alone through `VK_LUNARG_direct_driver_loading`, by the
render node's kernel driver; the loader left to itself opens every driver
installed (lavapipe with libLLVM and NVIDIA's beside Intel's on g815: 133
MB and 0.8s, against 15 MB and 10ms for Intel's alone).

## 2. The rule `auto` applies

`auto` reads these facts at start, the first two before any driver loads:

1. The compositor's main device, off linux-dmabuf v4's default feedback,
   and its render node. No dmabuf v4, no render node, or no linear
   ABGR8888 in the feedback: the CPU.
2. Where that node's PCI device sits. Straight on a root bus, with no
   port between (Intel's integrated graphics at 00:02.0): the CPU, with no
   Vulkan driver loaded, since a driver once loaded keeps ~7 MB of the
   shell's RSS for the rest of the run.
3. Otherwise the Vulkan device on that node (`on_node`): a device on
   another node is never taken, so no frame crosses between GPUs, and a
   software device (lavapipe, llvmpipe) is passed over.
4. That device's type: discrete, the GPU; integrated (AMD's APUs sit
   behind a port, so they reach this step), software, virtual or other,
   the CPU.

Integrated GPUs draw on the CPU because of section 4: on both measured, a
frame costs the shell more CPU time on the GPU than on the CPU, the RSS
nearly doubles and the slowest launcher open is later. Only a panel's
first frame is faster. A discrete GPU is not measured: the rig nests on
g815's Intel, and the owner's session on its NVIDIA is not a test target.
It takes the GPU because it is the compositor's own device there and the
per-frame overhead is a submit, not raster work the CPU would also do.
The one discrete host, g815 under Hyprland on its NVIDIA, never gets that
far: the compositor's feedback carries no linear ABGR8888, and the shell
exports linear dmabufs only, so it draws on the CPU (read off its log at
a4ccb4d0). Taking a modifier from the feedback the driver also exports is
what would put it on the GPU.

Power state (battery, power-saver) is not read: the integrated hosts are
on the CPU on mains already, and the quota runs (the power-saver
stand-in) widen the gap the same way.

Every start logs the pick and why, and `debug dump`'s `render.reason`
carries it:

    formalshell: render: drawing on the CPU: auto: /dev/dri/renderD128 is an integrated GPU on the root PCI bus, where the CPU draws for less
    formalshell: render: drawing on the CPU: the compositor takes no linear ABGR8888 dmabuf

`gpu` takes the node's hardware device whatever its type; `cpu` never
opens one.

## 3. The hosts

| Host | Compositor's device | Vulkan device | auto |
| --- | --- | --- | --- |
| g815 (owner's session) | renderD129, nvidia, behind 00:06.0 | NVIDIA GeForce RTX 5070 Laptop GPU, 595.112.07 | CPU, no linear ABGR8888 offered |
| g815 (nested rig) | renderD128, i915, 00:02.0 | Intel(R) Graphics (ARL) | CPU, root bus |
| e1504g | renderD128, i915, 00:02.0 | Intel(R) Graphics (ADL-N) | CPU, root bus |

## 4. Measurements

`dev/perf-gate.py run --backend cpu|gpu` on g815's rig (five idle runs,
three of the rest, medians), and the same legs on e1504g nested in its
own session off a g815 build (`FS_RESULT`, three runs each, one
`--r0-measure 300`). Plain runs; the quota runs move the same way.

| | g815 cpu | g815 gpu | e1504g cpu | e1504g gpu |
| --- | --- | --- | --- | --- |
| cold start, ms | 52 | 55 | 101 | 113 |
| idle CPU, % of a core | 0.0 | 0.0 | 0.0 | 0.0 |
| idle wakeups, 60s | 10 | 9 | 113 | 115 |
| spinning badge CPU, % | 0.82 | 2.38 | 1.58 | 7.67 |
| bar frame, render_us p50 | 48 | 313 | 53 | 936 |
| launcher frame, render_us p50 | 1445 | 1258 | 1948 | 5601 |
| launcher first commit p50 / max, ms | 7 / 11 | 7 / 17 | 12 / 16 | 15 / 28 |
| panel first commit p50, ms | 8 | 3 | 17 | 6 |
| RSS, KB | 58412 | 112384 | 59208 | 105480 |
| threads | 12 | 17 | 12 | 17 |

The GPU's frame cost is fixed per present (scene encoding, a submit, the
ownership barriers and the fence), so it loses most on small frames: a
16px badge costs six to eighteen times the CPU's raster of it. Sizing the
scene to the damage moved the bar's p50 only from 313 to 282us. Presents never
wait on the GPU: a sync_file from the submit goes on the dmabuf as its
write fence, which took the bar's hold under `--menu-budget` from 14 to
24ms to 0. The wallpaper's two pictures live in device memory, not RSS
(`--wallpaper`: 108256 KB on the GPU against 102488 on the CPU, the
difference the driver and the dmabufs). With `auto` on the rig, RSS and
cold start match `cpu` (69616 against 68916 KB, 45 against 45ms).

Frames match: `--render` reads the GPU's launcher card, desktop, panel,
toast and lock against the CPU's at RMSE 0.0037 or under.
